//! Local HTTP authority/body boundaries and disconnect races through independent children.
use crate::{
    project::{CLI_BOUND, Context},
    report::Report,
};
use anyhow::{Result, ensure};
use futures::StreamExt;
use serde_json::json;
use tokio::{net::TcpListener, time::Duration};

async fn free_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    Ok(listener.local_addr()?.port())
}
async fn request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: Option<&str>,
) -> Result<u16> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(CLI_BOUND)
        .build()?;
    let mut request = client
        .request(
            method.parse::<reqwest::Method>()?,
            format!("http://127.0.0.1:{port}{path}"),
        )
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");
    for (key, value) in headers {
        request = request.header(*key, value);
    }
    let default = json!({"jsonrpc":"2.0","id":7,"method":"tools/list","params":{}}).to_string();
    let response = request
        .body(body.unwrap_or(&default).to_owned())
        .send()
        .await?;
    let status = response.status().as_u16();
    let mut received = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        received += chunk?.len();
        ensure!(received <= 2 * 1024 * 1024, "Local HTTP response bound");
    }
    Ok(status)
}

pub async fn cases(context: &mut Context, report: &mut Report) -> Result<()> {
    let mut project = context.project()?;
    let result: Result<()> = async {
        let (_, success) = context.signin(&mut project, "http", false).await?;
        if !success {
            report.record("http-fixture-signin", false, json!({}), None)?;
            return Ok(());
        }
        let port = free_port().await?;
        let port_text = port.to_string();
        let configured = project.cli(&["http-port", &port_text]).await?;
        let index = project.start("http")?;
        let ready = project.processes[index].ready(true, CLI_BOUND).await;
        report.record(
            "http-startup-from-stored-session",
            ready && configured.success(),
            json!({"ready":ready}),
            None,
        )?;
        if !ready {
            return Ok(());
        }
        let cases = [
            ("ordinary-post", "POST", "/mcp", vec![], 200),
            (
                "foreign-origin",
                "POST",
                "/mcp",
                vec![("Origin", "https://evil.invalid".into())],
                403,
            ),
            (
                "null-origin",
                "POST",
                "/mcp",
                vec![("Origin", "null".into())],
                403,
            ),
            (
                "wrong-host",
                "POST",
                "/mcp",
                vec![("Host", "evil.invalid".into())],
                403,
            ),
            (
                "query-refused",
                "POST",
                "/mcp?target=elsewhere",
                vec![],
                400,
            ),
            ("method-refused", "PUT", "/mcp", vec![], 405),
            ("wrong-path", "POST", "/other", vec![], 404),
            (
                "same-origin",
                "POST",
                "/mcp",
                vec![("Origin", format!("http://127.0.0.1:{port}"))],
                200,
            ),
        ];
        for (name, method, path, headers, expected) in cases {
            let before = project.fixture.as_ref().expect("fixture").requests().len();
            let status = request(port, method, path, &headers, None).await?;
            let forwarded = project.fixture.as_ref().expect("fixture").requests().len() - before;
            report.record(
                format!("local-http-{name}"),
                status == expected && (expected == 200 || forwarded == 0),
                json!({"status":status,"forwarded_requests":forwarded}),
                None,
            )?;
        }
        let status = request(
            port,
            "POST",
            "/mcp",
            &[],
            Some(&"x".repeat(2 * 1024 * 1024 + 1)),
        )
        .await?;
        report.record(
            "local-http-request-size-limit",
            status == 413,
            json!({"status":status}),
            None,
        )?;
        let disconnected = project.cli(&["disconnect"]).await?;
        let exit = project.processes[index]
            .wait(Duration::from_secs(1))
            .await?;
        let alive = project.processes[index].alive()?;
        report.record(
            "local-http-idle-disconnect",
            disconnected.success() && !alive && exit.is_some(),
            json!({"exit":project.processes[index].exit}),
            None,
        )
    }
    .await;
    project.close().await?;
    result?;
    let mut project = context.project()?;
    let result: Result<()> = async {
        let (_, success) = context.signin(&mut project, "http", false).await?;
        let fixture = project.fixture.as_ref().expect("fixture");
        fixture.pause("/mcp", Some("initialize"));
        let index = project.start("probe")?;
        let paused = project.fixture.as_ref().expect("fixture").wait_paused(CLI_BOUND).await;
        let before = project.fixture.as_ref().expect("fixture").requests().len();
        let disconnected = project.cli(&["disconnect"]).await?;
        project.processes[index].wait(Duration::from_millis(550)).await?;
        project.fixture.as_ref().expect("fixture").resume();
        let exit = project.processes[index].wait(CLI_BOUND).await?;
        project.processes[index].stop().await?;
        let requests = project.fixture.as_ref().expect("fixture").requests();
        let later = &requests[before..];
        report.record("probe-no-requests-after-disconnect", success && disconnected.success() && paused && exit.is_some() && later.is_empty(), json!({"fixture_paused":paused,"post_disconnect_request_count":later.len(),"post_disconnect_methods":later.iter().map(|row| row["rpc_method"].clone()).collect::<Vec<_>>(),"exit":project.processes[index].exit}), Some("F03"))
    }.await;
    project.close().await?;
    result?;
    let mut project = context.project()?;
    let result: Result<()> = async {
        let (_, success) = context.signin(&mut project, "http", false).await?;
        let port = free_port().await?;
        let configured = project.cli(&["http-port", &port.to_string()]).await?;
        project.fixture.as_ref().expect("fixture").pause("/.well-known/oauth-authorization-server", None);
        let index = project.start("http")?;
        let paused = project.fixture.as_ref().expect("fixture").wait_paused(Duration::from_millis(1500)).await;
        if paused {
            let disconnected = project.cli(&["disconnect"]).await?;
            project.fixture.as_ref().expect("fixture").resume();
            let exit = project.processes[index].wait(Duration::from_secs(1)).await?;
            project.processes[index].stop().await?;
            let started = project.processes[index].http_ready();
            let mut forwarded = 0;
            let mut status = None;
            if started {
                let before = project.fixture.as_ref().expect("fixture").requests().len();
                status = request(port, "POST", "/mcp", &[], None).await.ok();
                forwarded = project.fixture.as_ref().expect("fixture").requests().len() - before;
            }
            report.record("http-startup-route-snapshot-consistent", success && configured.success() && disconnected.success() && exit.is_some() && !started && forwarded == 0, json!({"fixture_paused":true,"listener_started":started,"request_status":status,"forwarded_after_disconnect":forwarded}), Some("F03"))
        } else {
            report.record("http-startup-race-fixture-reached", false, json!({"note":"No startup network request; this fixture cannot force the source-level race."}), None)
        }
    }.await;
    project.close().await?;
    result
}
