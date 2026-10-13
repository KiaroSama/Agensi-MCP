//! FIFO and linked-file behavior is exercised through the real CLI, not core helpers.
use crate::{project::Context, report::Report};
use anyhow::Result;
#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use tokio::time::Duration;

#[cfg(not(unix))]
pub async fn cases(_context: &mut Context, _report: &mut Report) -> Result<()> {
    anyhow::bail!(
        "Filesystem audit requires Unix FIFO and symlink semantics; run the complete inventory in Linux CI"
    )
}
#[cfg(unix)]
pub async fn cases(context: &mut Context, report: &mut Report) -> Result<()> {
    use std::os::unix::fs::symlink;
    for kind in ["config", "session", "import", "skill-input"] {
        let project = context.project()?;
        let result: Result<()> = async {
            project.default_config()?;
            let name = match kind {
                "config" => "config.local.json",
                "session" => ".state/auth/rust-session.json",
                "import" => "fifo-import",
                _ => "fifo-skill",
            };
            let path = project.root.join(name);
            std::fs::create_dir_all(path.parent().expect("FIFO parent"))?;
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            fifo(&path)?;
            let args: Vec<&std::ffi::OsStr> = match kind {
                "config" => vec![std::ffi::OsStr::new("status")],
                "session" => vec![std::ffi::OsStr::new("doctor")],
                "import" => vec![std::ffi::OsStr::new("import"), path.as_os_str()],
                _ => vec![
                    std::ffi::OsStr::new("save-skill"),
                    std::ffi::OsStr::new("fixture"),
                    path.as_os_str(),
                ],
            };
            let run = project.run(&args, None, Duration::from_millis(900)).await?;
            let refused = if kind == "session" {
                run.exit == Some(0)
                    && run.json.as_ref().is_some_and(|value| {
                        value["session"]["readable"] == false
                            && value["session"]["usable"] == "unknown"
                    })
            } else {
                run.exit.is_some_and(|exit| exit != 0)
            };
            report.record(
                format!("fifo-{kind}-bounded"),
                !run.timed_out && refused,
                json!({"timeout":run.timed_out,"bound_seconds":0.9,"refused":refused}),
                Some("F05"),
            )
        }
        .await;
        project.close().await?;
        result?;
    }
    let project = context.project()?;
    let result: Result<()> = async {
        std::fs::write(project.root.join("logs"), "synthetic obstruction")?;
        let run = project.cli(&["status"]).await?;
        let present = run.stderr.contains("[INFO]");
        report.record(
            "stderr-logging-fallback-events",
            run.success()
                && present
                && run.stderr.contains("startup")
                && run.stderr.contains("shutdown"),
            json!({"exit":run.exit,"timestamped_info_present":present}),
            Some("F07"),
        )
    }
    .await;
    project.close().await?;
    result?;
    let project = context.project()?;
    let result: Result<()> = async {
        project.default_config()?;
        let outside = project.root.join("outside-synthetic-state");
        std::fs::create_dir(&outside)?;
        symlink(&outside, project.root.join(".state"))?;
        let run = project.cli(&["status"]).await?;
        report.record(
            "status-does-not-touch-unused-state-link",
            run.success() && std::fs::read_dir(&outside)?.next().is_none(),
            json!({"exit":run.exit}),
            None,
        )
    }
    .await;
    project.close().await?;
    result?;
    let mut project = context.project()?;
    let result: Result<()> = async {
        context.route(&mut project, "http", false).await?;
        std::fs::create_dir_all(project.root.join(".state"))?;
        let outside = project.root.join("outside-synthetic-auth");
        std::fs::create_dir(&outside)?;
        symlink(&outside, project.root.join(".state/auth"))?;
        let index = project.start("signin")?;
        project.processes[index]
            .ready(false, Duration::from_secs(1))
            .await;
        project.processes[index].stop().await?;
        let mut children = Vec::new();
        for entry in std::fs::read_dir(outside)? {
            children.push(entry?.file_name().to_string_lossy().into_owned());
        }
        children.sort();
        report.record(
            "linked-auth-directory-never-written",
            children.is_empty(),
            json!({"outside_names":children}),
            Some("F05"),
        )
    }
    .await;
    project.close().await?;
    result?;
    let project = context.project()?;
    let result: Result<()> = async {
        project.default_config()?;
        std::fs::create_dir_all(project.root.join(".state/auth"))?;
        project.write_json("outside-session.json", &json!({"client_id":"synthetic-client","token_response":null,"requested_scopes":[],"token_received_at":null,"issuer":null}))?;
        symlink(project.root.join("outside-session.json"), project.root.join(".state/auth/rust-session.json"))?;
        let run = project.cli(&["doctor"]).await?;
        let readable = run.json.as_ref().map(|value| value["session"]["readable"].clone()).unwrap_or(serde_json::Value::Null);
        report.record("linked-session-not-loaded", !run.timed_out && readable != true, json!({"exit":run.exit,"readable":readable}), Some("F05"))
    }.await;
    project.close().await?;
    result
}
#[cfg(unix)]
fn fifo(path: &std::path::Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    #[cfg(target_vendor = "apple")]
    type Mode = u16;
    #[cfg(not(target_vendor = "apple"))]
    type Mode = u32;
    unsafe extern "C" {
        fn mkfifo(path: *const std::ffi::c_char, mode: Mode) -> std::ffi::c_int;
    }
    let path = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: CString is live and NUL-terminated, 0600 is a valid POSIX mode;
    // mkfifo neither retains the pointer nor transfers Rust ownership.
    if unsafe { mkfifo(path.as_ptr(), 0o600) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
