mod policy;
pub mod process;
pub mod runner;

use std::ffi::OsString;

fn main() {
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--bootstrap-input") {
        std::process::exit(process::bootstrap_input(args.into_iter().skip(1).collect()));
    }
    if args.first().is_some_and(|arg| arg == "--bootstrap") {
        std::process::exit(process::bootstrap(args.into_iter().skip(1).collect()));
    }
    if args.first().is_some_and(|arg| arg == "release") {
        args.remove(0);
        std::process::exit(policy::main(args));
    }
    let root = match agensi_mcp::project_root().or_else(|_| {
        std::env::current_exe()?
            .parent()
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("Cannot resolve check executable directory."))
    }) {
        Ok(root) => root,
        Err(_) => {
            eprintln!("Cannot resolve check root; payload omitted.");
            std::process::exit(1);
        }
    };
    std::process::exit(runner::run(&root, args));
}
