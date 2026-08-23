// Console only in debug builds; release runs as a windowed app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pick;
mod db;
mod routes;
mod state;
mod static_files;

use anyhow::Result;
use axum::Router;
use state::AppState;
use std::net::SocketAddr;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use wry::WebViewBuilder;

fn main() -> Result<()> {
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

    // The window owns the main thread, so the server runs on its own runtime.
    let rt = tokio::runtime::Runtime::new()?;
    let listener = rt.block_on(tokio::net::TcpListener::bind(addr))?;
    let addr = listener.local_addr()?;
    tracing::info!("listening on http://{addr}");
    rt.spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("server stopped: {e}");
        }
    });

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Loom")
        .with_window_icon(app_icon())
        .with_inner_size(LogicalSize::new(1400.0, 900.0))
        .build(&event_loop)?;
    let _webview = WebViewBuilder::new()
        .with_url(format!("http://{addr}"))
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

/// The icon compiled into the exe by `app.rc`.
fn app_icon() -> Option<tao::window::Icon> {
    #[cfg(windows)]
    {
        use tao::platform::windows::IconExtWindows;
        tao::window::Icon::from_resource(1, None).ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
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
