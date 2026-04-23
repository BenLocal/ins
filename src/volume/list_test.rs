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

#[tokio::test]
async fn set_filesystem_updates_existing_record() -> anyhow::Result<()> {
    let path = unique_test_path("set-fs");
    add_filesystem(&path, "data", "node1", "/mnt/old").await?;
    set_filesystem(&path, "data", "node1", "/mnt/new").await?;
    let loaded = load_volumes(&path).await?;
    assert_eq!(loaded.len(), 1);
    match &loaded[0] {
        VolumeRecord::Filesystem(v) => assert_eq!(v.path, "/mnt/new"),
        _ => panic!("expected filesystem"),
    }
    tokio::fs::remove_file(&path).await.ok();
    Ok(())
}

#[tokio::test]
async fn set_changes_type_when_switching_filesystem_to_cifs() -> anyhow::Result<()> {
    let path = unique_test_path("set-switch");
    add_filesystem(&path, "data", "node1", "/mnt/a").await?;
    set_cifs(
        &path,
        "data",
        "node1",
        "//10.0.0.5/share",
        "alice",
        "secret",
    )
    .await?;
    let loaded = load_volumes(&path).await?;
    assert!(matches!(&loaded[0], VolumeRecord::Cifs(_)));
    tokio::fs::remove_file(&path).await.ok();
    Ok(())
}

#[tokio::test]
async fn set_errors_when_volume_missing() -> anyhow::Result<()> {
    let path = unique_test_path("set-miss");
    let err = set_filesystem(&path, "data", "node1", "/mnt/new")
        .await
        .expect_err("missing record should fail");
    assert!(err.to_string().contains("not found"));
    tokio::fs::remove_file(&path).await.ok();
    Ok(())
}

#[tokio::test]
async fn delete_removes_single_record_by_name_and_node() -> anyhow::Result<()> {
    let path = unique_test_path("delete");
    add_filesystem(&path, "data", "node1", "/mnt/a").await?;
    add_filesystem(&path, "data", "node2", "/mnt/b").await?;
    delete_volume(&path, "data", "node1").await?;
    let loaded = load_volumes(&path).await?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].node(), "node2");
    tokio::fs::remove_file(&path).await.ok();
    Ok(())
}

#[tokio::test]
async fn delete_errors_when_volume_missing() -> anyhow::Result<()> {
    let path = unique_test_path("delete-miss");
    let err = delete_volume(&path, "data", "node1")
        .await
        .expect_err("missing record should fail");
    assert!(err.to_string().contains("not found"));
    tokio::fs::remove_file(&path).await.ok();
    Ok(())
}
