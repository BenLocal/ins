use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Html;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::cli::service::list_service_records;
use crate::pipeline::PipelineMode;
use crate::web::error::{WebError, WebResult};
use crate::web::state::AppState;

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
    let services = list_service_records(&s.home)
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    let view: Vec<_> = services
        .iter()
        .map(|svc| {
            serde_json::json!({
                "label": format!("{}/{}/{}", svc.node_name, svc.app_name, svc.service),
            })
        })
        .collect();
    render(
        &s,
        "services/list.html",
        minijinja::context! { services => view },
        &headers,
    )
}

pub async fn detail(
    State(s): State<AppState>,
    Path(idx): Path<usize>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    let services = list_service_records(&s.home)
        .await
        .map_err(|e| WebError::from_anyhow(e, &headers))?;
    let svc = services
        .get(idx)
        .ok_or_else(|| WebError::not_found(format!("service idx {idx} out of range")))?;
    let detail_text = crate::tui::state::service_detail(svc);
    render(
        &s,
        "services/detail.html",
        minijinja::context! { detail => detail_text },
        &headers,
    )
}

pub async fn start_check(
    State(s): State<AppState>,
    Path(idx): Path<usize>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    start(s, idx, PipelineMode::Check, &headers).await
}

pub async fn start_deploy(
    State(s): State<AppState>,
    Path(idx): Path<usize>,
    headers: HeaderMap,
) -> WebResult<Html<String>> {
    start(s, idx, PipelineMode::Deploy, &headers).await
}

async fn start(
    s: AppState,
    idx: usize,
    mode: PipelineMode,
    headers: &HeaderMap,
) -> WebResult<Html<String>> {
    let services = list_service_records(&s.home)
        .await
        .map_err(|e| WebError::from_anyhow(e, headers))?;
    let svc = services
        .get(idx)
        .ok_or_else(|| WebError::not_found(format!("service idx {idx} out of range")))?
        .clone();
    let job = s
        .jobs
        .spawn(mode, svc.clone(), (*s.home).clone(), s.config.clone())
        .await;
    render(
        &s,
        "services/job.html",
        minijinja::context! {
            job_id => job.id,
            mode => match mode {
                PipelineMode::Check => "check",
                PipelineMode::Deploy => "deploy",
            },
            service => format!("{}/{}/{}", svc.node_name, svc.app_name, svc.service),
        },
        headers,
    )
}

pub async fn stream(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, WebError> {
    let job = s
        .jobs
        .get(&id)
        .await
        .ok_or_else(|| WebError::not_found(format!("job '{id}' not found")))?;
    let backlog = job.output.snapshot();
    let rx = job
        .output
        .subscribe()
        .ok_or_else(|| WebError::bad_request("job output is not streaming"))?;
    let live = BroadcastStream::new(rx).filter_map(|item| async move {
        item.ok().map(|line| {
            if line.starts_with("[ins:done]") {
                Event::default().event("done").data(line)
            } else {
                Event::default().event("line").data(line)
            }
        })
    });
    let initial = stream::iter(vec![Event::default().event("backlog").data(backlog)]);
    let merged = initial.chain(live).map(Ok::<_, Infallible>);
    Ok(Sse::new(merged).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
