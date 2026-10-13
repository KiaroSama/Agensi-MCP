//! Source-pinned named inventory, sanitized observations and atomic UTF-8 reports.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub const SOURCE_COMMIT: &str = "309052d64a5c205b906c6ee719e7816b32c9a365";
pub struct Report {
    pub rows: Vec<Value>,
    started: String,
    log: File,
    debug: bool,
}
impl Report {
    pub fn new(work: &Path, debug: bool) -> Result<Self> {
        let logs = work.join("logs");
        std::fs::create_dir_all(&logs)?;
        let now = chrono::Utc::now();
        let path = logs.join(format!(
            "audit-blackbox_{}_UTC_{}.log",
            now.format("%Y-%m-%d_%H-%M-%S"),
            uuid::Uuid::new_v4().simple()
        ));
        let log = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Self {
            rows: Vec::new(),
            started: now.to_rfc3339(),
            log,
            debug,
        })
    }
    pub fn event(&mut self, level: &str, message: &str) -> Result<()> {
        let line = format!(
            "[{} UTC] [{level}] [AUDIT] {message}\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        self.log.write_all(line.as_bytes())?;
        self.log.flush()?;
        if self.debug || level != "DEBUG" {
            eprint!("{line}");
        }
        Ok(())
    }
    pub fn record(
        &mut self,
        name: impl Into<String>,
        passed: bool,
        observed: Value,
        finding: Option<&str>,
    ) -> Result<()> {
        let name = name.into();
        let status = if passed { "PASS" } else { "FAIL" };
        let mut row = json!({"case":name,"status":status,"observed":observed});
        if let Some(finding) = finding {
            row["finding"] = json!(finding);
        }
        self.event(
            if passed { "INFO" } else { "WARNING" },
            &format!("{status} {name}"),
        )?;
        self.rows.push(row);
        Ok(())
    }
    pub fn inventory(&mut self, source: &Path, phase: &str) -> Result<()> {
        let inventory: Value = serde_json::from_slice(&std::fs::read(
            source.join("tests/fixtures/audit-inventory.json"),
        )?)?;
        ensure!(
            inventory["source_commit"] == SOURCE_COMMIT,
            "Wrong source inventory commit"
        );
        let original = inventory["cases"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid case inventory"))?;
        ensure!(
            original.len() == 98,
            "Source inventory must contain 98 original cases"
        );
        let expected: Vec<String> = original
            .iter()
            .map(|row| {
                ensure!(
                    row["status"] == "PASS",
                    "Source inventory contains a non-PASS oracle"
                );
                Ok(row["case"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing source case identity"))?
                    .to_owned())
            })
            .collect::<Result<_>>()?;
        let selected: Vec<_> = expected
            .iter()
            .filter(|name| phase == "all" || case_phase(name) == phase)
            .cloned()
            .collect();
        let actual: Vec<_> = self
            .rows
            .iter()
            .filter_map(|row| row["case"].as_str().map(str::to_owned))
            .collect();
        let expected_set: BTreeSet<_> = selected.iter().cloned().collect();
        let actual_set: BTreeSet<_> = actual.iter().cloned().collect();
        // Identities and order are independent source evidence; a matching count cannot pass.
        if selected != actual
            || actual_set.len() != actual.len()
            || expected_set.len() != selected.len()
        {
            self.record("complete-inventory", false, json!({"expected":selected.len(),"observed":actual.len(),"missing":expected_set.difference(&actual_set).collect::<Vec<_>>(),"unexpected":actual_set.difference(&expected_set).collect::<Vec<_>>(),"order_matches":selected == actual}), None)?;
        }
        Ok(())
    }
    pub fn finish(&mut self, source: &Path, phase: &str, output: &Path) -> Result<bool> {
        let passed = self
            .rows
            .iter()
            .filter(|row| row["status"] == "PASS")
            .count();
        let failed = self.rows.len() - passed;
        let name = if cfg!(windows) {
            "agensi-mcp.exe"
        } else {
            "agensi-mcp"
        };
        let mut file = File::open(source.join("bin").join(name))?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 16384];
        loop {
            let size = file.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            hash.update(&buffer[..size]);
        }
        let binary_sha256: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let report = json!({"started_utc":self.started,"binary_sha256":binary_sha256,"phase":phase,"platform":if cfg!(target_os="linux") {"Linux"} else {std::env::consts::OS},"synthetic_loopback_only":true,"cases":self.rows,"passed":passed,"failed":failed});
        let parent = output
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Output needs a parent directory"))?;
        std::fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut staged, &report)?;
        staged.write_all(b"\n")?;
        staged.as_file().sync_all()?;
        staged.persist(output)?;
        self.event("INFO", &format!("shutdown passed={passed} failed={failed}"))?;
        Ok(failed == 0)
    }
}
fn case_phase(name: &str) -> &'static str {
    if name.starts_with("oauth-") {
        "oauth"
    } else if name.starts_with("local-http-")
        || name.starts_with("http-")
        || name.starts_with("probe-")
    {
        "http"
    } else if name.starts_with("fifo-")
        || name.starts_with("linked-")
        || name.starts_with("stderr-logging-")
        || name.starts_with("status-does-")
    {
        "filesystem"
    } else {
        "management"
    }
}
