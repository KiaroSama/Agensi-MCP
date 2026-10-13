#[path = "../check/process.rs"]
pub mod process;
pub mod runner_fixture;

#[cfg(windows)]
mod windows {
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
        time::Duration,
    };
    type Result<T> = anyhow::Result<T>;

    struct Menu {
        directory: tempfile::TempDir,
    }
    struct Output {
        code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }
    impl Menu {
        fn new() -> Result<Self> {
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            fs::create_dir_all(source.join(".tmp"))?;
            let directory = tempfile::Builder::new()
                .prefix("menu input-")
                .tempdir_in(source.join(".tmp"))?;
            fs::create_dir(directory.path().join("bin"))?;
            for path in ["run.ps1", "Cargo.toml", "bin/agensi-mcp.exe"] {
                fs::copy(source.join(path), directory.path().join(path))?;
            }
            Ok(Self { directory })
        }
        fn root(&self) -> &Path {
            self.directory.path()
        }
        fn launch(&self, input: &str, options: &[&str]) -> Result<Output> {
            let mut command: Vec<OsString> = ["pwsh", "-NoLogo", "-NoProfile", "-File"]
                .map(OsString::from)
                .into();
            command.push(self.root().join("run.ps1").into_os_string());
            command.extend(options.iter().map(OsString::from));
            let captured = std::panic::catch_unwind(|| {
                crate::runner_fixture::capture_with_input(
                    &self.root().join("bin"),
                    &command,
                    input.as_bytes(),
                    Duration::from_secs(10),
                )
            })
            .map_err(|_| anyhow::anyhow!("Menu capture or owned cleanup failed."))?;
            anyhow::ensure!(
                captured.stdout.len() < 1024 * 1024 && captured.stderr.len() < 1024 * 1024,
                "Menu output exceeded its capture bound."
            );
            Ok(Output {
                code: captured.status.code().unwrap_or(1),
                stdout: captured.stdout,
                stderr: captured.stderr,
            })
        }
    }
    fn count(bytes: &[u8], needle: &[u8]) -> usize {
        bytes
            .windows(needle.len())
            .filter(|part| *part == needle)
            .count()
    }
    fn test_advertised_quit_alias_exits(menu: &Menu) -> Result<()> {
        anyhow::ensure!(menu.launch("quit\n", &[])?.code == 0, "quit alias failed");
        Ok(())
    }
    fn test_existing_exit_aliases_remain_supported(menu: &Menu) -> Result<()> {
        for choice in ["exit\n", "0\n"] {
            anyhow::ensure!(menu.launch(choice, &[])?.code == 0, "exit alias failed");
        }
        Ok(())
    }
    fn test_eof_exits_without_repeating_default_action(menu: &Menu) -> Result<()> {
        let result = menu.launch("", &[])?;
        anyhow::ensure!(
            result.code == 0 && count(&result.stdout, b"Agensi MCP Main menu:") == 1,
            "EOF did not exit once"
        );
        anyhow::ensure!(
            !menu.root().join("config.local.json").exists(),
            "EOF wrote configuration"
        );
        anyhow::ensure!(
            count(&result.stderr, b"[INFO] [menu] Input closed") == 1,
            "EOF status missing"
        );
        Ok(())
    }
    fn test_invalid_selection_then_eof_is_bounded(menu: &Menu) -> Result<()> {
        let result = menu.launch("not-a-choice\n", &[])?;
        anyhow::ensure!(
            result.code == 0 && count(&result.stdout, b"Agensi MCP Main menu:") == 2,
            "Invalid selection/EOF did not exit"
        );
        anyhow::ensure!(
            !menu.root().join("config.local.json").exists(),
            "Invalid selection wrote configuration"
        );
        Ok(())
    }
    fn test_blank_line_still_selects_default_import(menu: &Menu) -> Result<()> {
        fs::write(
            menu.root().join("Proxy list.txt"),
            b"http://127.0.0.1:32123:::fixture\n",
        )?;
        anyhow::ensure!(
            menu.launch("\n\nexit\n", &[])?.code == 0,
            "Blank import failed"
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(menu.root().join("config.local.json"))?)?;
        anyhow::ensure!(
            value["mode"] == "disconnected"
                && value["profiles"][0]["name"] == "fixture"
                && value["profiles"].as_array().map(Vec::len) == Some(1),
            "Blank input changed default import"
        );
        Ok(())
    }
    fn test_status_switch_keeps_machine_readable_output(menu: &Menu) -> Result<()> {
        let result = menu.launch("", &["-Status"])?;
        anyhow::ensure!(result.code == 0, "Status failed");
        let value: serde_json::Value = serde_json::from_slice(&result.stdout)?;
        anyhow::ensure!(
            value["mode"] == "disconnected" && value["profiles"] == serde_json::json!([]),
            "Status JSON changed"
        );
        Ok(())
    }
    pub fn run() -> i32 {
        let log =
            agensi_mcp::logging::Logger::new(Path::new(env!("CARGO_MANIFEST_DIR")), "menu-tests");
        type Case = (&'static str, fn(&Menu) -> Result<()>);
        let tests: [Case; 6] = [
            (
                "test_advertised_quit_alias_exits",
                test_advertised_quit_alias_exits,
            ),
            (
                "test_existing_exit_aliases_remain_supported",
                test_existing_exit_aliases_remain_supported,
            ),
            (
                "test_eof_exits_without_repeating_default_action",
                test_eof_exits_without_repeating_default_action,
            ),
            (
                "test_invalid_selection_then_eof_is_bounded",
                test_invalid_selection_then_eof_is_bounded,
            ),
            (
                "test_blank_line_still_selects_default_import",
                test_blank_line_still_selects_default_import,
            ),
            (
                "test_status_switch_keeps_machine_readable_output",
                test_status_switch_keeps_machine_readable_output,
            ),
        ];
        let mut failed = 0;
        for (name, test) in tests {
            log.event("INFO", "menu-tests", name);
            match Menu::new().and_then(|menu| test(&menu)) {
                Ok(()) => println!("PASS {name}"),
                Err(_) => {
                    failed += 1;
                    eprintln!("FAIL {name}; fixture/error payload omitted");
                }
            }
        }
        println!("menu contracts: passed={} failed={failed}", 6 - failed);
        log.event(
            if failed == 0 { "INFO" } else { "ERROR" },
            "menu-tests",
            "Menu contracts completed.",
        );
        i32::from(failed != 0)
    }
}
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|v| v == "--bootstrap-input") {
        std::process::exit(process::bootstrap_input(args.into_iter().skip(1).collect()));
    }
    #[cfg(windows)]
    let code = windows::run();
    #[cfg(not(windows))]
    let code = {
        eprintln!("Native Windows and a freshly built executable are required; no pass claimed.");
        2
    };
    std::process::exit(code);
}
