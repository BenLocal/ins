//! Shared local/remote script executor for node-info probes.

use anyhow::{Context, anyhow, bail};
use tokio::process::Command;

use crate::file::remote::RemoteFile;
use crate::node::types::{NodeRecord, RemoteNodeRecord};

/// Run a shell script on the node and return stdout.
/// Bails on non-zero exit (probes are fail-loud; the caller can swallow the
/// error for optional probes).
pub(crate) async fn run_probe_script(node: &NodeRecord, script: &str) -> anyhow::Result<String> {
    match node {
        NodeRecord::Local() => run_local(script).await,
        NodeRecord::Remote(remote) => run_remote(remote, script).await,
    }
}

async fn run_local(script: &str) -> anyhow::Result<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .await
        .context("run local probe script")?;
    if !output.status.success() {
        bail!(
            "local probe script exited with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn run_remote(remote: &RemoteNodeRecord, script: &str) -> anyhow::Result<String> {
    let rf = build_remote_file(remote);
    let result = rf
        .exec(script)
        .await
        .with_context(|| format!("run remote probe on node '{}'", remote.name))?;
    if result.exit_status != 0 {
        return Err(anyhow!(
            "remote probe on node '{}' exited with status {}: {}",
            remote.name,
            result.exit_status,
            result.stderr.trim()
        ));
    }
    Ok(result.stdout)
}

fn build_remote_file(remote: &RemoteNodeRecord) -> RemoteFile {
    let rf = RemoteFile::new(
        remote.ip.clone(),
        remote.port,
        remote.user.clone(),
        remote.password.clone(),
    );
    if let Some(key_path) = &remote.key_path {
        rf.with_key_path(key_path.clone())
    } else {
        rf
    }
}
