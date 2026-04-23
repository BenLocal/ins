use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::fs;

use crate::volume::types::{CifsVolume, FilesystemVolume, VolumeRecord};

pub(crate) fn volumes_file(home: &Path) -> PathBuf {
    home.join("volumes.json")
}

pub(crate) async fn load_volumes(path: &Path) -> anyhow::Result<Vec<VolumeRecord>> {
    if !fs::try_exists(path)
        .await
        .with_context(|| format!("check volumes file {}", path.display()))?
    {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path)
        .await
        .with_context(|| format!("read volumes file {}", path.display()))?;

    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&content).with_context(|| format!("parse volumes file {}", path.display()))
}

pub(crate) async fn save_volumes(path: &Path, volumes: &[VolumeRecord]) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(volumes)?;
    fs::write(path, format!("{content}\n"))
        .await
        .with_context(|| format!("write volumes file {}", path.display()))?;
    Ok(())
}

pub(crate) async fn add_filesystem(
    path: &Path,
    name: &str,
    node: &str,
    host_path: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    ensure_unique(&volumes, name, node)?;
    volumes.push(VolumeRecord::Filesystem(FilesystemVolume {
        name: name.to_string(),
        node: node.to_string(),
        path: host_path.to_string(),
    }));
    save_volumes(path, &volumes).await
}

pub(crate) async fn add_cifs(
    path: &Path,
    name: &str,
    node: &str,
    server: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    ensure_unique(&volumes, name, node)?;
    volumes.push(VolumeRecord::Cifs(CifsVolume {
        name: name.to_string(),
        node: node.to_string(),
        server: server.to_string(),
        username: username.to_string(),
        password: password.to_string(),
    }));
    save_volumes(path, &volumes).await
}

fn ensure_unique(volumes: &[VolumeRecord], name: &str, node: &str) -> anyhow::Result<()> {
    if volumes.iter().any(|v| v.name() == name && v.node() == node) {
        anyhow::bail!("volume '{}' on node '{}' already exists", name, node);
    }
    Ok(())
}

pub(crate) async fn set_filesystem(
    path: &Path,
    name: &str,
    node: &str,
    host_path: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    let index = find_index(&volumes, name, node)?;
    volumes[index] = VolumeRecord::Filesystem(FilesystemVolume {
        name: name.to_string(),
        node: node.to_string(),
        path: host_path.to_string(),
    });
    save_volumes(path, &volumes).await
}

pub(crate) async fn set_cifs(
    path: &Path,
    name: &str,
    node: &str,
    server: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    let index = find_index(&volumes, name, node)?;
    volumes[index] = VolumeRecord::Cifs(CifsVolume {
        name: name.to_string(),
        node: node.to_string(),
        server: server.to_string(),
        username: username.to_string(),
        password: password.to_string(),
    });
    save_volumes(path, &volumes).await
}

pub(crate) async fn delete_volume(path: &Path, name: &str, node: &str) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    let index = find_index(&volumes, name, node)?;
    volumes.remove(index);
    save_volumes(path, &volumes).await
}

fn find_index(volumes: &[VolumeRecord], name: &str, node: &str) -> anyhow::Result<usize> {
    volumes
        .iter()
        .position(|v| v.name() == name && v.node() == node)
        .ok_or_else(|| anyhow::anyhow!("volume '{}' on node '{}' not found", name, node))
}

#[cfg(test)]
#[path = "list_test.rs"]
mod list_test;
