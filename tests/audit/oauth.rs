//! Real subprocess sign-in and raw callback mutations against the local TLS fixture.
use crate::{
    project::{CLI_BOUND, Callback, Context, raw_callback, reply_status},
    report::Report,
};
use anyhow::Result;
use serde_json::json;
use tokio::time::Duration;

pub async fn cases(context: &mut Context, report: &mut Report) -> Result<()> {
    for (protocol, user) in [
        ("http", false),
        ("socks4", false),
        ("socks4a", false),
        ("socks5", false),
        ("socks5h", false),
        ("socks4", true),
        ("socks4a", true),
    ] {
        let mut project = context.project()?;
        let result: Result<()> = async {
            let (index, success) = context.signin(&mut project, protocol, user).await?;
            project.processes[index].alive()?;
            report.record(format!("oauth-{protocol}-userid-{}",if user {"True"} else {"False"}), success, json!({"exit":project.processes[index].exit,"session_saved":project.session_exists(),"proxy_connections":project.fixture.as_ref().expect("fixture").connections()}), if user {Some("F01")} else {None})
        }.await;
        project.close().await?;
        result?;
    }
    for mutation in [
        "fragment",
        "invalid-utf8",
        "idle",
        "wrong-state",
        "duplicate-state",
        "duplicate-code",
        "wrong-host",
        "missing-host",
        "duplicate-host",
        "mixed-case-host",
    ] {
        let mut project = context.project()?;
        let result: Result<()> = async {
            context.route(&mut project, "http", false).await?;
            let index = project.start("signin")?;
            if !project.processes[index].ready(false, CLI_BOUND).await {
                report.record(format!("oauth-callback-{mutation}"), false, json!({"fixture_ready":false}), None)?;
                return Ok(());
            }
            project.bind_authorization(index)?;
            let (status, passed) = match mutation {
                "invalid-utf8" | "idle" => {
                    let (port, _) = project.processes[index].callback_target(&Callback::default())?;
                    let input = if mutation == "idle" { b"".as_slice() } else { b"GET /oauth/callback?x=\xff HTTP/1.1\r\n\r\n".as_slice() };
                    let reply = raw_callback(port, input, false, Duration::from_secs(if mutation == "idle" {6} else {2})).await?;
                    let refused = reply_status(&reply) == Some(400);
                    let alive = project.processes[index].alive()?;
                    let status = if alive { project.processes[index].callback(Callback::default()).await? } else { None };
                    (status, refused && alive && status == Some(200))
                }
                "fragment" => {
                    let status = project.processes[index].callback(Callback { fragment:true, ..Callback::default() }).await?;
                    (status, status == Some(200))
                }
                "wrong-state" => {
                    let status = project.processes[index].callback(Callback { wrong_state:true, ..Callback::default() }).await?;
                    let valid = project.processes[index].callback(Callback::default()).await?;
                    (status, status == Some(400) && valid == Some(200))
                }
                "mixed-case-host" => {
                    let status = project.processes[index].callback(Callback { headers:Some("hOsT: 127.0.0.1:{port}\r\n"), ..Callback::default() }).await?;
                    (status, status == Some(200))
                }
                "wrong-host" | "missing-host" | "duplicate-host" => {
                    let mutation = match mutation {
                        "wrong-host" => Callback { host:Some("evil.invalid"), ..Callback::default() },
                        "missing-host" => Callback { headers:Some(""), ..Callback::default() },
                        _ => Callback { headers:Some("Host: 127.0.0.1:{port}\r\nHost: 127.0.0.1:{port}\r\n"), ..Callback::default() },
                    };
                    let status = project.processes[index].callback(mutation).await?;
                    let rejected = status == Some(400) && !project.session_exists();
                    let valid = project.processes[index].callback(Callback::default()).await?;
                    (status, rejected && valid == Some(200))
                }
                _ => {
                    let extra = if mutation == "duplicate-state" { "&state=other" } else { "&code=other" };
                    let status = project.processes[index].callback(Callback { extra, ..Callback::default() }).await?;
                    (status, status == Some(400) && !project.session_exists())
                }
            };
            let alive = project.processes[index].alive()?;
            report.record(format!("oauth-callback-{mutation}"), passed, json!({"callback_status":status,"process_alive":alive,"session_saved":project.session_exists()}), Some("F02"))
        }.await;
        project.close().await?;
        result?;
    }
    let mut project = context.project()?;
    let result: Result<()> = async {
        context.route(&mut project, "http", false).await?;
        let index = project.start("signin")?;
        let ready = project.processes[index].ready(false, CLI_BOUND).await;
        let disconnected = project.cli(&["disconnect"]).await?;
        let exit = project.processes[index]
            .wait(Duration::from_millis(550))
            .await?;
        let alive = project.processes[index].alive()?;
        report.record(
            "oauth-pending-revoked-on-disconnect",
            ready && disconnected.success() && exit.is_some() && !alive,
            json!({"still_waiting_after_550ms":alive}),
            Some("F03"),
        )
    }
    .await;
    project.close().await?;
    result?;
    refresh_cases(context, report).await?;
    shutdown_cases(context, report).await
}

async fn refresh_cases(context: &mut Context, report: &mut Report) -> Result<()> {
    for (name, expiry, rejected, missing_refresh) in [
        ("extension-signin-short-expiry-refresh", 10, false, false),
        (
            "extension-signin-long-expiry-no-refresh",
            3600,
            false,
            false,
        ),
        ("extension-signin-failed-refresh", 10, true, false),
        ("extension-signin-missing-refresh-token", 10, false, true),
    ] {
        let mut project = context.project()?;
        let result: Result<()> = async {
            context.route(&mut project, "http", false).await?;
            project.fixture.as_ref().expect("Fixture").token_policy(expiry, rejected);
            if missing_refresh { project.fixture.as_ref().expect("Fixture").omit_refresh_token(); }
            let (index, success) = context.signin(&mut project, "http", false).await?;
            let (exchanges, refreshes) = project.fixture.as_ref().expect("Fixture").token_counts();
            let metadata = if project.session_exists() {
                let saved = project.read_json(".state/auth/rust-session.json")?;
                saved["client_id"] == "fixture-client" && saved["issuer"] == crate::fixture::ISSUER && saved["token_received_at"].is_number() && saved["granted_scopes"].as_array().is_some_and(|scopes| scopes.contains(&json!("read")) && scopes.contains(&json!("install")))
            } else { false };
            let failure = rejected || missing_refresh;
            let expected_exit = if failure { Some(1) } else { Some(0) };
            report.extension(name,
                success != failure && project.processes[index].exit == expected_exit && exchanges == 1 && refreshes == usize::from(expiry < 30 && !missing_refresh) && metadata,
                json!({"exit":project.processes[index].exit,"exchange_count":exchanges,"refresh_count":refreshes,"saved_metadata_valid":metadata}))
        }.await;
        project.close().await?;
        result?;
    }
    let mut project = context.project()?;
    let result: Result<()> = async {
        context.route(&mut project, "http", false).await?;
        let index = project.start("signin")?;
        let ready = project.processes[index].ready(false, CLI_BOUND).await;
        let competing = project.start("signin")?;
        let exit = project.processes[competing].wait(Duration::from_secs(6)).await?;
        let first_alive = project.processes[index].alive()?;
        let mut success = false;
        if ready && first_alive {
            project.bind_authorization(index)?;
            let status = project.processes[index].callback(Callback::default()).await?;
            success = status == Some(200) && project.processes[index].wait(CLI_BOUND).await? == Some(0);
        }
        report.extension("extension-signin-independent-guard", ready && first_alive && exit == Some(1) && success,
            json!({"competing_exit":exit,"first_retained_exclusion":first_alive,"first_completed":success}))
    }.await;
    project.close().await?;
    result
}

async fn shutdown_cases(context: &mut Context, report: &mut Report) -> Result<()> {
    for mode in ["interrupt-open-stdin", "eof", "route-error"] {
        let mut project = context.project()?;
        let result: Result<()> = async {
            let (_, signed_in) = context.signin(&mut project, "http", false).await?;
            let index = project.start("bridge")?;
            let ready = project.processes[index].initialize().await?;
            let started = std::time::Instant::now();
            match mode {
                "interrupt-open-stdin" => {
                    #[cfg(unix)]
                    project.processes[index].interrupt()?;
                    #[cfg(not(unix))]
                    anyhow::bail!("Owned hidden interrupt delivery is not supported on this audit host");
                }
                "eof" => project.processes[index].close_input(),
                _ => { let disconnected = project.cli(&["disconnect"]).await?; anyhow::ensure!(disconnected.success(), "Route control failed"); }
            }
            let input_before_wait = project.processes[index].input_open();
            let exit = project.processes[index].wait(Duration::from_secs(9)).await?;
            let input_after_wait = project.processes[index].input_open();
            let input_contract = input_before_wait == (mode != "eof") && input_after_wait == input_before_wait;
            report.extension(&format!("extension-bridge-{mode}"), signed_in && ready && input_contract && exit == Some(if mode == "route-error" {1} else {0}),
                json!({"protocol_ready":ready,"exit":exit,"elapsed_ms":started.elapsed().as_millis(),"input_held_open_before_wait":input_before_wait,"input_held_open_after_wait":input_after_wait}))
        }.await;
        project.close().await?;
        result?;
    }
    Ok(())
}
