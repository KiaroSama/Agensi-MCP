use crate::config::Config;
use anyhow::{Result, ensure};

pub const DEFAULT_HTTP_PORT: u16 = 1594;
pub fn http_port(config: &Config) -> Result<u16> {
    match config.extra.get("local_http_port") {
        None => Ok(DEFAULT_HTTP_PORT),
        Some(value) => {
            let port = value
                .as_u64()
                .filter(|p| (1..=65535).contains(p))
                .ok_or_else(|| {
                    anyhow::anyhow!("Local HTTP port must be an integer from 1 to 65535.")
                })?;
            Ok(port as u16)
        }
    }
}
pub fn set_http_port(config: &mut Config, value: &str) -> Result<()> {
    let port: u16 = value
        .parse()
        .map_err(|_| anyhow::anyhow!("Local HTTP port must be an integer from 1 to 65535."))?;
    ensure!(
        port != 0,
        "Local HTTP port must be an integer from 1 to 65535."
    );
    config
        .extra
        .insert("local_http_port".into(), serde_json::json!(port));
    Ok(())
}
