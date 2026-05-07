use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::{Router, response::Html, routing::get};
use tokio::net::TcpListener;

use crate::config::InsConfig;

pub struct WebOptions {
    pub bind: SocketAddr,
    #[allow(dead_code)] // consumed in later tasks
    pub no_open: bool,
    pub token: Option<String>,
}

pub async fn run(home: PathBuf, config: Arc<InsConfig>, options: WebOptions) -> anyhow::Result<()> {
    let _ = (home, config); // wired up in later tasks
    let app = Router::new().route("/", get(|| async { Html("<h1>ins web</h1>") }));
    let listener = TcpListener::bind(options.bind)
        .await
        .with_context(|| format!("bind {}", options.bind))?;
    let actual = listener.local_addr()?;
    let token_str = options.token.as_deref().unwrap_or("none");
    println!("Listening on http://{actual}/  (token: {token_str})");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("axum serve")
}
