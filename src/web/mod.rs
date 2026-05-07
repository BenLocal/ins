pub mod assets;
pub mod error;
pub mod jobs;
pub mod state;
pub mod templates;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use axum::response::Html;
use axum::{Router, routing::get};
use tokio::net::TcpListener;

use crate::config::InsConfig;
use crate::web::state::AppState;

pub struct WebOptions {
    pub bind: SocketAddr,
    #[allow(dead_code)] // consumed in later tasks
    pub no_open: bool,
    pub token: Option<String>,
}

pub async fn run(home: PathBuf, config: Arc<InsConfig>, options: WebOptions) -> anyhow::Result<()> {
    let token_str = options.token.clone().unwrap_or_else(|| "none".to_string());
    let state = AppState {
        home: Arc::new(home),
        config,
        jobs: Arc::new(crate::web::jobs::JobRegistry::default()),
        token: options.token.map(Arc::new),
        templates: crate::web::templates::build(),
    };

    let app = Router::new()
        .route("/", get(render_index))
        .route("/static/htmx.min.js", get(assets::htmx))
        .route("/static/htmx-sse.js", get(assets::htmx_sse))
        .route("/static/style.css", get(assets::style))
        .with_state(state);

    let listener = TcpListener::bind(options.bind)
        .await
        .with_context(|| format!("bind {}", options.bind))?;
    let actual = listener.local_addr()?;
    println!("Listening on http://{actual}/  (token: {token_str})");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("axum serve")
}

async fn render_index(State(s): State<AppState>) -> Html<String> {
    let tmpl = s.templates.get_template("index.html").expect("template");
    Html(tmpl.render(()).expect("render"))
}
