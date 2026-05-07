use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

#[allow(dead_code)]
pub struct WebError {
    status: StatusCode,
    message: String,
    is_hx_request: bool,
}

#[allow(dead_code)]
impl WebError {
    pub fn from_anyhow(err: anyhow::Error, headers: &HeaderMap) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("{err:#}"),
            is_hx_request: headers.contains_key("HX-Request"),
        }
    }

    pub fn bad_request(message: impl Into<String>, headers: &HeaderMap) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            is_hx_request: headers.contains_key("HX-Request"),
        }
    }

    pub fn not_found(message: impl Into<String>, headers: &HeaderMap) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            is_hx_request: headers.contains_key("HX-Request"),
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        if self.is_hx_request {
            (self.status, self.message).into_response()
        } else {
            let body = format!(
                "<!doctype html><html><body><h1>Error {}</h1><pre>{}</pre></body></html>",
                self.status.as_u16(),
                html_escape(&self.message),
            );
            (
                self.status,
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                body,
            )
                .into_response()
        }
    }
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[allow(dead_code)]
pub type WebResult<T> = Result<T, WebError>;
