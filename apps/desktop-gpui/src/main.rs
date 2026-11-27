//! ApiArk GPUI — native Rust desktop application entry point.
//!
//! This crate is an in-progress migration from Tauri. Some state/models/theme
//! definitions are scaffolding for features wired up in later migration phases.
#![allow(dead_code)]
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod models;
mod runtime;
mod services;
mod state;
mod theme;
mod ui;

use gpui_kit::{
    application, assets, component::{Theme, ThemeMode}, init, open_window, px, App, Bounds,
    KeyBinding, WindowBounds, WindowOptions,
};

use app::{AppView, FocusEnvironment, NewTab, OpenCommandPalette, SaveRequest};

fn main() {
    let thread = std::thread::Builder::new()
        .name("apiark-main".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .expect("Failed to start APIArk main thread");
    thread.join().expect("APIArk main thread panicked");
}

fn run() {
    init_logging();
    runtime::init();

    application()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            init(cx);
            // Use gpui-component's light theme so inputs/buttons match the
            // original ApiArk desktop palette.
            Theme::change(ThemeMode::Light, None, cx);
            cx.bind_keys([
                KeyBinding::new("cmd-s", SaveRequest, None),
                KeyBinding::new("ctrl-s", SaveRequest, None),
                KeyBinding::new("cmd-p", OpenCommandPalette, None),
                KeyBinding::new("ctrl-p", OpenCommandPalette, None),
                KeyBinding::new("cmd-e", FocusEnvironment, None),
                KeyBinding::new("ctrl-e", FocusEnvironment, None),
                KeyBinding::new("cmd-n", NewTab, None),
                KeyBinding::new("ctrl-n", NewTab, None),
            ]);

            let bounds = Bounds::centered(None, gpui_kit::size(px(1280.0), px(800.0)), cx);

            open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| AppView::new(window, cx),
            )
            .expect("Failed to open main window");
        });
}

fn init_logging() {
    let apiark_dir = dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".apiark");
    let log_dir = apiark_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "apiark.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "apiark_gpui=info".into()),
        )
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    let crash_dir = apiark_dir.join("crash-reports");
    let _ = std::fs::create_dir_all(&crash_dir);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "Unknown panic".to_string());
        let report = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "appVersion": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "message": payload,
        });
        let path = crash_dir.join(format!("crash_{}.json", timestamp));
        let _ = std::fs::write(
            path,
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        );
        default_hook(info);
    }));

    tracing::info!("ApiArk GPUI starting");
}
