use agensi_mcp::{auth, bridge, config, logging::Logger, management, network, project_root};
use std::io::Read;

#[tokio::main]
async fn main() {
    // Vendor protocol diagnostics may include credential bytes; custom safe logging is separate.
    log::set_max_level(log::LevelFilter::Off);
    let root = match project_root() {
        Ok(r) => r,
        Err(_) => {
            eprintln!("Cannot resolve Agensi project root.");
            std::process::exit(1);
        }
    };
    let log = Logger::new(&root, "agensi-native");
    log.event(
        "INFO",
        "startup",
        "Native Rust manager started; credential values omitted.",
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = args.first().map(String::as_str).unwrap_or("bridge");
    let result: anyhow::Result<()> = async {
        let _runtime_guard = if action == "cleanup" {
            None
        } else {
            Some(agensi_mcp::cleanup::runtime_guard(&root)?)
        };
        match action {
            "doctor" => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&agensi_mcp::diagnostics::doctor(&root).await?)?
                );
                Ok(())
            }
            "network-check" => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &agensi_mcp::diagnostics::network_check(&root).await?
                    )?
                );
                Ok(())
            }
            "cleanup" => {
                if args.len() > 2 || args.get(1).is_some_and(|arg| arg != "--apply") {
                    anyhow::bail!("Use cleanup or cleanup --apply.");
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&agensi_mcp::cleanup::run(
                        &root,
                        args.get(1).is_some()
                    )?)?
                );
                Ok(())
            }
            "bridge" => bridge::run(&root).await,
            "http" => agensi_mcp::local_http::run(&root).await,
            "signin" | "--signin" => {
                let cfg = config::load(&root)?;
                auth::signin(&root, network::client(&cfg)?).await
            }
            "probe" => {
                println!("{}", bridge::probe(&root).await?);
                Ok(())
            }
            _ => {
                let mut input = String::new();
                if args.iter().any(|a| a == "--stdin") {
                    std::io::stdin().take(8193).read_to_string(&mut input)?;
                    if input.len() > 8192 {
                        anyhow::bail!("Proxy input too long.");
                    }
                }
                let value = management::command(&root, &args, &input)?;
                println!("{}", serde_json::to_string_pretty(&value)?);
                Ok(())
            }
        }
    }
    .await;
    match result {
        Ok(()) => {
            log.event("INFO", "shutdown", "Execution completed; exit=0.");
        }
        Err(error) => {
            eprintln!("{error}");
            log.event(
                "ERROR",
                "shutdown",
                "Execution failed; raw diagnostics omitted; exit=1.",
            );
            std::process::exit(1);
        }
    }
}
