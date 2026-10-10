use std::process::Command;
#[cfg(windows)]
use std::{io::Write, process::Stdio, time::Instant};

#[test]
fn native_entry_status_from_foreign_cwd_and_failure_exit() {
    let bin = env!("CARGO_BIN_EXE_agensi-mcp");
    let cwd = tempfile::tempdir().expect("cwd");
    let output = Command::new(bin)
        .arg("status")
        .current_dir(cwd.path())
        .output()
        .expect("status");
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("status JSON");
    assert!(json.get("mode").is_some());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("password"));
    let failed = Command::new(bin)
        .arg("invalid-command")
        .current_dir(cwd.path())
        .output()
        .expect("failure");
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Use status"));
}

#[test]
#[cfg(windows)]
fn native_menu_has_colon_and_signin_without_profile_dump() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let release = root.join("bin/agensi-mcp.exe");
    assert!(
        release.exists(),
        "Build/copy native binary for launcher testing."
    );
    let mut child = Command::new("pwsh")
        .args(["-NoLogo", "-NoProfile", "-File"])
        .arg(root.join("run.ps1"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("menu process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"exit\n")
        .expect("menu input");
    let started = Instant::now();
    loop {
        if child.try_wait().expect("menu state").is_some() {
            break;
        }
        if started.elapsed().as_secs() > 6 {
            child.kill().expect("kill timed out menu");
            let _ = child.wait();
            panic!("Menu exceeded six-second startup budget");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("menu output");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("UTF8 menu");
    assert!(text.contains("quit=exit: "));
    assert!(text.contains("7. Sign in"));
    assert!(!text.contains("socks5://"));
}
