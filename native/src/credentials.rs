use async_trait::async_trait;
use fs2::FileExt;
use rmcp::transport::auth::{
    AuthError, CredentialRefreshGuard, CredentialStore, StoredCredentials,
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct FileCredentials {
    pub path: PathBuf,
}
fn store_error() -> AuthError {
    AuthError::CredentialStoreError("Local credential storage failed; values omitted.".into())
}
impl FileCredentials {
    pub fn new(root: &std::path::Path) -> Self {
        Self {
            path: root.join(".state/auth/rust-session.json"),
        }
    }
    pub async fn import_legacy(&self, root: &std::path::Path) -> anyhow::Result<bool> {
        crate::storage::check_file(&self.path)?;
        if self.path.exists() {
            return Ok(false);
        }
        let hash = format!("{:x}", md5::compute(crate::auth::ENDPOINT));
        let old = root.join(".state/auth/mcp-remote-v1");
        let info_path = old.join(format!("{hash}_client_info.json"));
        let tokens_path = old.join(format!("{hash}_tokens.json"));
        crate::storage::check_file(&info_path)?;
        crate::storage::check_file(&tokens_path)?;
        if !info_path.exists() || !tokens_path.exists() {
            return Ok(false);
        }
        let read = |path: PathBuf| -> anyhow::Result<serde_json::Value> {
            let bytes = crate::storage::read_optional(&path)?
                .ok_or_else(|| anyhow::anyhow!("Cannot read legacy session."))?;
            serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("Invalid legacy session; values omitted."))
        };
        let info = read(info_path)?;
        let tokens = read(tokens_path)?;
        // Only public-client tokens with explicit issuer/scope/expiry can cross the storage boundary.
        if info.get("client_secret").is_some() {
            return Ok(false);
        }
        let Some(client) = info.get("client_id").and_then(|v| v.as_str()) else {
            return Ok(false);
        };
        let Some(issuer) = tokens
            .get("issuer")
            .or_else(|| info.get("issuer"))
            .and_then(|v| v.as_str())
        else {
            return Ok(false);
        };
        let Ok(issuer_url) = url::Url::parse(issuer) else {
            return Ok(false);
        };
        if issuer_url.scheme() != "https"
            || !issuer_url.username().is_empty()
            || issuer_url.password().is_some()
        {
            return Ok(false);
        }
        let Some(scope) = tokens.get("requested_scope").and_then(|v| v.as_str()) else {
            return Ok(false);
        };
        let (Some(expiry), Some(duration)) = (
            tokens.get("expires_at").and_then(|v| v.as_u64()),
            tokens.get("expires_in").and_then(|v| v.as_u64()),
        ) else {
            return Ok(false);
        };
        let Some(received) = (expiry / 1000).checked_sub(duration) else {
            return Ok(false);
        };
        let token = serde_json::from_value(tokens.clone())
            .map_err(|_| anyhow::anyhow!("Legacy token shape incompatible; renew sign-in."))?;
        let value = StoredCredentials::new(
            client.to_string(),
            Some(token),
            scope.split_whitespace().map(str::to_string).collect(),
            Some(received),
        )
        .with_issuer(Some(issuer.to_string()));
        self.save(value).await.map_err(|_| {
            anyhow::anyhow!("Cannot persist compatible session; old session preserved.")
        })?;
        Ok(true)
    }
    pub async fn guard(&self) -> Result<CredentialRefreshGuard, AuthError> {
        let path = self.path.with_extension("lock");
        tokio::task::spawn_blocking(move || {
            let parent = path.parent().ok_or_else(store_error)?;
            crate::storage::check_file(&path).map_err(|_| store_error())?;
            fs::create_dir_all(parent).map_err(|_| store_error())?;
            crate::storage::check_file(&path).map_err(|_| store_error())?;
            let mut options = OpenOptions::new();
            options.create(true).truncate(false).read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options.open(path).map_err(|_| store_error())?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if file.try_lock_exclusive().is_ok() {
                    return Ok(CredentialRefreshGuard::new(Lock(file)));
                }
                if Instant::now() >= deadline {
                    return Err(AuthError::CredentialStoreError(
                        "Another session is updating credentials; retry later.".into(),
                    ));
                }
                // Bounded lock coordination; no spin or unbounded file-lock wait.
                std::thread::sleep(Duration::from_millis(25));
            }
        })
        .await
        .map_err(|_| store_error())?
    }
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
#[async_trait]
impl CredentialStore for FileCredentials {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let Some(bytes) = crate::storage::read_optional(&self.path).map_err(|_| store_error())? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| store_error())
    }
    async fn save(&self, value: StoredCredentials) -> Result<(), AuthError> {
        let parent = self.path.parent().ok_or_else(store_error)?;
        crate::storage::check_file(&self.path).map_err(|_| store_error())?;
        fs::create_dir_all(parent).map_err(|_| store_error())?;
        crate::storage::check_file(&self.path).map_err(|_| store_error())?;
        let bytes = serde_json::to_vec_pretty(&value).map_err(|_| store_error())?;
        if bytes.len() > 1024 * 1024 {
            return Err(store_error());
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| store_error())?;
        temp.write_all(&bytes)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|_| store_error())?;
        crate::storage::check_file(&self.path).map_err(|_| store_error())?;
        temp.persist(&self.path).map_err(|_| store_error())?;
        Ok(())
    }
    async fn clear(&self) -> Result<(), AuthError> {
        // Preserve issuer-bound credentials for human recovery rather than silently deleting them.
        Err(AuthError::CredentialStoreError(
            "Saved issuer changed; renew sign-in explicitly. Existing session preserved.".into(),
        ))
    }
    async fn acquire_refresh_guard(&self) -> Result<Option<CredentialRefreshGuard>, AuthError> {
        self.guard().await.map(Some)
    }
}
