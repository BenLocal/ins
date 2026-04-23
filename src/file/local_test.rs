use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

fn test_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("ins_local_file_test_{unique}"))
        .join(name)
}

#[tokio::test]
async fn local_file_write_and_read_round_trip() {
    let file = LocalFile;
    let path = test_path("nested/output.txt");
    let content = "hello from local file";

    file.write(&path, content, None).await.unwrap();
    let loaded = file.read(&path, None).await.unwrap();

    assert_eq!(loaded, content);
    fs::remove_file(&path).await.unwrap();
    fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
}

#[tokio::test]
async fn local_file_reports_progress_for_write_and_read() {
    let file = LocalFile;
    let path = test_path("progress.txt");
    let content = "progress payload";

    let write_events = Arc::new(Mutex::new(Vec::new()));
    let write_progress: ProgressFn = {
        let write_events = write_events.clone();
        Arc::new(move |current, total| {
            write_events.lock().unwrap().push((current, total));
        })
    };

    file.write(&path, content, Some(&write_progress))
        .await
        .unwrap();

    let write_events = write_events.lock().unwrap().clone();
    assert_eq!(
        write_events.first().copied(),
        Some((0, content.len() as u64))
    );
    assert_eq!(
        write_events.last().copied(),
        Some((content.len() as u64, content.len() as u64))
    );

    let read_events = Arc::new(Mutex::new(Vec::new()));
    let read_progress: ProgressFn = {
        let read_events = read_events.clone();
        Arc::new(move |current, total| {
            read_events.lock().unwrap().push((current, total));
        })
    };

    let loaded = file.read(&path, Some(&read_progress)).await.unwrap();
    assert_eq!(loaded, content);

    let read_events = read_events.lock().unwrap().clone();
    assert_eq!(
        read_events.first().copied(),
        Some((0, content.len() as u64))
    );
    assert_eq!(
        read_events.last().copied(),
        Some((content.len() as u64, content.len() as u64))
    );

    fs::remove_file(&path).await.unwrap();
}
