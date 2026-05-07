use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
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
