use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::Html;
use serde::Deserialize;

use crate::cli::node::{NodeAddArgs, NodeSetArgs};
use crate::node::persist::{
    add_node_record, delete_node_record, list_node_records, nodes_file, set_node_record,
};
use crate::node::types::{NodeRecord, RemoteNodeRecord};
use crate::web::error::{WebError, WebResult};
use crate::web::state::AppState;

#[derive(Deserialize)]
pub struct NodeForm {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
}

fn empty_to_none(s: Option<String>) -> Option<String> {
    s.and_then(|v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
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

pub async fn list(State(s): State<AppState>, headers: HeaderMap) -> WebResult<Html<String>> {
    let nodes = list_node_records(&nodes_file(&s.home))
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    let view: Vec<_> = nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "name": match n {
                    NodeRecord::Local() => "local".to_string(),
                    NodeRecord::Remote(r) => r.name.clone(),
                },
                "label": match n {
                    NodeRecord::Local() => "local".to_string(),
                    NodeRecord::Remote(r) => format!("{} — {}@{}:{}", r.name, r.user, r.ip, r.port),
                },
                "removable": matches!(n, NodeRecord::Remote(_)),
            })
        })
        .collect();
    render(
        &s,
        "nodes/list.html",
        minijinja::context! { nodes => view },
        &headers,
    )
}

pub async fn detail(
    State(s): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    let nodes = list_node_records(&nodes_file(&s.home))
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    let detail_text = nodes
        .iter()
        .find(|n| match n {
            NodeRecord::Local() => name == "local",
            NodeRecord::Remote(r) => r.name == name,
        })
        .map(crate::node::detail::node_detail)
        .ok_or_else(|| WebError::not_found(format!("node '{name}' not found"), &headers))?;
    render(
        &s,
        "nodes/detail.html",
        minijinja::context! { detail => detail_text },
        &headers,
    )
}

pub async fn new_form(State(s): State<AppState>, headers: HeaderMap) -> WebResult<Html<String>> {
    render_form(&s, "add", "Add node", "Create", "/nodes", None, &headers)
}

pub async fn edit_form(
    State(s): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    let nodes = list_node_records(&nodes_file(&s.home))
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    let remote = nodes
        .into_iter()
        .find_map(|n| match n {
            NodeRecord::Remote(r) if r.name == name => Some(r),
            _ => None,
        })
        .ok_or_else(|| WebError::not_found(format!("node '{name}' not found"), &headers))?;
    let action = format!("/nodes/{}", remote.name);
    render_form(
        &s,
        "edit",
        "Edit node",
        "Save",
        &action,
        Some(&remote),
        &headers,
    )
}

fn render_form(
    s: &AppState,
    mode: &str,
    title: &str,
    submit_label: &str,
    action: &str,
    remote: Option<&RemoteNodeRecord>,
    headers: &HeaderMap,
) -> WebResult<Html<String>> {
    let ctx = minijinja::context! {
        mode => mode,
        title => title,
        submit_label => submit_label,
        action => action,
        name => remote.map(|r| r.name.clone()).unwrap_or_default(),
        ip => remote.map(|r| r.ip.clone()).unwrap_or_default(),
        port => remote.map(|r| r.port).unwrap_or(22),
        user => remote.map(|r| r.user.clone()).unwrap_or_default(),
        key_path => remote.and_then(|r| r.key_path.clone()).unwrap_or_default(),
    };
    render(s, "nodes/form.html", ctx, headers)
}

pub async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NodeForm>,
) -> WebResult<Html<String>> {
    add_node_record(
        &nodes_file(&s.home),
        NodeAddArgs {
            name: form.name,
            ip: form.ip,
            port: form.port,
            user: form.user,
            password: empty_to_none(form.password).unwrap_or_default(),
            key_path: empty_to_none(form.key_path),
        },
    )
    .await
    .map_err(|e| WebError::from_anyhow(e, &headers))?;
    list(State(s), headers).await
}

pub async fn update(
    State(s): State<AppState>,
    Path(_name): Path<String>,
    headers: HeaderMap,
    Form(form): Form<NodeForm>,
) -> WebResult<Html<String>> {
    set_node_record(
        &nodes_file(&s.home),
        NodeSetArgs {
            name: form.name,
            ip: form.ip,
            port: form.port,
            user: form.user,
            password: empty_to_none(form.password).unwrap_or_default(),
            key_path: empty_to_none(form.key_path),
        },
    )
    .await
    .map_err(|e| WebError::from_anyhow(e, &headers))?;
    list(State(s), headers).await
}

pub async fn delete(
    State(s): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    delete_node_record(&nodes_file(&s.home), &name)
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    list(State(s), headers).await
}
