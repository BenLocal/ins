use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use russh::client::{self, Config};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKey};
use russh_sftp::client::SftpSession;

use super::{progress_for_read, progress_for_write, FileTrait, ProgressFn};

#[derive(Clone, Debug)]
pub struct RemoteFile {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Optional path to private key (e.g. "~/.ssh/id_rsa").
    /// If set, password can be empty or used as passphrase for the key.
    pub key_path: Option<String>,
}

impl RemoteFile {
    pub fn new(host: String, port: u16, user: String, password: String) -> Self {
        Self {
            host,
            port,
            user,
            password,
            key_path: None,
        }
    }

    pub fn with_key_path(mut self, path: String) -> Self {
        self.key_path = Some(path);
        self
    }

    /// Connect, authenticate, open SFTP subsystem and run the given closure with SftpSession.
    async fn with_sftp<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(SftpSession) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send>> + Send,
        T: Send,
    {
        let addrs = (self.host.as_str(), self.port);
        let config = Config {
            inactivity_timeout: Some(Duration::from_secs(30)),
            ..Default::default()
        };
        let config = Arc::new(config);
        let handler = SftpClientHandler;

        let mut handle = client::connect(config, addrs, handler)
            .await
            .map_err(|e| anyhow!("ssh connect to {}:{}: {}", self.host, self.port, e))?;

        let auth_ok = if let Some(ref key_path) = self.key_path {
            let passphrase = if self.password.is_empty() {
                None
            } else {
                Some(self.password.as_str())
            };
            let key_pair = load_secret_key(key_path, passphrase)
                .map_err(|e| anyhow!("load key {}: {}", key_path, e))?;
            let hash_alg = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
            let auth = handle
                .authenticate_publickey(
                    self.user.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg),
                )
                .await
                .map_err(|e| anyhow!("ssh pubkey auth: {}", e))?;
            auth.success()
        } else {
            let auth = handle
                .authenticate_password(self.user.clone(), self.password.clone())
                .await
                .map_err(|e| anyhow!("ssh password auth: {}", e))?;
            auth.success()
        };

        if !auth_ok {
            return Err(anyhow!("ssh authentication failed for user {}", self.user));
        }

        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| anyhow!("open session channel: {}", e))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| anyhow!("request sftp subsystem: {}", e))?;

        let stream = channel.into_stream();
        let sftp = SftpSession::new(stream)
            .await
            .map_err(|e| anyhow!("sftp session: {}", e))?;

        f(sftp).await
    }
}

struct SftpClientHandler;

impl russh::client::Handler for SftpClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[async_trait]
impl FileTrait for RemoteFile {
    async fn read(&self, path: &Path, progress: Option<&ProgressFn>) -> anyhow::Result<String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 remote path {}", path.display()))?
            .to_string();

        let (bar, own_prog) = if progress.is_none() && std::io::stdout().is_terminal() {
            let (b, p) = progress_for_read(path);
            (Some(b), Some(p))
        } else {
            (None, None)
        };
        let progress = progress.or(own_prog.as_ref());

        if let Some(ref cb) = progress {
            cb(0, 0);
        }

        let content = self
            .with_sftp(|sftp| {
                Box::pin(async move {
                    let bytes = sftp
                        .read(&path_str)
                        .await
                        .map_err(|e| anyhow!("sftp read {}: {}", path_str, e))?;
                    String::from_utf8(bytes).map_err(|e| anyhow!("remote file not utf-8: {}", e))
                })
            })
            .await?;

        if let Some(ref cb) = progress {
            cb(content.len() as u64, content.len() as u64);
        }
        if let Some(ref b) = bar {
            b.finish_with_message("Done");
        }

        Ok(content)
    }

    async fn write(
        &self,
        path: &Path,
        content: &str,
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 remote path {}", path.display()))?
            .to_string();
        let data = content.as_bytes().to_vec();
        let data_len = data.len() as u64;

        let (bar, own_prog) = if progress.is_none() && std::io::stdout().is_terminal() {
            let (b, p) = progress_for_write(path, data_len);
            (Some(b), Some(p))
        } else {
            (None, None)
        };
        let progress = progress.or(own_prog.as_ref());

        if let Some(ref cb) = progress {
            cb(0, data_len);
        }

        self.with_sftp(|sftp| {
            Box::pin(async move {
                if let Some(parent) = Path::new(&path_str).parent() {
                    if !parent.as_os_str().is_empty() {
                        create_parent_dirs(&sftp, parent).await?;
                    }
                }
                sftp.write(&path_str, &data)
                    .await
                    .map_err(|e| anyhow!("sftp write {}: {}", path_str, e))
            })
        })
        .await?;

        if let Some(ref cb) = progress {
            cb(data_len, data_len);
        }
        if let Some(ref b) = bar {
            b.finish_with_message("Done");
        }

        Ok(())
    }
}

/// Create remote directory and all parent components (mkdir -p style).
async fn create_parent_dirs(sftp: &SftpSession, path: &Path) -> anyhow::Result<()> {
    let mut prefix = std::path::PathBuf::new();
    for comp in path.components() {
        prefix.push(comp);
        let s = prefix.to_string_lossy();
        if !s.is_empty() {
            sftp.create_dir(s.as_ref()).await.ok(); // ignore if exists
        }
    }
    Ok(())
}
