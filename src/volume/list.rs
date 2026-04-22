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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::types::{CifsVolume, FilesystemVolume};
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        env::temp_dir().join(format!(
            "ins-volume-{name}-{}-{nanos}.json",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn load_returns_empty_when_file_missing() {
        let path = unique_test_path("missing");
        let loaded = load_volumes(&path).await.expect("load");
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn save_then_load_roundtrips_mixed_types() -> anyhow::Result<()> {
        let path = unique_test_path("roundtrip");
        let volumes = vec![
            VolumeRecord::Filesystem(FilesystemVolume {
                name: "data".into(),
                node: "node1".into(),
                path: "/mnt/data".into(),
            }),
            VolumeRecord::Cifs(CifsVolume {
                name: "data".into(),
                node: "node2".into(),
                server: "//10.0.0.5/share".into(),
                username: "alice".into(),
                password: "secret".into(),
            }),
        ];

        save_volumes(&path, &volumes).await?;
        let loaded = load_volumes(&path).await?;

        assert_eq!(loaded.len(), 2);
        match &loaded[0] {
            VolumeRecord::Filesystem(v) => {
                assert_eq!(v.name, "data");
                assert_eq!(v.node, "node1");
                assert_eq!(v.path, "/mnt/data");
            }
            _ => panic!("expected filesystem"),
        }
        match &loaded[1] {
            VolumeRecord::Cifs(v) => {
                assert_eq!(v.server, "//10.0.0.5/share");
                assert_eq!(v.username, "alice");
                assert_eq!(v.password, "secret");
            }
            _ => panic!("expected cifs"),
        }

        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }

    #[tokio::test]
    async fn add_filesystem_persists_record() -> anyhow::Result<()> {
        let path = unique_test_path("add-fs");
        add_filesystem(&path, "data", "node1", "/mnt/data").await?;
        let loaded = load_volumes(&path).await?;
        assert_eq!(loaded.len(), 1);
        match &loaded[0] {
            VolumeRecord::Filesystem(v) => {
                assert_eq!(v.name, "data");
                assert_eq!(v.node, "node1");
                assert_eq!(v.path, "/mnt/data");
            }
            _ => panic!("expected filesystem"),
        }
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }

    #[tokio::test]
    async fn add_rejects_duplicate_name_node_pair() -> anyhow::Result<()> {
        let path = unique_test_path("dup");
        add_filesystem(&path, "data", "node1", "/mnt/a").await?;
        let err = add_cifs(
            &path,
            "data",
            "node1",
            "//10.0.0.5/share",
            "alice",
            "secret",
        )
        .await
        .expect_err("duplicate should fail");
        assert!(err.to_string().contains("already exists"));
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }
}
