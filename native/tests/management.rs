use agensi_mcp::config::{self, Mode, Profile};
use agensi_mcp::management;
use agensi_mcp::proxy_import;
use agensi_mcp::skill_files::{self, InlineFile};
use fs2::FileExt;
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// Source mapping: routing.test.mjs route state/redaction and inline files ->
// generated_management_oracle, config_atomic_preservation, skill_installation_boundaries.
// proxy-import.test.mjs named list/additive import -> generated_management_oracle,
// additive_import_is_transactional. Launcher/network/OAuth source cases belong to parent.
// Pure cases + tiny isolated filesystem inputs, no sleeps/processes/network/shared CWD.
// Integrator's runner supplies outer wall/idle bounds; lock tests never block.
fn root() -> TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    fs::create_dir_all(&base).expect("project scratch root");
    tempfile::Builder::new()
        .prefix("management space-")
        .tempdir_in(base)
        .expect("isolated root")
}

fn run(root: &Path, args: &[&str], stdin: &str) -> anyhow::Result<Value> {
    management::command(
        root,
        &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
        stdin,
    )
}

fn assert_oracle(case: &Value, actual: anyhow::Result<Value>) {
    let id = case["name"].as_str().expect("fixture case name");
    if let Some(error) = case["error"].as_str() {
        assert_eq!(actual.expect_err(id).to_string(), error, "{id}");
    } else {
        assert_eq!(actual.expect(id), case["expected"], "{id}");
    }
}

#[test]
fn generated_management_oracle() {
    let fixtures: Value = serde_json::from_str(include_str!("../../tests/fixtures/migration.json"))
        .expect("source-generated UTF-8 fixture");
    let dir = root();
    for group in ["proxies", "configs", "lists"] {
        let cases = fixtures[group].as_array().expect("fixture case group");
        assert!(
            !cases.is_empty(),
            "{group} fixtures must not silently disappear"
        );
        for case in cases {
            let actual = match group {
                "proxies" => {
                    let input = case["input"].as_str().expect("proxy input");
                    config::validate_proxy(input).map(|url| {
                        let mut config = config::initial();
                        config
                            .profiles
                            .push(Profile::new("fixture".into(), input.into()));
                        json!({ "href": url.as_str(), "status": config::public_status(&config) })
                    })
                }
                "configs" => {
                    fs::write(
                        dir.path().join("config.local.json"),
                        serde_json::to_vec(&case["input"]).expect("JSON fixture"),
                    )
                    .expect("isolated config input");
                    config::load(dir.path()).map(|config| config::public_status(&config))
                }
                "lists" => proxy_import::parse_proxy_list(
                    case["input"].as_str().expect("list input"),
                )
                .map(|entries| serde_json::to_value(entries).expect("list result serialization")),
                _ => unreachable!(),
            };
            assert_oracle(case, actual);
        }
    }
}

#[test]
fn config_atomic_preservation() {
    let dir = root();
    let path = dir.path().join("config.local.json");
    assert_eq!(
        config::load(dir.path()).expect("missing config"),
        config::initial()
    );
    let mut config = config::initial();
    config.profiles.push(Profile::new(
        "named".into(),
        "http://fixture-user:fixture-pass@localhost:80".into(),
    ));
    config.mode = Mode::Proxy;
    config.selected = Some("named".into());
    config
        .extra
        .insert("future".into(), json!({ "keep": "café نمونه" }));
    config.profiles[0]
        .extra
        .insert("future".into(), json!([1, 2]));
    config::save(dir.path(), &config).expect("initial atomic save");
    assert_eq!(config::load(dir.path()).expect("UTF-8 reload"), config);
    let before = fs::read(&path).expect("before bytes");
    config.selected = Some("missing".into());
    assert!(config::save(dir.path(), &config).is_err());
    assert_eq!(fs::read(&path).expect("after rejected save"), before);
    config.selected = Some("named".into());
    config.mode = Mode::Direct;
    config::save(dir.path(), &config).expect("replace existing file on Windows and Linux");
    assert_eq!(
        config::load(dir.path()).expect("replacement config"),
        config
    );
    assert!(
        !config::public_status(&config)
            .to_string()
            .contains("fixture-pass")
    );
    assert!(!format!("{config:?}").contains("fixture-pass"));
    assert!(
        !fs::read_dir(dir.path())
            .expect("root entries")
            .any(|entry| entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".config-"))
    );
    fs::write(&path, b"{\"credential\":\"FIXTURE_PRIVATE_VALUE\"").expect("malformed fixture");
    let corrupt = fs::read(&path).expect("corrupt bytes");
    let error =
        config::save(dir.path(), &config).expect_err("corrupt settings must not be replaced");
    assert!(!error.to_string().contains("FIXTURE_PRIVATE_VALUE"));
    assert_eq!(fs::read(&path).expect("preserved corruption"), corrupt);
}

#[test]
fn management_commands_preserve_route_and_unknown_data() {
    let dir = root();
    assert_eq!(
        run(dir.path(), &["status"], "").expect("status"),
        json!({ "mode": "disconnected", "selected": null, "profiles": [] })
    );
    run(
        dir.path(),
        &["add", "named", "--stdin"],
        " http://fixture-user:fixture-pass@localhost:80\n",
    )
    .expect("stdin add");
    assert!(run(dir.path(), &["add", "NAMED", "http://localhost:81"], "").is_err());
    assert!(run(dir.path(), &["select", "NAMED"], "").is_err());
    run(dir.path(), &["select", "named"], "").expect("select");
    let mut config = config::load(dir.path()).expect("config");
    config
        .extra
        .insert("future".into(), json!({ "preserved": true }));
    config::save(dir.path(), &config).expect("forward fields");
    for action in ["direct", "disconnect"] {
        let status = run(dir.path(), &[action], "").expect("mode command");
        assert_eq!(status["selected"], "named");
        assert_eq!(
            status["mode"],
            if action == "direct" {
                "direct"
            } else {
                "disconnected"
            }
        );
        assert_eq!(
            config::load(dir.path()).expect("round trip").extra["future"],
            json!({ "preserved": true })
        );
    }
    let before = fs::read(dir.path().join("config.local.json")).expect("before");
    for args in [
        vec!["unknown"],
        vec!["add", "invalid name!", "http://localhost:80"],
        vec!["add", "valid", "not a proxy"],
    ] {
        assert!(run(dir.path(), &args, "").is_err());
    }
    assert_eq!(
        fs::read(dir.path().join("config.local.json")).expect("after"),
        before
    );
}

#[test]
fn additive_import_is_transactional() {
    let dir = root();
    run(dir.path(), &["add", "existing", "http://localhost:80"], "").expect("existing profile");
    run(dir.path(), &["select", "existing"], "").expect("existing route");
    let list = dir.path().join("Proxy list.txt");
    let row = "socks5://127.0.0.1:1080:fixture-user:fixture-pass:UK 5";
    fs::write(
        &list,
        format!("\u{feff}{row}\r\n{row}\nhttp://localhost:080:::EXISTING\n"),
    )
    .expect("UTF-8 BOM input");
    assert_eq!(
        proxy_import::import(dir.path(), &list).expect("add and skip"),
        json!({ "added": 1, "skipped": 2 })
    );
    let config = config::load(dir.path()).expect("unchanged route");
    assert_eq!(config.mode, Mode::Proxy);
    assert_eq!(config.selected.as_deref(), Some("existing"));
    assert_eq!(
        proxy_import::import(dir.path(), &list).expect("all skipped"),
        json!({ "added": 0, "skipped": 3 })
    );
    let path = dir.path().join("config.local.json");
    let before = fs::read(&path).expect("before");
    for text in [
        format!("{row}\nhttp://localhost:81:::EXISTING"),
        format!("{row}\ninvalid FIXTURE_PRIVATE_VALUE"),
    ] {
        fs::write(&list, text).expect("conflicting input");
        let error = proxy_import::import(dir.path(), &list).expect_err("nothing imported");
        assert!(error.to_string().starts_with("Line 2:"));
        assert!(!error.to_string().contains("FIXTURE_PRIVATE_VALUE"));
        assert_eq!(fs::read(&path).expect("preserved config"), before);
    }
    // Deliberately invalid bytes exercise the file format's fatal UTF-8 rejection.
    fs::write(&list, [0xff, 0xfe, 0xfd]).expect("invalid bytes");
    assert!(
        proxy_import::import(dir.path(), &list)
            .expect_err("UTF-8 refused")
            .to_string()
            .contains("UTF-8")
    );
    fs::write(&list, vec![b' '; 1024 * 1024 + 1]).expect("oversized file");
    assert!(proxy_import::import(dir.path(), &list).is_err());
    assert_eq!(fs::read(&path).expect("still preserved"), before);
    fs::write(&list, "http://localhost:80:::duplicate\n".repeat(10001)).expect("entry bound input");
    assert!(
        proxy_import::import(dir.path(), &list)
            .expect_err("entry bound")
            .to_string()
            .contains("10000")
    );
    assert_eq!(fs::read(&path).expect("entry bound no changes"), before);
}

#[test]
fn profile_limit_refuses_import_without_partial_save() {
    let dir = root();
    let mut config = config::initial();
    config.profiles = (0..100)
        .map(|i| Profile::new(format!("profile{i}"), "http://localhost:80".into()))
        .collect();
    config::save(dir.path(), &config).expect("100 profiles allowed");
    let before = fs::read(dir.path().join("config.local.json")).expect("before");
    let list = dir.path().join("Proxy list.txt");
    fs::write(&list, "http://localhost:81:::new").expect("101st profile input");
    assert!(proxy_import::import(dir.path(), &list).is_err());
    assert_eq!(
        fs::read(dir.path().join("config.local.json")).expect("after"),
        before
    );
}

#[test]
fn concurrent_management_refuses_stale_writes_and_preserves_contents() {
    let dir = root();
    run(dir.path(), &["add", "first", "http://localhost:80"], "").expect("initial profile");
    let before = fs::read(dir.path().join("config.local.json")).expect("before");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(".management.lock"))
        .expect("lock file");
    FileExt::try_lock_exclusive(&lock).expect("separate handle holds writer lock");
    let error = run(dir.path(), &["add", "second", "http://localhost:81"], "")
        .expect_err("bounded busy refusal");
    assert!(error.to_string().contains("busy"));
    assert_eq!(
        fs::read(dir.path().join("config.local.json")).expect("after busy refusal"),
        before
    );
    FileExt::unlock(&lock).expect("release lock");
    run(dir.path(), &["add", "second", "http://localhost:81"], "")
        .expect("retry adds without loss");
    assert_eq!(
        config::load(dir.path())
            .expect("both writers preserved")
            .profiles
            .len(),
        2
    );
}

fn files() -> Vec<InlineFile> {
    vec![
        InlineFile {
            path: "SKILL.md".into(),
            content: "# Skill\nUTF-8: café نمونه\n".into(),
        },
        InlineFile {
            path: "references/usage.md".into(),
            content: "Use locally.\n".into(),
        },
    ]
}

fn stages(root: &Path) -> Vec<PathBuf> {
    let skills = root.join("skills");
    if !skills.exists() {
        return Vec::new();
    }
    fs::read_dir(skills)
        .expect("skills entries")
        .map(|e| e.expect("entry").path())
        .filter(|p| {
            p.file_name()
                .expect("file name")
                .to_string_lossy()
                .starts_with(".install-")
        })
        .collect()
}

#[test]
fn skill_installation_boundaries() {
    let dir = root();
    let files = files();
    let saved = skill_files::save_skill(dir.path(), "example", &files).expect("UTF-8 installation");
    assert_eq!(
        fs::read(saved.join("SKILL.md")).expect("installed bytes"),
        files[0].content.as_bytes()
    );
    assert_eq!(
        fs::read(saved.join("references/usage.md")).expect("reference bytes"),
        files[1].content.as_bytes()
    );
    assert!(skill_files::save_skill(dir.path(), "example", &files).is_err());
    assert!(skill_files::save_skill(dir.path(), "EXAMPLE", &files).is_err());
    for unsafe_path in [
        "../outside.md",
        "C:/outside.md",
        "file:stream",
        "NUL.txt",
        "bad.",
        "bad ",
        "\\outside",
        "/outside",
        "a//b",
        "a/./b",
        "a/../b",
        "COM1.txt",
        "LPT9",
        "a\u{0}b",
        "bad?name",
    ] {
        let mut unsafe_files = files.clone();
        unsafe_files.push(InlineFile {
            path: unsafe_path.into(),
            content: "no".into(),
        });
        assert!(
            skill_files::save_skill(dir.path(), "unsafe", &unsafe_files).is_err(),
            "{unsafe_path}"
        );
        assert!(!dir.path().join("skills/unsafe").exists());
    }
    for invalid_name in ["", "../example", "NUL", "COM1", "name with spaces", "café"] {
        assert!(skill_files::save_skill(dir.path(), invalid_name, &files).is_err());
    }
    assert!(stages(dir.path()).is_empty());
}

#[test]
fn skill_package_limits_and_casing() {
    let dir = root();
    let mut duplicate = files();
    duplicate.push(InlineFile {
        path: "skill.MD".into(),
        content: "duplicate".into(),
    });
    assert!(skill_files::save_skill(dir.path(), "duplicate", &duplicate).is_err());
    let missing = vec![InlineFile {
        path: "nested/SKILL.md".into(),
        content: "nested only".into(),
    }];
    assert!(skill_files::save_skill(dir.path(), "missing", &missing).is_err());
    let mut conflict = files();
    conflict.push(InlineFile {
        path: "references".into(),
        content: "not a directory".into(),
    });
    assert!(skill_files::save_skill(dir.path(), "conflict", &conflict).is_err());
    let mut many = files();
    many.extend((0..499).map(|i| InlineFile {
        path: format!("file{i}.md"),
        content: String::new(),
    }));
    assert!(skill_files::save_skill(dir.path(), "many", &many).is_err());
    let oversized = [InlineFile {
        path: "SKILL.md".into(),
        content: "é".repeat(10 * 1024 * 1024 + 1),
    }];
    assert!(skill_files::save_skill(dir.path(), "oversized", &oversized).is_err());
    let mut casing = files();
    casing.push(InlineFile {
        path: "References\\second.md".into(),
        content: "same directory".into(),
    });
    let saved =
        skill_files::save_skill(dir.path(), "casing", &casing).expect("cross-platform casing");
    assert!(saved.join("references/second.md").exists());
    assert!(stages(dir.path()).is_empty());
    for name in ["duplicate", "missing", "conflict", "many", "oversized"] {
        assert!(!dir.path().join("skills").join(name).exists());
    }
}

#[test]
fn save_skill_command_accepts_array_and_files_object() {
    let dir = root();
    let json_file = dir.path().join("inline skill.json");
    for (name, data) in [
        ("array", serde_json::to_value(files()).expect("array")),
        ("object", json!({ "files": files() })),
    ] {
        fs::write(&json_file, serde_json::to_vec(&data).expect("JSON")).expect("fixture");
        let saved = run(
            dir.path(),
            &["save-skill", name, json_file.to_str().expect("UTF-8 path")],
            "",
        )
        .expect("command installation");
        assert_eq!(
            saved["saved"],
            dir.path()
                .join("skills")
                .join(name)
                .to_str()
                .expect("UTF-8 installed path")
        );
    }
    fs::write(&json_file, b"invalid FIXTURE_PRIVATE_VALUE").expect("invalid JSON");
    let error = run(
        dir.path(),
        &["save-skill", "invalid", json_file.to_str().expect("path")],
        "",
    )
    .expect_err("JSON refusal");
    assert!(!error.to_string().contains("FIXTURE_PRIVATE_VALUE"));
    assert!(!dir.path().join("skills/invalid").exists());
}

#[cfg(unix)]
#[test]
fn linked_ancestors_and_settings_are_refused() {
    use std::os::unix::fs::symlink;
    let dir = root();
    let outside = root();
    symlink(outside.path(), dir.path().join("skills")).expect("linked skills fixture");
    assert!(skill_files::save_skill(dir.path(), "linked", &files()).is_err());
    assert!(!outside.path().join("linked").exists());
    fs::write(
        outside.path().join("settings.json"),
        serde_json::to_vec(&config::initial()).expect("JSON"),
    )
    .expect("outside fixture");
    symlink(
        outside.path().join("settings.json"),
        dir.path().join("config.local.json"),
    )
    .expect("linked settings fixture");
    assert!(config::load(dir.path()).is_err());
    assert!(config::save(dir.path(), &config::initial()).is_err());
}

#[cfg(windows)]
#[test]
fn failed_windows_replacement_preserves_existing_contents() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = root();
    config::save(dir.path(), &config::initial()).expect("initial config");
    let path = dir.path().join("config.local.json");
    let before = fs::read(&path).expect("before");
    // Allow reading/writing, deny delete sharing: force MoveFileExW to fail without
    // changing ACLs, readonly attributes or requiring a privileged symlink fixture.
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&path)
        .expect("open without delete sharing");
    let mut direct = config::initial();
    direct.mode = Mode::Direct;
    assert!(config::save(dir.path(), &direct).is_err());
    assert_eq!(fs::read(&path).expect("preserved original bytes"), before);
    drop(handle);
    config::save(dir.path(), &direct).expect("replacement after handle closes");
    assert_eq!(
        config::load(dir.path()).expect("direct config").mode,
        Mode::Direct
    );
    assert!(stages(dir.path()).is_empty());
}
