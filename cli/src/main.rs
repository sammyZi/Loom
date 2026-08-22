mod pick;
mod db;
mod routes;
mod state;
mod static_files;

use anyhow::Result;
use axum::Router;
use state::AppState;
use std::net::SocketAddr;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    enable_dpi();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state = AppState::new();
    // This API executes shell commands, so it must not answer cross-origin
    // requests from arbitrary websites: any tab could otherwise drive the IDE.
    // The embedded UI is same-origin and needs no CORS; only the dev server
    // does. Override with IDE_AI_EXTRA_ORIGIN when serving the UI elsewhere.
    let mut origins = vec![
        "http://localhost:3000".parse().unwrap(),
        "http://127.0.0.1:3000".parse().unwrap(),
    ];
    if let Ok(extra) = std::env::var("IDE_AI_EXTRA_ORIGIN") {
        if let Ok(o) = extra.parse() {
            origins.push(o);
        }
    }
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .merge(routes::router())
        .fallback(static_files::fallback)
        .layer(cors)
        .with_state(state);

    let bind = std::env::var("IDE_AI_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let addr: SocketAddr = bind.parse()?;
    tracing::info!("listening on http://{addr}");
    let url = format!("http://{addr}");
    let _ = webbrowser::open(&url);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn enable_dpi() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}
