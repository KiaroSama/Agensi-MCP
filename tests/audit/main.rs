//! Explicit Cargo `harness = false` executable; all cases launch the independent native CLI.
mod ci_support;
mod filesystem;
mod fixture;
mod http;
mod management;
mod oauth;
mod project;
mod report;
mod tls_proxy;

use anyhow::{Context as _, Result, bail, ensure};
use project::Context;
use report::Report;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

struct Options {
    source: PathBuf,
    work: PathBuf,
    phase: String,
    output: PathBuf,
    debug: bool,
}
struct Parsed {
    source: PathBuf,
    work: Option<PathBuf>,
    phase: String,
    output: PathBuf,
    debug: bool,
}
fn parse_args(arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<Option<Parsed>> {
    let mut source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut work = None;
    let mut phase = "all".to_owned();
    let mut output = None;
    let mut debug = false;
    let mut normalized = Vec::new();
    for argument in arguments {
        if let Some((key, value)) = argument.to_str().and_then(|value| value.split_once('='))
            && ["--source-root", "--work-root", "--phase", "--output"].contains(&key)
        {
            ensure!(!value.is_empty(), "Audit option requires a value");
            normalized.push(std::ffi::OsString::from(key));
            normalized.push(std::ffi::OsString::from(value));
            continue;
        }
        normalized.push(argument);
    }
    let mut arguments = normalized.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--source-root") => {
                source = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--source-root requires a path"))?
                    .into()
            }
            Some("--work-root") => {
                work =
                    Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        anyhow::anyhow!("--work-root requires a path")
                    })?))
            }
            Some("--phase") => {
                phase = arguments
                    .next()
                    .and_then(|value| value.into_string().ok())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "--phase requires management/filesystem/oauth/http/tls-proxy/all"
                        )
                    })?
            }
            Some("--output") => {
                output =
                    Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        anyhow::anyhow!("--output requires a path")
                    })?))
            }
            Some("--debug") => debug = true,
            Some("--help" | "-h") => {
                println!(
                    "audit_blackbox --source-root PATH --work-root PATH --phase management|filesystem|oauth|http|tls-proxy|all --output PATH [--debug]\nIndependent native CLI cases; all traffic terminates in synthetic loopback fixtures.\nComplete filesystem inventory requires Unix. --output is required."
                );
                return Ok(None);
            }
            _ => bail!("Unknown audit argument; use --help"),
        }
    }
    ensure!(
        [
            "management",
            "filesystem",
            "oauth",
            "http",
            "tls-proxy",
            "all"
        ]
        .contains(&phase.as_str()),
        "Invalid audit phase"
    );
    Ok(Some(Parsed {
        source,
        work,
        phase,
        output: output.ok_or_else(|| anyhow::anyhow!("--output is required"))?,
        debug,
    }))
}
fn options() -> Result<Option<Options>> {
    let Some(Parsed {
        source,
        work,
        phase,
        output,
        debug,
    }) = parse_args(std::env::args_os().skip(1))?
    else {
        return Ok(None);
    };
    let source = source.canonicalize().context("audit-stage:source-root")?;
    let work = work.unwrap_or_else(|| source.join(".tmp/audit-blackbox"));
    let temp = source.join(".tmp");
    reject_links(&temp).context("audit-stage:temp-ancestors")?;
    std::fs::create_dir_all(&temp).context("audit-stage:temp-create")?;
    let temp = temp
        .canonicalize()
        .context("audit-stage:temp-canonicalize")?;
    // Canonicalize the existing ancestor before comparing: Windows adds a verbatim
    // prefix even when --work-root uses an ordinary drive-letter spelling.
    let work = contained(&work, &temp).context("audit-stage:work-containment")?;
    std::fs::create_dir_all(&work).context("audit-stage:work-create")?;
    let work = work
        .canonicalize()
        .context("audit-stage:work-canonicalize")?;
    ensure!(
        work.starts_with(&temp),
        "Work root escapes project temp directory"
    );
    let output = contained(&output, &temp).context("audit-stage:output-containment")?;
    ensure!(
        output != temp && !output.is_dir(),
        "Output must name a report file"
    );
    let binary = source.join("bin").join(if cfg!(windows) {
        "agensi-mcp.exe"
    } else {
        "agensi-mcp"
    });
    ensure!(
        std::fs::symlink_metadata(&binary)?.file_type().is_file(),
        "Expected independent native executable under source-root/bin"
    );
    Ok(Some(Options {
        source,
        work,
        phase,
        output,
        debug,
    }))
}
fn contained(path: &Path, temp: &Path) -> Result<PathBuf> {
    ensure!(
        !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "Audit path must not contain parent traversal"
    );
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    reject_links(&path)?;
    let path = resolve_missing(&path)?;
    ensure!(
        path.starts_with(temp),
        "Audit output/work must stay inside source-root/.tmp"
    );
    Ok(path)
}
fn argument_contracts(source: &Path) -> Result<()> {
    let parse = |args: &[&str]| parse_args(args.iter().map(std::ffi::OsString::from));
    let joined = parse(&[
        "--source-root=source-é space",
        "--work-root=work-é space",
        "--phase=oauth",
        "--output=result-é space.json",
        "--debug",
    ])?
    .ok_or_else(|| anyhow::anyhow!("Equals arguments not parsed"))?;
    let split = parse(&[
        "--source-root",
        "source-é space",
        "--work-root",
        "work-é space",
        "--phase",
        "oauth",
        "--output",
        "result-é space.json",
        "--debug",
    ])?
    .ok_or_else(|| anyhow::anyhow!("Split arguments not parsed"))?;
    ensure!(
        joined.source == split.source
            && joined.work == split.work
            && joined.phase == "oauth"
            && joined.output == split.output
            && joined.debug
            && split.debug,
        "Audit argument spelling contract failed"
    );
    ensure!(
        parse(&["--phase=tls-proxy", "--output=x"])?
            .is_some_and(|parsed| parsed.phase == "tls-proxy"),
        "TLS proxy phase not parsed"
    );
    for args in [
        &["--phase=invalid", "--output=x"][..],
        &["--output="][..],
        &["--output"][..],
        &["--unexpected"][..],
        &[][..],
    ] {
        ensure!(parse(args).is_err(), "Invalid audit arguments accepted");
    }
    let temp = source.join(".tmp").canonicalize()?;
    ensure!(
        contained(&source.join("Cargo.toml"), &temp).is_err(),
        "Audit output escaped temp root"
    );
    ensure!(
        contained(&temp.join("../escape.json"), &temp).is_err(),
        "Audit output traversal accepted"
    );
    ensure!(
        contained(&temp.join("contract-é space.json"), &temp)?.starts_with(&temp),
        "Audit temp report rejected"
    );
    Ok(())
}
fn resolve_missing(path: &Path) -> Result<PathBuf> {
    let mut existing = path;
    let mut suffix = Vec::new();
    while !existing.exists() {
        suffix.push(
            existing
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid work root"))?
                .to_owned(),
        );
        existing = existing
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Missing work root ancestor"))?;
    }
    let mut resolved = existing.canonicalize()?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}
fn reject_links(path: &Path) -> Result<()> {
    // Prefix-only Windows paths (such as \\?\D:) are not filesystem objects.
    // Check complete ancestors instead, retaining drive-root and every existing directory.
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "Linked audit output/work ancestor refused"
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        metadata.file_attributes() & 0x400 == 0,
                        "Reparse audit output/work ancestor refused"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
async fn run(options: Options) -> Result<bool> {
    // One owned invocation gets a unique root so concurrent jobs cannot share CA/pause state.
    let owned = tempfile::Builder::new()
        .prefix("audit invocation-é-")
        .tempdir_in(&options.work)?;
    let work = owned.path().canonicalize()?;
    let mut report = Report::new(&work, options.debug)?;
    report.event("INFO", "startup isolated synthetic loopback-only audit")?;
    let mut context = Context::new(options.source.clone(), work);
    let result: Result<()> = async {
        argument_contracts(&options.source)?;
        for phase in ["management", "filesystem", "oauth", "http", "tls-proxy"] {
            if (options.phase == "all" && phase == "tls-proxy")
                || (options.phase != "all" && options.phase != phase)
            {
                continue;
            }
            report.event(
                "INFO",
                &format!("Starting {phase} cases with isolated synthetic fixtures"),
            )?;
            match phase {
                "management" => management::cases(&context, &mut report).await?,
                "filesystem" => filesystem::cases(&mut context, &mut report).await?,
                "oauth" => oauth::cases(&mut context, &mut report).await?,
                "tls-proxy" => http::tls_proxy_cases(&mut context, &mut report).await?,
                _ => http::cases(&mut context, &mut report).await?,
            }
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        // Never emit error payloads which might carry authorization URLs or fixture state.
        report.record(
            "harness-completion",
            false,
            json!({"exception_type":"RustError"}),
            None,
        )?;
    }
    if report.inventory(&options.source, &options.phase).is_err() {
        report.record(
            "complete-inventory",
            false,
            json!({"error":"Source case inventory invalid or unavailable"}),
            None,
        )?;
    }
    let passed = report.finish(&options.source, &options.phase, &options.output)?;
    context.certificates.take();
    drop(report);
    if passed {
        owned.close()?;
    } else {
        let _ = owned.keep();
    }
    Ok(passed)
}
fn main() -> ExitCode {
    let options = match options() {
        Ok(Some(options)) => options,
        Ok(None) => return ExitCode::SUCCESS,
        Err(error) => {
            let stage = error
                .chain()
                .find_map(|cause| {
                    let message = cause.to_string();
                    message
                        .strip_prefix("audit-stage:")
                        .filter(|value| {
                            value
                                .bytes()
                                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
                        })
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "arguments-or-binary".into());
            eprintln!("Audit prerequisite failed stage={stage}; use --help. Payload omitted.");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("Audit runtime initialization failed; payload omitted.");
            return ExitCode::from(2);
        }
    };
    let result = runtime.block_on(run(options));
    runtime.shutdown_timeout(std::time::Duration::from_secs(3));
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(_) => {
            eprintln!("Audit failed to complete or persist its report; payload omitted.");
            ExitCode::from(2)
        }
    }
}
