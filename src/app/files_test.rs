use super::*;
use tempfile::TempDir;
use tokio::fs;

async fn make_app() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("app1");
    fs::create_dir_all(&dir).await.unwrap();
    (tmp, dir)
}

#[tokio::test]
async fn write_then_read_round_trips() {
    let (_g, dir) = make_app().await;
    write_file(&dir, "config/foo.txt", "hello").await.unwrap();
    let body = read_file(&dir, "config/foo.txt").await.unwrap();
    assert_eq!(body, "hello");
}

#[tokio::test]
async fn safe_join_rejects_dotdot() {
    let (_g, dir) = make_app().await;
    let err = read_file(&dir, "../etc/passwd").await.unwrap_err();
    assert!(format!("{err}").contains("invalid"));
}

#[tokio::test]
async fn safe_join_rejects_absolute() {
    let (_g, dir) = make_app().await;
    let err = read_file(&dir, "/etc/passwd").await.unwrap_err();
    assert!(format!("{err}").contains("invalid"));
}

#[tokio::test]
async fn create_dir_then_list_tree() {
    let (_g, dir) = make_app().await;
    create_file(&dir, "src", FileKind::Directory).await.unwrap();
    create_file(&dir, "src/main.rs", FileKind::Text)
        .await
        .unwrap();
    let entries = list_tree(&dir).await.unwrap();
    let paths: Vec<_> = entries.iter().map(|e| e.relative_path.clone()).collect();
    assert!(paths.contains(&"src".to_string()));
    assert!(paths.contains(&"src/main.rs".to_string()));
}

#[tokio::test]
async fn delete_removes_file_and_dir() {
    let (_g, dir) = make_app().await;
    write_file(&dir, "a.txt", "x").await.unwrap();
    delete_file(&dir, "a.txt").await.unwrap();
    assert!(read_file(&dir, "a.txt").await.is_err());

    create_file(&dir, "d", FileKind::Directory).await.unwrap();
    write_file(&dir, "d/x.txt", "y").await.unwrap();
    delete_file(&dir, "d").await.unwrap();
    assert!(read_file(&dir, "d/x.txt").await.is_err());
}
