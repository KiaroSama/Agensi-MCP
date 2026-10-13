//! Management CLI assertions use the retained, independent migration JSON oracle.
use crate::{
    project::{CLI_BOUND, Context},
    report::Report,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub async fn cases(context: &Context, report: &mut Report) -> Result<()> {
    let oracle: Value = serde_json::from_slice(&std::fs::read(
        context.source.join("tests/fixtures/migration.json"),
    )?)?;
    for group in ["proxies", "configs", "lists"] {
        for case in oracle[group]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing oracle group"))?
        {
            let project = context.project()?;
            let result: Result<()> = async {
                let name = case["name"].as_str().ok_or_else(|| anyhow::anyhow!("Missing oracle identity"))?;
                let mut error = case["error"].as_str();
                let mut expected = if group == "proxies" { case["expected"]["status"].clone() } else { case["expected"].clone() };
                let run = match group {
                    "proxies" => project.run(&["add", "fixture", "--stdin"], Some(case["input"].as_str().ok_or_else(|| anyhow::anyhow!("Invalid proxy fixture"))?), CLI_BOUND).await?,
                    "configs" => { project.config(case["input"].clone())?; project.cli(&["status"]).await? }
                    _ => {
                        let path = project.root.join("synthetic-proxies.txt");
                        std::fs::write(&path, case["input"].as_str().ok_or_else(|| anyhow::anyhow!("Invalid list fixture"))?)?;
                        project.run(&[std::ffi::OsStr::new("import"), path.as_os_str()], None, CLI_BOUND).await?
                    }
                };
                // The CLI trims input while the lower-level historical oracle does not.
                if group == "proxies" && ["proxy-20", "proxy-21"].contains(&name) {
                    error = None;
                    expected = json!({"mode":"disconnected","selected":null,"profiles":[{"name":"fixture","protocol":"http","host":"localhost","port":80,"authenticated":false}]});
                }
                let mut passed = if let Some(error) = error {
                    !run.timed_out && run.exit.is_some_and(|exit| exit != 0) && run.stderr.contains(error)
                } else { run.success() };
                if passed && error.is_none() {
                    if group == "lists" {
                        let mut names = BTreeSet::new();
                        let mut unique = Vec::new();
                        for entry in expected.as_array().ok_or_else(|| anyhow::anyhow!("Invalid expected list"))? {
                            let name = entry["name"].as_str().ok_or_else(|| anyhow::anyhow!("Missing expected name"))?;
                            if names.insert(name.to_lowercase()) { unique.push(json!({"name":name,"url":entry["url"]})); }
                        }
                        passed = project.read_json("config.local.json")?["profiles"] == json!(unique);
                    } else { passed = run.json.as_ref() == Some(&expected); }
                }
                report.record(format!("oracle-{name}"), passed, json!({"exit":run.exit,"timeout":run.timed_out}), None)
            }.await;
            project.close().await?;
            result?;
        }
    }
    for (index, filename) in ["SKILL.md", "skill.md", "Skill.md"].into_iter().enumerate() {
        let project = context.project()?;
        let result: Result<()> = async {
            project.write_json(
                "synthetic-skill.json",
                &json!([{"path":filename,"content":"# Synthetic fixture"}]),
            )?;
            let slug = format!("case-{index}");
            let input = project.root.join("synthetic-skill.json");
            let run = project
                .run(
                    &[
                        std::ffi::OsStr::new("save-skill"),
                        std::ffi::OsStr::new(&slug),
                        input.as_os_str(),
                    ],
                    None,
                    CLI_BOUND,
                )
                .await?;
            let target = project.root.join("skills").join(&slug);
            let mut files = Vec::new();
            if target.exists() {
                for entry in std::fs::read_dir(&target)? {
                    files.push(entry?.file_name().to_string_lossy().into_owned());
                }
                files.sort();
            }
            report.record(
                format!("skill-entry-{filename}"),
                run.success() && target.join("SKILL.md").is_file(),
                json!({"exit":run.exit,"files":files}),
                Some("F04"),
            )
        }
        .await;
        project.close().await?;
        result?;
    }
    let project = context.project()?;
    let result: Result<()> = async {
        let future = json!({"keep":["unicode-é",17]});
        project.config(
            json!({"schema":1,"mode":"disconnected","selected":null,"profiles":[],"future":future}),
        )?;
        let run = project.cli(&["direct"]).await?;
        report.record(
            "unknown-config-preserved",
            run.success() && project.read_json("config.local.json")?["future"] == future,
            json!({"exit":run.exit}),
            None,
        )?;
        let before = std::fs::read(project.root.join("config.local.json"))?;
        let run = project.cli(&["select", "missing"]).await?;
        report.record(
            "failed-config-update-preserves-bytes",
            !run.timed_out
                && run.exit.is_some_and(|exit| exit != 0)
                && before == std::fs::read(project.root.join("config.local.json"))?,
            json!({"exit":run.exit}),
            None,
        )?;
        let disconnected = project.cli(&["disconnect"]).await?;
        let refused = project.cli(&["network-check"]).await?;
        report.record(
            "disconnected-network-check-refuses",
            disconnected.success()
                && !refused.timed_out
                && refused.exit.is_some_and(|exit| exit != 0),
            json!({}),
            None,
        )
    }
    .await;
    project.close().await?;
    result?;
    for path in [
        "../outside",
        "/absolute",
        "C:\\escape",
        "NUL.txt",
        "nested/../escape",
    ] {
        let project = context.project()?;
        let result: Result<()> = async {
            project.write_json("synthetic-skill.json", &json!([{"path":"SKILL.md","content":"# Fixture"},{"path":path,"content":"fixture"}]))?;
            let input = project.root.join("synthetic-skill.json");
            let run = project.run(&[std::ffi::OsStr::new("save-skill"), std::ffi::OsStr::new("bad"), input.as_os_str()], None, CLI_BOUND).await?;
            report.record(format!("skill-refuses-{path}"), !run.timed_out && run.exit.is_some_and(|exit| exit != 0) && !project.root.join("skills/bad").exists(), json!({"exit":run.exit}), None)
        }.await;
        project.close().await?;
        result?;
    }
    ensure!(
        report
            .rows
            .iter()
            .any(|row| row["case"] == "oracle-proxy-0"),
        "Management source oracle not exercised"
    );
    Ok(())
}
