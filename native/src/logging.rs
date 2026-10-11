use chrono::Utc;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
};

pub struct Logger {
    file: Mutex<Option<File>>,
}
impl Logger {
    pub fn new(root: &Path, component: &str) -> Self {
        let file = (|| -> anyhow::Result<File> {
            anyhow::ensure!(
                !component.is_empty()
                    && component
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
                "Invalid log component."
            );
            let directory = root.join("logs");
            crate::config::reject_linked_ancestors(&directory)?;
            fs::create_dir_all(&directory)?;
            crate::config::reject_linked_ancestors(&directory)?;
            let stamp = Utc::now().format("%Y-%m-%d_%H-%M-%S_UTC");
            let name = format!(
                "{component}_{stamp}_{}_{}.log",
                std::process::id(),
                uuid::Uuid::new_v4()
            );
            let path = directory.join(name);
            crate::storage::check_file(&path)?;
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            Ok(options.open(path)?)
        })();
        let logger = Self {
            file: Mutex::new(file.ok()),
        };
        if logger.file.lock().map(|f| f.is_none()).unwrap_or(true) {
            logger.event(
                "WARNING",
                "logging",
                "File logging unavailable; safe records use stderr.",
            );
        }
        logger
    }
    pub fn event(&self, level: &str, component: &str, message: &str) {
        let level = match level {
            "INFO" | "WARNING" | "ERROR" | "DEBUG" => level,
            _ => "WARNING",
        };
        // Callers supply safe messages only. Flatten control characters to prevent forged entries.
        let plain = |value: &str| {
            value
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>()
        };
        let entry = format!(
            "[{} UTC] [{level}] [{}] {}\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S"),
            plain(component),
            plain(message)
        );
        if let Ok(mut guard) = self.file.lock()
            && let Some(file) = guard.as_mut()
        {
            if file
                .write_all(entry.as_bytes())
                .and_then(|_| file.flush())
                .is_ok()
            {
                return;
            }
            *guard = None;
        }
        eprint!("{entry}");
    }
}
