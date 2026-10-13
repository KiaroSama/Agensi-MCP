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
    result
}
