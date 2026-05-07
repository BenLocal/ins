use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use tempfile::TempDir;
use tower::ServiceExt;

use super::build_router;
use crate::config::InsConfig;
use crate::web::jobs::JobRegistry;
use crate::web::state::AppState;
use crate::web::templates;

fn make_state(home: &TempDir, token: Option<&str>) -> AppState {
    AppState {
        home: Arc::new(home.path().to_path_buf()),
        config: Arc::new(InsConfig::default()),
        jobs: Arc::new(JobRegistry::default()),
        token: token.map(|t| Arc::new(t.to_string())),
        templates: templates::build(),
    }
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn root_renders_three_panes() {
    let home = TempDir::new().unwrap();
    let router = build_router(make_state(&home, None));
    let resp = router
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("nodes-pane"));
    assert!(body.contains("apps-pane"));
    assert!(body.contains("services-pane"));
}

#[tokio::test]
async fn token_required_when_set() {
    let home = TempDir::new().unwrap();
    let router = build_router(make_state(&home, Some("secret123")));

    let resp = router
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/?token=secret123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .any(|h| h.to_str().unwrap().contains("ins_token=secret123"))
    );
}

#[tokio::test]
async fn nodes_list_after_create() {
    let home = TempDir::new().unwrap();
    let router = build_router(make_state(&home, None));
    let body = "name=edge-1&ip=10.0.0.1&port=22&user=root&password=&key_path=";
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/nodes")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("edge-1"));

    // Verify it actually persisted.
    let nodes_path = home.path().join("nodes.json");
    assert!(nodes_path.is_file(), "nodes.json should exist");
    let json = tokio::fs::read_to_string(&nodes_path).await.unwrap();
    assert!(json.contains("edge-1"));
    assert!(json.contains("10.0.0.1"));
    assert!(json.contains("\"port\":22") || json.contains("\"port\": 22"));
}

#[tokio::test]
async fn apps_save_does_not_treat_path_ending_in_delete_as_delete() {
    use crate::app::files;
    let home = TempDir::new().unwrap();
    let app_dir = home.path().join("app/foo");
    tokio::fs::create_dir_all(&app_dir).await.unwrap();
    files::create_file(&app_dir, "scripts", files::FileKind::Directory)
        .await
        .unwrap();
    files::write_file(&app_dir, "scripts/delete", "shebang")
        .await
        .unwrap();

    let router = build_router(make_state(&home, None));
    let body = "content=updated";
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/apps/foo/files/scripts/delete")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The file at scripts/delete should still exist with new content.
    let content = files::read_file(&app_dir, "scripts/delete").await.unwrap();
    assert_eq!(content, "updated");
    // And the scripts directory should NOT have been removed.
    assert!(app_dir.join("scripts").is_dir());
}

#[tokio::test]
async fn apps_path_traversal_blocked() {
    let home = TempDir::new().unwrap();
    tokio::fs::create_dir_all(home.path().join("app/foo"))
        .await
        .unwrap();
    let router = build_router(make_state(&home, None));
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/apps/foo/files/..%2Fetc%2Fpasswd")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // safe_join returns Err → handler maps to 500; some axum versions decode URL
    // encoding before path matching, in which case the dotdot might be matched
    // as a literal path component. Either way, this should NOT return 200.
    assert!(
        resp.status() == StatusCode::INTERNAL_SERVER_ERROR
            || resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn sse_stream_emits_backlog_line_then_done() {
    use crate::execution_output::ExecutionOutput;
    use crate::pipeline::PipelineMode;
    use crate::store::duck::InstalledServiceRecord;
    use crate::web::jobs::{Job, JobState};

    let home = TempDir::new().unwrap();
    let state = make_state(&home, None);

    // Inject a synthetic Job into the registry without spawning a pipeline.
    let output = ExecutionOutput::streaming();
    output.line("hello");

    let job = Arc::new(Job {
        id: "20260507-000000-deadbeef".into(),
        mode: PipelineMode::Check,
        service: InstalledServiceRecord {
            service: "x".into(),
            namespace: String::new(),
            app_name: "y".into(),
            node_name: "z".into(),
            workspace: "/tmp".into(),
            created_at_ms: 0,
        },
        output: output.clone(),
        state: Arc::new(tokio::sync::RwLock::new(JobState::Running)),
        started_at: chrono::Utc::now(),
    });
    state.jobs.jobs_for_test_insert(job.clone()).await;

    output.line("world");
    output.line("[ins:done] ok");

    let router = build_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/jobs/20260507-000000-deadbeef/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // SSE keeps the connection open — consume with a timeout instead of to_bytes.
    let mut stream = resp.into_body().into_data_stream();
    let mut buf = Vec::new();
    let timeout = tokio::time::sleep(std::time::Duration::from_millis(200));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            _ = &mut timeout => break,
            chunk = stream.next() => match chunk {
                Some(Ok(c)) => buf.extend_from_slice(&c),
                _ => break,
            },
        }
    }
    let body = String::from_utf8_lossy(&buf);
    assert!(
        body.contains("event: backlog"),
        "missing backlog event in: {body}"
    );
    assert!(body.contains("hello"), "missing 'hello' in: {body}");
}
