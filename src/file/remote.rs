use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use crossterm::terminal;
use russh::client::{self, Config, KeyboardInteractiveAuthResponse};
use russh::keys::{PrivateKeyWithHashAlg, PublicKey, load_secret_key};
use russh::{ChannelMsg, Pty};
use russh_sftp::client::SftpSession;
use tokio::io::AsyncWriteExt;

use super::{FileTrait, ProgressFn};

const CHUNK_SIZE: usize = 64 * 1024;

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

#[derive(Clone, Debug)]
pub struct RemoteCommandOutput {
    pub exit_status: u32,
    pub stdout: String,
    pub stderr: String,
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

    #[allow(dead_code)]
    pub fn with_key_path(mut self, path: String) -> Self {
        self.key_path = Some(path);
        self
    }

    pub async fn exec(&self, command: &str) -> anyhow::Result<RemoteCommandOutput> {
        self.exec_with_options(command, ExecOptions::default())
            .await
    }

    pub async fn tty_exec(&self, command: &str) -> anyhow::Result<RemoteCommandOutput> {
        self.exec_with_options(command, ExecOptions::tty()).await
    }

    async fn exec_with_options(
        &self,
        command: &str,
        options: ExecOptions,
    ) -> anyhow::Result<RemoteCommandOutput> {
        let handle = self.connect_handle().await?;
        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| anyhow!("open session channel: {}", e))?;

        if options.request_tty {
            let (cols, rows) = terminal_size();
            channel
                .request_pty(
                    false,
                    &terminal_type(),
                    cols,
                    rows,
                    0,
                    0,
                    &[] as &[(Pty, u32)],
                )
                .await
                .map_err(|e| anyhow!("ssh request pty '{}': {}", command, e))?;
        }

        channel
            .exec(true, command)
            .await
            .map_err(|e| anyhow!("ssh exec '{}': {}", command, e))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = None;

        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    if options.stream_output {
                        write_and_flush_stdout(&data)?;
                    }
                    stdout.extend_from_slice(&data)
                }
                ChannelMsg::ExtendedData { data, ext } if ext == 1 => {
                    if options.stream_output {
                        write_and_flush_stderr(&data)?;
                    }
                    stderr.extend_from_slice(&data)
                }
                ChannelMsg::ExitStatus {
                    exit_status: status,
                } => exit_status = Some(status),
                _ => {}
            }
        }

        Ok(RemoteCommandOutput {
            exit_status: exit_status.unwrap_or(0),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    /// Connect, authenticate, open SFTP subsystem and run the given closure with SftpSession.
    async fn with_sftp<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(
                SftpSession,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send>>
            + Send,
        T: Send,
    {
        let handle = self.connect_handle().await?;

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

    async fn connect_handle(&self) -> anyhow::Result<client::Handle<SftpClientHandler>> {
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

        let auth_ok = authenticate(&mut handle, self).await?;

        if !auth_ok {
            return Err(anyhow!(
                "ssh authentication failed for user {} at {}:{}",
                self.user,
                self.host,
                self.port
            ));
        }

        Ok(handle)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ExecOptions {
    stream_output: bool,
    request_tty: bool,
}

impl ExecOptions {
    fn tty() -> Self {
        Self {
            stream_output: true,
            request_tty: true,
        }
    }
}

fn terminal_type() -> String {
    std::env::var("TERM")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "xterm-256color".to_string())
}

fn terminal_size() -> (u32, u32) {
    if let Ok((cols, rows)) = terminal::size() {
        if cols > 0 && rows > 0 {
            return (u32::from(cols), u32::from(rows));
        }
    }

    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(120);
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(40);
    (cols, rows)
}

fn write_and_flush_stdout(data: &[u8]) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(data)
        .map_err(|e| anyhow!("write remote stdout to local stdout: {}", e))?;
    stdout
        .flush()
        .map_err(|e| anyhow!("flush local stdout: {}", e))
}

fn write_and_flush_stderr(data: &[u8]) -> anyhow::Result<()> {
    let mut stderr = std::io::stderr().lock();
    stderr
        .write_all(data)
        .map_err(|e| anyhow!("write remote stderr to local stderr: {}", e))?;
    stderr
        .flush()
        .map_err(|e| anyhow!("flush local stderr: {}", e))
}

async fn authenticate<H>(
    handle: &mut client::Handle<H>,
    remote: &RemoteFile,
) -> anyhow::Result<bool>
where
    H: client::Handler,
{
    let key_path = remote
        .key_path
        .as_ref()
        .map(|p| p.as_str())
        .unwrap_or("~/.ssh/id_rsa");

    if !key_path.is_empty() && Path::new(key_path).exists() {
        let key_pair =
            load_secret_key(key_path, None).map_err(|e| anyhow!("load key {}: {}", key_path, e))?;
        let hash_alg = handle
            .best_supported_rsa_hash()
            .await
            .ok()
            .flatten()
            .flatten();
        let auth = handle
            .authenticate_publickey(
                remote.user.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg),
            )
            .await
            .map_err(|e| anyhow!("ssh pubkey auth: {}", e))?;
        if auth.success() {
            return Ok(true);
        }
    }

    if !remote.password.is_empty() {
        let auth = handle
            .authenticate_password(remote.user.clone(), remote.password.clone())
            .await
            .map_err(|e| anyhow!("ssh password auth: {}", e))?;
        if auth.success() {
            return Ok(true);
        }

        if authenticate_keyboard_interactive(handle, remote).await? {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn authenticate_keyboard_interactive<H>(
    handle: &mut client::Handle<H>,
    remote: &RemoteFile,
) -> anyhow::Result<bool>
where
    H: client::Handler,
{
    let mut response = handle
        .authenticate_keyboard_interactive_start(remote.user.clone(), None::<String>)
        .await
        .map_err(|e| anyhow!("ssh keyboard-interactive auth start: {}", e))?;

    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let responses = prompts
                    .into_iter()
                    .map(|prompt| {
                        if prompt.prompt.is_empty() {
                            String::new()
                        } else {
                            remote.password.clone()
                        }
                    })
                    .collect();
                response = handle
                    .authenticate_keyboard_interactive_respond(responses)
                    .await
                    .map_err(|e| anyhow!("ssh keyboard-interactive auth respond: {}", e))?;
            }
        }
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
    async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 remote path {}", path.display()))?
            .to_string();

        self.with_sftp(|sftp| {
            Box::pin(async move {
                let dir_path = Path::new(&path_str);
                if let Some(parent) = dir_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        create_parent_dirs(&sftp, parent).await?;
                    }
                }
                ensure_remote_dir(&sftp, &path_str).await?;
                Ok(())
            })
        })
        .await
    }

    async fn read_bytes(
        &self,
        path: &Path,
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<Vec<u8>> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 remote path {}", path.display()))?
            .to_string();

        if let Some(ref cb) = progress {
            cb(0, 0);
        }

        let content = self
            .with_sftp(|sftp| {
                Box::pin(async move {
                    sftp.read(&path_str)
                        .await
                        .map_err(|e| anyhow!("sftp read {}: {}", path_str, e))
                })
            })
            .await?;

        if let Some(ref cb) = progress {
            cb(content.len() as u64, content.len() as u64);
        }
        Ok(content)
    }

    async fn write_bytes(
        &self,
        path: &Path,
        content: &[u8],
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 remote path {}", path.display()))?
            .to_string();
        let data = content.to_vec();
        let data_len = data.len() as u64;

        let progress_cb = progress.cloned();

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
                let mut file = sftp
                    .create(&path_str)
                    .await
                    .map_err(|e| anyhow!("sftp create {}: {}", path_str, e))?;
                let mut written = 0usize;
                while written < data.len() {
                    let end = (written + CHUNK_SIZE).min(data.len());
                    file.write_all(&data[written..end])
                        .await
                        .map_err(|e| anyhow!("sftp write {}: {}", path_str, e))?;
                    written = end;
                    if let Some(ref cb) = progress_cb {
                        cb(written as u64, data_len);
                    }
                }
                file.shutdown()
                    .await
                    .map_err(|e| anyhow!("sftp close {}: {}", path_str, e))
            })
        })
        .await?;

        if let Some(ref cb) = progress {
            cb(data_len, data_len);
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
            ensure_remote_dir(sftp, s.as_ref()).await?;
        }
    }
    Ok(())
}

async fn ensure_remote_dir(sftp: &SftpSession, path: &str) -> anyhow::Result<()> {
    if sftp
        .try_exists(path)
        .await
        .map_err(|e| anyhow!("sftp stat {}: {}", path, e))?
    {
        return Ok(());
    }

    match sftp.create_dir(path).await {
        Ok(()) => Ok(()),
        Err(create_err) => {
            if sftp
                .try_exists(path)
                .await
                .map_err(|e| anyhow!("sftp stat {} after mkdir failure: {}", path, e))?
            {
                Ok(())
            } else {
                Err(anyhow!("sftp mkdir {}: {}", path, create_err))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn remote_file_with_key_path_sets_field() {
        let remote = RemoteFile::new("host".into(), 22, "user".into(), "secret".into())
            .with_key_path("~/.ssh/id_rsa".into());

        assert_eq!(remote.host, "host");
        assert_eq!(remote.port, 22);
        assert_eq!(remote.user, "user");
        assert_eq!(remote.password, "secret");
        assert_eq!(remote.key_path.as_deref(), Some("~/.ssh/id_rsa"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_file_rejects_non_utf8_paths_before_network_access() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let remote = RemoteFile::new("127.0.0.1".into(), 22, "user".into(), "secret".into());
        let invalid = PathBuf::from(OsStr::from_bytes(b"/tmp/\xFFinvalid"));

        let read_err = remote.read(&invalid, None).await.unwrap_err().to_string();
        assert!(read_err.contains("non-utf8 remote path"));

        let write_err = remote
            .write(&invalid, "data", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(write_err.contains("non-utf8 remote path"));
    }
}
