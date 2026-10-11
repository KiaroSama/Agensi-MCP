use agensi_mcp::cleanup;
use fs2::FileExt;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// Isolated project-local filesystem fixtures; no network, sleeps or environment mutation.
// All lock acquisitions are try-only. The integrator supplies outer wall/idle limits.
fn root() -> TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    fs::create_dir_all(&base).unwrap();
    let base = base.canonicalize().unwrap();
    let directory = tempfile::Builder::new()
        .prefix("cleanup space-")
        .tempdir_in(base)
        .unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = 'fixture'\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("run.ps1"),
        "# Fixture project marker\n",
    )
    .unwrap();
    directory
}

fn write(root: &Path, name: &str, content: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path
}

fn candidate<'a>(result: &'a Value, name: &str) -> &'a Value {
    result["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .expect("candidate reported")
}

#[test]
fn dry_run_is_read_only_and_apply_deletes_only_exact_legacy_allowlist() {
    let directory = root();
    let root = directory.path();
    let module = write(
        root,
        "node_modules/fixture/package.json",
        b"{\"name\":\"fixture\"}",
    );
    let old_binary = write(
        root,
        ".ai/ARCHIVE/pre-ci-binary/agensi-mcp.exe",
        b"fixture old binary",
    );
    let mut preserved = Vec::new();
    for name in [
        ".cache/cargo/registry/fixture",
        "target/debug/fixture",
        "dist/fixture.zip",
        ".tmp/unmarked/fixture",
        "plugins/preserved-reference/fixture",
        ".state/session.json",
        "auth/fixture",
        "config.local.json",
        "logs/run.log",
        "skills/fixture/SKILL.md",
        "bin/agensi-mcp.exe",
        ".git/fixture",
        ".ai/RESEARCH/fixture.md",
        ".ai/ARCHIVE/other-backup/fixture",
        "user-file.txt",
    ] {
        preserved.push(write(root, name, b"preserved fixture"));
    }
    let expected_bytes =
        fs::metadata(&module).unwrap().len() + fs::metadata(&old_binary).unwrap().len();
    let preview = cleanup::run(root, false).unwrap();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["bytes"], expected_bytes);
    assert_eq!(preview["deleted"], 0);
    assert_eq!(preview["deleted_bytes"], 0);
    assert!(module.exists() && old_binary.exists());
    assert!(
        !root.join(".runtime.lock").exists(),
        "dry-run must not create a lock"
    );
    assert_eq!(candidate(&preview, "target")["bytes"], Value::Null);
    assert_eq!(candidate(&preview, ".cache")["status"], "skipped");
    let serialized = preview.to_string();
    assert!(!serialized.contains(&root.display().to_string()));
    assert!(!serialized.contains("session.json"));
    assert!(!serialized.contains("package.json"));

    let applied = cleanup::run(root, true).unwrap();
    assert_eq!(applied["deleted"], 2);
    assert_eq!(applied["deleted_bytes"], expected_bytes);
    assert_eq!(applied["skipped"], 5);
    assert!(!root.join("node_modules").exists());
    assert!(!root.join(".ai/ARCHIVE/pre-ci-binary").exists());
    for path in preserved {
        assert_eq!(fs::read(path).unwrap(), b"preserved fixture");
    }
    assert_eq!(candidate(&applied, "plugins")["status"], "skipped");
}

#[test]
fn protected_descendant_refuses_entire_candidate_without_a_lock_write() {
    let directory = root();
    let root = directory.path();
    let ordinary = write(root, "node_modules/fixture/ordinary.txt", b"keep");
    let protected = write(
        root,
        "node_modules/fixture/.env.local",
        b"synthetic fixture only",
    );
    for apply in [false, true] {
        let result = cleanup::run(root, apply).unwrap();
        assert_eq!(candidate(&result, "node_modules")["status"], "skipped");
        assert_eq!(candidate(&result, "node_modules")["bytes"], Value::Null);
        assert_eq!(result["deleted"], 0);
        assert_eq!(result["bytes"], 0);
        assert!(ordinary.exists() && protected.exists());
        assert!(!root.join(".runtime.lock").exists());
    }
}

#[test]
fn shared_runtime_guards_prevent_cleanup_and_drop_releases_them() {
    let directory = root();
    let root = directory.path();
    let module = write(root, "node_modules/fixture/file", b"keep until idle");
    let first = cleanup::runtime_guard(root).unwrap();
    let second = cleanup::runtime_guard(root).unwrap();
    for apply in [false, true] {
        assert!(
            cleanup::run(root, apply)
                .unwrap_err()
                .to_string()
                .contains("Runtime is active")
        );
        assert!(module.exists());
    }
    drop(first);
    assert!(cleanup::run(root, true).is_err());
    drop(second);
    assert_eq!(cleanup::run(root, true).unwrap()["deleted"], 1);
    assert!(!module.exists());
}

#[test]
fn exclusive_cleanup_lease_prevents_runtime_and_another_cleanup() {
    let directory = root();
    let root = directory.path();
    let path = root.join(".runtime.lock");
    let lease = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    FileExt::try_lock_exclusive(&lease).unwrap();
    assert!(cleanup::runtime_guard(root).is_err());
    assert!(cleanup::run(root, false).is_err());
    assert!(cleanup::run(root, true).is_err());
    drop(lease);
    assert!(cleanup::runtime_guard(root).is_ok());
}

#[test]
fn unsafe_root_and_lock_are_rejected_without_creating_or_deleting_files() {
    let directory = root();
    let root = directory.path();
    let module = write(root, "node_modules/fixture/file", b"preserved");
    // PathBuf::push normalizes '..' for Windows verbatim roots; build the raw input instead.
    let mut raw = root.as_os_str().to_os_string();
    raw.push(format!("{0}child{0}..{0}", std::path::MAIN_SEPARATOR));
    let traversal = PathBuf::from(raw);
    assert!(
        traversal
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    );
    for path in [PathBuf::from("."), traversal] {
        assert!(cleanup::run(&path, true).is_err());
        assert!(cleanup::runtime_guard(&path).is_err());
    }
    fs::create_dir(root.join(".runtime.lock")).unwrap();
    assert!(cleanup::run(root, true).is_err());
    assert!(module.exists());
    fs::remove_dir(root.join(".runtime.lock")).unwrap();
    fs::remove_file(root.join("run.ps1")).unwrap();
    assert!(cleanup::run(root, true).is_err());
    assert!(!root.join(".runtime.lock").exists());
    assert!(module.exists());
}

#[cfg(unix)]
fn link_directory(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn link_directory(target: &Path, link: &Path) {
    use std::os::windows::process::CommandExt;
    // Junction creation is unprivileged; test the actual Windows reparse-point branch.
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(output.status.success(), "junction fixture creation failed");
}

#[cfg(any(unix, windows))]
#[test]
fn linked_candidate_descendant_ancestor_and_runtime_lock_never_follow_targets() {
    let directory = root();
    let outside = root();
    let root = directory.path();
    let sentinel = write(outside.path(), "sentinel", b"outside fixture must survive");
    link_directory(outside.path(), &root.join("node_modules"));
    let result = cleanup::run(root, true).unwrap();
    assert_eq!(candidate(&result, "node_modules")["status"], "skipped");
    assert_eq!(result["bytes"], 0);
    assert!(!root.join(".runtime.lock").exists());
    remove_link(&root.join("node_modules"));

    let ordinary = write(
        root,
        "node_modules/fixture/ordinary",
        b"keep entire candidate",
    );
    link_directory(outside.path(), &root.join("node_modules/fixture/link"));
    let result = cleanup::run(root, true).unwrap();
    assert_eq!(candidate(&result, "node_modules")["status"], "skipped");
    assert!(ordinary.exists());
    assert!(!root.join(".runtime.lock").exists());
    remove_link(&root.join("node_modules/fixture/link"));
    fs::remove_dir_all(root.join("node_modules")).unwrap();

    link_directory(outside.path(), &root.join(".ai"));
    let result = cleanup::run(root, true).unwrap();
    assert_eq!(
        candidate(&result, ".ai/ARCHIVE/pre-ci-binary")["status"],
        "skipped"
    );
    assert!(!root.join(".runtime.lock").exists());
    remove_link(&root.join(".ai"));

    link_directory(outside.path(), &root.join(".runtime.lock"));
    assert!(cleanup::run(root, true).is_err());
    assert!(cleanup::runtime_guard(root).is_err());
    remove_link(&root.join(".runtime.lock"));
    assert_eq!(fs::read(sentinel).unwrap(), b"outside fixture must survive");
}

#[cfg(unix)]
fn remove_link(path: &Path) {
    fs::remove_file(path).unwrap();
}

#[cfg(windows)]
fn remove_link(path: &Path) {
    fs::remove_dir(path).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_locked_file_reports_partial_deletion_truthfully() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = root();
    let root = directory.path();
    let locked = write(root, "node_modules/fixture/locked.bin", b"locked fixture");
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&locked)
        .unwrap();
    let removable = write(
        root,
        ".ai/ARCHIVE/pre-ci-binary/old.exe",
        b"old binary fixture",
    );
    let result = cleanup::run(root, true).unwrap();
    let entry = candidate(&result, "node_modules");
    assert_eq!(entry["status"], "skipped");
    assert_eq!(entry["deleted_bytes"], 0);
    assert_eq!(result["deleted"], 1);
    assert_eq!(result["status"], "partial");
    assert!(!removable.exists());
    assert!(entry["reason"].as_str().unwrap().contains("OS code"));
    assert!(locked.exists());
    drop(handle);
    assert_eq!(cleanup::run(root, true).unwrap()["deleted"], 1);
}
