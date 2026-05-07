use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::Html;
use serde::Deserialize;

use crate::app::files::{self, FileKind, TreeEntry};
use crate::cli::app::list_app_records;
use crate::web::error::{WebError, WebResult};
use crate::web::state::AppState;

#[derive(Deserialize)]
pub struct CreateForm {
    pub path: String,
    pub kind: String, // "text" | "directory"
}

#[derive(Deserialize)]
pub struct SaveForm {
    // Optional — absent when the POST is a delete action (no body needed).
    pub content: Option<String>,
}

fn render(
    s: &AppState,
    name: &str,
    ctx: minijinja::Value,
    headers: &HeaderMap,
) -> WebResult<Html<String>> {
    let tmpl = s
        .templates
        .get_template(name)
        .map_err(|e| WebError::from_anyhow(anyhow::Error::from(e), headers))?;
    Ok(Html(tmpl.render(ctx).map_err(|e| {
        WebError::from_anyhow(anyhow::Error::from(e), headers)
    })?))
}

fn format_entries(entries: Vec<TreeEntry>) -> Vec<serde_json::Value> {
    entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "relative_path": e.relative_path,
                "kind": match e.kind {
                    FileKind::Directory => "Directory",
                    FileKind::Text => "Text",
                },
            })
        })
        .collect()
}

pub async fn list(State(s): State<AppState>, headers: HeaderMap) -> WebResult<Html<String>> {
    let apps = list_app_records(&s.app_home(), s.config.defaults_env())
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    let view: Vec<_> = apps
        .iter()
        .map(|a| serde_json::json!({ "name": a.name }))
        .collect();
    render(
        &s,
        "apps/list.html",
        minijinja::context! { apps => view },
        &headers,
    )
}

pub async fn files_view(
    State(s): State<AppState>,
    Path(app): Path<String>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    let app_dir = s.app_home().join(&app);
    let entries = files::list_tree(&app_dir)
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    render(
        &s,
        "apps/files.html",
        minijinja::context! { app => app, entries => format_entries(entries) },
        &headers,
    )
}

pub async fn editor(
    State(s): State<AppState>,
    Path((app, rel)): Path<(String, String)>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    let app_dir = s.app_home().join(&app);
    let content = files::read_file(&app_dir, &rel)
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    render(
        &s,
        "apps/editor.html",
        minijinja::context! { app => app, rel => rel, content => content },
        &headers,
    )
}

pub async fn create(
    State(s): State<AppState>,
    Path(app): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CreateForm>,
) -> WebResult<Html<String>> {
    let app_dir = s.app_home().join(&app);
    let kind = if form.kind == "directory" {
        FileKind::Directory
    } else {
        FileKind::Text
    };
    files::create_file(&app_dir, &form.path, kind)
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    files_view(State(s), Path(app), headers).await
}

/// Handles POST `/apps/:app/files/*rel`.
///
/// Two sub-actions are multiplexed over this single wildcard route because
/// axum 0.7 / matchit does not allow a more-specific route
/// `/apps/:app/files/*rel/delete` to coexist with `/apps/:app/files/*rel`.
///
/// The client sends:
/// - **delete**: POST to `…/<rel>/delete` with no `content` field → rel ends
///   with `/delete`; strip that suffix and remove the file.
/// - **save**: POST to `…/<rel>` with a `content` field → write the file.
pub async fn save_or_delete(
    State(s): State<AppState>,
    Path((app, rel)): Path<(String, String)>,
    headers: HeaderMap,
    Form(form): Form<SaveForm>,
) -> WebResult<Html<String>> {
    const DELETE_SUFFIX: &str = "/delete";
    if let Some(file_rel) = rel.strip_suffix(DELETE_SUFFIX) {
        // Delete action — rel captured the trailing `/delete` segment.
        let app_dir = s.app_home().join(&app);
        files::delete_file(&app_dir, file_rel)
            .await
            .map_err(|e| WebError::from_anyhow(e, &headers))?;
        files_view(State(s), Path(app), headers).await
    } else {
        // Save action.
        let content = form.content.unwrap_or_default();
        let app_dir = s.app_home().join(&app);
        files::write_file(&app_dir, &rel, &content)
            .await
            .map_err(|e| WebError::from_anyhow(e, &headers))?;
        files_view(State(s), Path(app), headers).await
    }
}
