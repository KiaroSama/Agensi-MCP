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
        let file = (|| -> std::io::Result<File> {
            fs::create_dir_all(root.join("logs"))?;
            let stamp = Utc::now().format("%Y-%m-%d_%H-%M-%S_UTC");
            let name = format!("{component}_{stamp}_{}.log", std::process::id());
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(root.join("logs").join(name))
        })();
        if file.is_err() {
            eprintln!("Logging unavailable; safe status will use stderr.");
        }
        Self {
            file: Mutex::new(file.ok()),
        }
    }
    pub fn event(&self, level: &str, component: &str, message: &str) {
        let entry = format!(
            "[{} UTC] [{level}] [{component}] {message}\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        if let Ok(mut guard) = self.file.lock()
            && let Some(file) = guard.as_mut()
            && file
                .write_all(entry.as_bytes())
                .and_then(|_| file.flush())
                .is_err()
        {
            eprintln!("File logging failed.");
        }
    }
}
