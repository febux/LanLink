//! Native desktop GUI for LanLink using Tauri v2.
//!
//! This crate wraps the existing web UI assets and talks directly to the
//! daemon's local status socket and the coordinator REST API, so it works
//! cross-platform with no separate server process.

mod commands;

use std::path::PathBuf;

use clap::Parser;
use commands::DesktopState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

#[derive(Parser, Debug)]
#[command(name = "lanlink-desktop", about = "Native desktop GUI for LanLink")]
struct Args {
    /// Coordinator base URL.
    #[arg(long, default_value = "http://localhost:8080")]
    coordinator: String,

    /// Daemon status socket path. Defaults to auto-discovery of
    /// /run/lanlink/lanlinkd-*.sock or /tmp/lanlinkd-*.sock.
    #[arg(long)]
    status_socket: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let socket_desc = args
        .status_socket
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "auto-discover in /run/lanlink or /tmp".to_string());
    tracing::info!(
        coordinator = %args.coordinator,
        status_socket = %socket_desc,
        "starting lanlink-desktop"
    );

    let state = DesktopState::new(args.coordinator, args.status_socket);

    let app = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::resolve_status_socket,
            commands::list_directory,
            commands::create_network,
            commands::join_network,
            commands::open_browser,
            commands::coordinator_status,
            commands::coordinator_start,
            commands::coordinator_stop,
            commands::daemon_config_exists,
            #[cfg(windows)]
            commands::daemon_log_tail,
            #[cfg(windows)]
            commands::service_install,
            #[cfg(windows)]
            commands::service_uninstall,
            #[cfg(windows)]
            commands::service_start,
            #[cfg(windows)]
            commands::service_stop,
            #[cfg(windows)]
            commands::service_status,
            #[cfg(windows)]
            commands::service_up,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_clone.hide();
                    }
                });
            }
            build_tray(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Tauri app");

    app.run(move |_app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            // On all platforms, exit when the user asks.
            api.prevent_exit();
        }
    });
}

fn build_tray(handle: &AppHandle) -> anyhow::Result<()> {
    let show_i = MenuItem::with_id(handle, "show", "Show LanLink", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(handle, "quit", "Quit", true, None::<&str>)?;

    // Some Tauri versions / platforms are sensitive to the separator type in
    // `Menu::with_items`. Use a plain menu without separators to maximize
    // compatibility.
    let menu = Menu::with_items(handle, &[&show_i, &quit_i])?;

    let icon = handle.default_window_icon().cloned();
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        // On Linux we have to show the menu on left click; on macOS/Windows we
        // can toggle the window directly. We handle both by always showing the
        // menu on left click and relying on the menu entries.
        .show_menu_on_left_click(true);
    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }
    builder
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            tracing::info!(tray_event = %id, "tray menu activated");
            match id {
                "show" => show_window(app),
                "quit" => {
                    tracing::info!("quitting via tray menu");
                    // Use a direct process exit. Tauri's app.exit(0) can be
                    // intercepted by RunEvent::ExitRequested on some
                    // platforms and leave the tray icon alive.
                    std::process::exit(0);
                }
                _ => {}
            }
        })
        .build(handle)?;

    Ok(())
}

fn show_window(handle: &AppHandle) {
    if let Some(window) = handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
