use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;

const HTMX: &[u8] = include_bytes!("static/htmx.min.js");
const HTMX_SSE: &[u8] = include_bytes!("static/htmx-sse.js");
const STYLE: &[u8] = include_bytes!("static/style.css");

pub async fn htmx() -> impl IntoResponse {
    ([(CONTENT_TYPE, "application/javascript")], Body::from(HTMX))
}

pub async fn htmx_sse() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript")],
        Body::from(HTMX_SSE),
    )
}

pub async fn style() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/css; charset=utf-8")],
        Body::from(STYLE),
    )
}
