pub mod assets;
pub mod auth;
pub mod error;
pub mod handlers;
pub mod jobs;
pub mod state;
pub mod templates;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::{
    Router,
    routing::{get, post},
};
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
        .route("/", get(handlers::index::render))
        .route(
            "/nodes",
            get(handlers::nodes::list).post(handlers::nodes::create),
        )
        .route("/nodes/new", get(handlers::nodes::new_form))
        .route(
            "/nodes/:name",
            get(handlers::nodes::detail).post(handlers::nodes::update),
        )
        .route("/nodes/:name/edit", get(handlers::nodes::edit_form))
        .route("/nodes/:name/delete", post(handlers::nodes::delete))
        .route("/apps", get(handlers::apps::list))
        .route("/apps/:app", get(handlers::apps::files_view))
        .route("/apps/:app/files", post(handlers::apps::create))
        .route(
            "/apps/:app/files/*rel",
            get(handlers::apps::editor).post(handlers::apps::save_or_delete),
        )
        .route("/services", get(handlers::services::list))
        .route("/services/:idx", get(handlers::services::detail))
        .route(
            "/services/:idx/check",
            post(handlers::services::start_check),
        )
        .route(
            "/services/:idx/deploy",
            post(handlers::services::start_deploy),
        )
        .route("/jobs/:id/stream", get(handlers::services::stream))
        .route("/static/htmx.min.js", get(assets::htmx))
        .route("/static/htmx-sse.js", get(assets::htmx_sse))
        .route("/static/style.css", get(assets::style))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::token_guard,
        ));

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
