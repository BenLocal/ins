use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Html;

use crate::web::error::{WebError, WebResult};
use crate::web::state::AppState;

pub async fn render(State(s): State<AppState>, headers: HeaderMap) -> WebResult<Html<String>> {
    let tmpl = s
        .templates
        .get_template("index.html")
        .map_err(|e| WebError::from_anyhow(anyhow::Error::from(e), &headers))?;
    Ok(Html(tmpl.render(()).map_err(|e| {
        WebError::from_anyhow(anyhow::Error::from(e), &headers)
    })?))
}
