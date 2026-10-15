use agensi_mcp::logging::Logger;
use serde::Deserialize;
use std::{env, ffi::OsString, fs, io::Write};

pub fn validate(
    version: &str,
    tag: &str,
    reference: &str,
    in_ci: bool,
) -> Result<(), &'static str> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Package version is not a supported stable release version.");
    }
    if !tag.is_empty() && tag != format!("v{version}") {
        return Err("Release tag must exactly match the Cargo package version.");
    }
    if !tag.is_empty() && in_ci && reference != "refs/heads/main" {
        return Err("Versioned release publication requires the main branch.");
    }
    Ok(())
}

pub fn reviewed_signing_branch(
    reference: &str,
    repository: &str,
    actor: &str,
    event: &str,
) -> bool {
    reference == "refs/heads/fix/private-signed-release"
        && repository == "KiaroSama/Agensi-MCP"
        && actor == "KiaroSama"
        && event == "workflow_dispatch"
}

#[derive(Deserialize)]
struct Manifest {
    package: Package,
}
#[derive(Deserialize)]
struct Package {
    version: String,
}

pub fn main(args: Vec<OsString>) -> i32 {
    let root = match agensi_mcp::project_root() {
        Ok(root) => root,
        Err(_) => {
            eprintln!("Release installation root unavailable.");
            return 1;
        }
    };
    let mut manifest = root.join("Cargo.toml");
    let mut tag = env::var("RELEASE_TAG").unwrap_or_default();
    let mut debug = false;
    let mut args = expand_options(args).into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--manifest") => match args.next() {
                Some(path) => manifest = path.into(),
                None => return usage(),
            },
            Some("--tag") => match args.next().and_then(|v| v.into_string().ok()) {
                Some(value) => tag = value,
                None => return usage(),
            },
            Some("--debug") => debug = true,
            Some("--help") | Some("-h") => {
                println!("check-run release [--manifest PATH] [--tag vX.Y.Z] [--debug]");
                return 0;
            }
            _ => return usage(),
        }
    }
    execute(root, manifest, tag, debug)
}

pub fn expand_options(args: Vec<OsString>) -> Vec<OsString> {
    let mut expanded = Vec::new();
    for arg in args {
        if let Some(text) = arg.to_str()
            && let Some((key, value)) = text.split_once('=')
            && matches!(key, "--manifest" | "--tag")
        {
            expanded.push(OsString::from(key));
            expanded.push(OsString::from(value));
        } else {
            expanded.push(arg);
        }
    }
    expanded
}

fn execute(
    root: std::path::PathBuf,
    manifest: std::path::PathBuf,
    tag: String,
    debug: bool,
) -> i32 {
    let log = Logger::new(&root, "release");
    if debug {
        log.event("DEBUG", "release", "Validating release identity.");
    }
    let result = (|| -> anyhow::Result<String> {
        let document: Manifest = toml::from_str(&fs::read_to_string(manifest)?)?;
        let reference = env::var("GITHUB_REF").unwrap_or_default();
        let reviewed = reviewed_signing_branch(
            &reference,
            &env::var("GITHUB_REPOSITORY").unwrap_or_default(),
            &env::var("GITHUB_ACTOR").unwrap_or_default(),
            &env::var("GITHUB_EVENT_NAME").unwrap_or_default(),
        );
        // This exception validates assets only; the publication job remains main-only.
        validate(
            &document.package.version,
            &tag,
            &reference,
            env::var("GITHUB_ACTIONS").as_deref() == Ok("true") && !reviewed,
        )
        .map_err(anyhow::Error::msg)?;
        if let Some(output) = env::var_os("GITHUB_OUTPUT").filter(|v| !v.is_empty()) {
            let mut stream = fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(output)?;
            writeln!(stream, "version={}", document.package.version)?;
            stream.flush()?;
        }
        Ok(document.package.version)
    })();
    match result {
        Ok(version) => {
            let message = format!(
                "Release identity validated; version={version} mode={}",
                if tag.is_empty() {
                    "checks/artifacts"
                } else {
                    "release"
                }
            );
            log.event("INFO", "release", &message);
            eprintln!(
                "[{} UTC] [INFO] [release] {message}",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
            );
            0
        }
        Err(_) => {
            let message = "Release identity validation failed; payload omitted; no assets should be published.";
            log.event("ERROR", "release", message);
            eprintln!(
                "[{} UTC] [ERROR] [release] {message}",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
            );
            1
        }
    }
}
fn usage() -> i32 {
    eprintln!("Expected check-run release [--manifest PATH] [--tag vX.Y.Z] [--debug].");
    2
}
