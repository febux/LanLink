//! `lanlink-tui` — terminal frontend for LanLink.
//!
//! Connects to two surfaces:
//! - the daemon's local status socket (Unix domain socket, NDJSON) for live
//!   data-plane state, polled once per second;
//! - the coordinator REST API for the public directory and create/join
//!   operations (same semantics as `lanlinkctl`).
//!
//! Architecture: input / poller / action tasks feed [`Event`]s into one mpsc
//! channel; the main loop folds them into [`App`] state (a pure reducer that
//! returns [`Action`]s for the loop to dispatch) and redraws.

mod app;
mod coord;
mod daemon;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use tokio::sync::mpsc;

use app::{App, Event};

#[derive(Parser, Debug)]
#[command(
    name = "lanlink-tui",
    about = "LanLink terminal UI: live daemon status, directory, create/join"
)]
struct Args {
    /// Coordinator base URL.
    #[arg(long, default_value = "http://localhost:8080")]
    coordinator: String,

    /// Daemon status socket path. Defaults to auto-discovery of
    /// /run/lanlink/lanlinkd-*.sock or /tmp/lanlinkd-*.sock.
    #[arg(long)]
    status_socket: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let socket = match args.status_socket {
        Some(path) => path,
        None => daemon::discover_socket()
            .await
            .unwrap_or_else(|| PathBuf::from("/run/lanlink/lanlinkd-0.sock")),
    };
    let socket_desc = socket.display().to_string();

    // Terminal setup. The panic hook restores the terminal even if something
    // below panics, so a crash never leaves the shell unusable.
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        original_hook(info);
    }));

    let (event_tx, event_rx) = mpsc::channel::<Event>(64);

    spawn_input(event_tx.clone());
    spawn_snapshot_poller(socket.clone(), event_tx.clone());
    spawn_directory_poller(args.coordinator.clone(), event_tx.clone());

    let mut app = App::new(args.coordinator.clone(), socket_desc);

    let result = run_loop(&mut terminal, &mut app, event_rx, event_tx.clone()).await;

    // Terminal teardown.
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

/// Fold events into app state, dispatch returned actions, redraw.
async fn run_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut event_rx: mpsc::Receiver<Event>,
    event_tx: mpsc::Sender<Event>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        tokio::select! {
            event = event_rx.recv() => {
                let Some(event) = event else { break };
                let actions = app.on_event(event);
                for action in actions {
                    let base = app.coordinator.clone();
                    coord::dispatch(action, base, event_tx.clone());
                }
                if app.quit {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                app.on_event(Event::Tick);
            }
        }
    }
    Ok(())
}

/// Blocking crossterm reader on a dedicated thread (event::read blocks).
fn spawn_input(tx: mpsc::Sender<Event>) {
    tokio::task::spawn_blocking(move || loop {
        match crossterm::event::read() {
            Ok(crossterm::event::Event::Key(key)) => {
                if tx.blocking_send(Event::Key(key)).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    });
}

/// Poll the daemon status socket once per second. Errors are events too:
/// they drive the "daemon offline" indicator.
fn spawn_snapshot_poller(socket: PathBuf, tx: mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let result = daemon::fetch_snapshot(&socket)
                .await
                .map_err(|e| e.to_string());
            if tx.send(Event::Snapshot(result)).await.is_err() {
                break;
            }
        }
    });
}

/// Refresh the coordinator directory every 10 seconds.
fn spawn_directory_poller(coordinator: String, tx: mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("failed to build http client");
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let result = coord::list_directory(&client, &coordinator)
                .await
                .map_err(|e| e.to_string());
            if tx.send(Event::Directory(result)).await.is_err() {
                break;
            }
        }
    });
}
