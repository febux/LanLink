//! `lanlink-gui` — unified GUI launcher for LanLink.
//!
//! Starts either the terminal UI (`lanlink-tui`), the browser WebUI
//! (`lanlink-web`), or the Tauri desktop app (`lanlink-desktop`) with a single
//! command. By default it connects to a coordinator and daemon that are already
//! running; use `--dev` to start the full development stack (coordinator +
//! daemon + GUI) in one shot.
//!
//! Examples:
//!
//! ```text
//! # Terminal UI, connect to running daemon/coordinator
//! lanlink-gui
//! lanlink-gui --mode tui
//!
//! # Browser UI, connect to running daemon/coordinator
//! lanlink-gui --mode web
//! lanlink-gui --mode web --bind 0.0.0.0:8686 --no-browser
//!
//! # Native desktop UI, connect to running daemon/coordinator
//! lanlink-gui --mode desktop
//!
//! # Full development stack with one command
//! lanlink-gui --dev --mode tui
//! lanlink-gui --dev --mode web --bind 127.0.0.1:8686
//! lanlink-gui --dev --mode desktop
//! ```

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "lanlink-gui",
    about = "Unified GUI launcher for LanLink (TUI, WebUI, or Desktop)",
    long_about = "Launch the LanLink terminal UI, browser UI, or native desktop UI with one command. \
        Connects to an already-running daemon and coordinator by default; \
        use --dev to start the full stack."
)]
struct Args {
    /// GUI mode: terminal (tui), browser (web), or native desktop (desktop).
    #[arg(long, value_enum, default_value = "tui")]
    mode: Mode,

    /// Start the full development stack (coordinator + daemon + GUI).
    /// When omitted, only the GUI frontend is started and it connects to
    /// services that are already running.
    #[arg(long)]
    dev: bool,

    /// Coordinator base URL.
    #[arg(long, default_value = "http://localhost:8080")]
    coordinator: String,

    /// Daemon status socket path. Defaults to auto-discovery in the
    /// launched frontend.
    #[arg(long)]
    status_socket: Option<PathBuf>,

    /// WebUI bind address (web mode only).
    #[arg(long, default_value = "127.0.0.1:8686")]
    bind: String,

    /// Do not open the browser in web mode.
    #[arg(long)]
    no_browser: bool,

    /// Directory containing the sibling binaries (lanlink-tui, lanlink-web,
    /// lanlink-desktop, lanlink-coordinator, lanlinkd). Auto-detected from the
    /// executable location by default.
    #[arg(long)]
    bin_dir: Option<PathBuf>,

    /// Build profile used when auto-detecting the binary directory.
    /// Ignored when --bin-dir is set.
    #[arg(long, value_enum, default_value = "debug")]
    profile: Profile,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default)]
enum Mode {
    #[default]
    Tui,
    Web,
    Desktop,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq)]
enum Profile {
    #[default]
    Debug,
    Release,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    if args.dev {
        start_dev_stack(&args).await
    } else {
        start_frontend(&args).await
    }
}

/// Start only the chosen frontend binary and wait for it.
async fn start_frontend(args: &Args) -> Result<()> {
    let bin_dir = resolve_bin_dir(args)?;

    match args.mode {
        Mode::Tui => start_tui(bin_dir, args).await,
        Mode::Web => start_web(bin_dir, args).await,
        Mode::Desktop => start_desktop(bin_dir, args).await,
    }
}

/// Start the full development stack: coordinator, daemon (as root), and GUI.
async fn start_dev_stack(args: &Args) -> Result<()> {
    // In --dev mode we hand off to the shell script that already knows how to
    // juggle sudo, process groups, and cleanup. This keeps the Rust launcher
    // small and avoids re-implementing privilege escalation in Rust.
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("scripts/gui-start.sh"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("scripts/gui-start.sh"));

    if !script.exists() {
        anyhow::bail!(
            "dev launcher script not found: {}. Use --mode without --dev to connect to an already-running daemon.",
            script.display()
        );
    }

    let mut cmd = Command::new("bash");
    cmd.arg(&script);

    if args.profile == Profile::Release {
        cmd.arg("--release");
    }

    cmd.arg("--mode").arg(mode_str(args.mode));
    cmd.arg("--bind").arg(&args.bind);
    if args.no_browser {
        cmd.arg("--no-browser");
    }
    cmd.arg("--coordinator").arg(&args.coordinator);
    if let Some(socket) = &args.status_socket {
        cmd.arg("--status-socket").arg(socket);
    }
    if let Some(dir) = &args.bin_dir {
        cmd.arg("--bin-dir").arg(dir);
    }

    tracing::info!("starting dev stack via {}", script.display());
    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", script.display()))?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("dev stack exited with status: {}", status);
    }
}

async fn start_tui(bin_dir: PathBuf, args: &Args) -> Result<()> {
    let binary = bin_dir.join("lanlink-tui");
    if !binary.exists() {
        anyhow::bail!(
            "lanlink-tui binary not found at {}. Build with: cargo build -p lanlink-tui",
            binary.display()
        );
    }

    let mut cmd = Command::new(&binary);
    cmd.arg("--coordinator").arg(&args.coordinator);
    if let Some(socket) = &args.status_socket {
        cmd.arg("--status-socket").arg(socket);
    }

    tracing::info!("starting TUI: {}", binary.display());
    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", binary.display()))?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("lanlink-tui exited with status: {}", status);
    }
}

async fn start_web(bin_dir: PathBuf, args: &Args) -> Result<()> {
    let binary = bin_dir.join("lanlink-web");
    if !binary.exists() {
        anyhow::bail!(
            "lanlink-web binary not found at {}. Build with: cargo build -p lanlink-web",
            binary.display()
        );
    }

    let url = format!("http://{}", args.bind.replace("0.0.0.0", "127.0.0.1"));

    let mut cmd = Command::new(&binary);
    cmd.arg("--bind")
        .arg(&args.bind)
        .arg("--coordinator")
        .arg(&args.coordinator);
    if let Some(socket) = &args.status_socket {
        cmd.arg("--status-socket").arg(socket);
    }

    tracing::info!("starting WebUI on {}", url);
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to run {}", binary.display()))?;

    // Wait briefly for the server to come up before opening the browser.
    let bind = args.bind.clone();
    let port = bind
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse::<u16>().ok())
        .context("invalid --bind address; expected host:port")?;

    for _ in 0..50 {
        if port_open("127.0.0.1", port).await {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    if !args.no_browser {
        open_browser(&url);
    } else {
        println!("WebUI running at {}", url);
    }

    wait_for_child(child).await
}

async fn start_desktop(bin_dir: PathBuf, args: &Args) -> Result<()> {
    let binary = bin_dir.join("lanlink-desktop");
    if !binary.exists() {
        anyhow::bail!(
            "lanlink-desktop binary not found at {}. Build with: cargo build -p lanlink-desktop",
            binary.display()
        );
    }

    let mut cmd = Command::new(&binary);
    cmd.arg("--coordinator").arg(&args.coordinator);
    if let Some(socket) = &args.status_socket {
        cmd.arg("--status-socket").arg(socket);
    }

    tracing::info!("starting desktop app: {}", binary.display());
    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", binary.display()))?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("lanlink-desktop exited with status: {}", status);
    }
}

async fn wait_for_child(mut child: std::process::Child) -> Result<()> {
    let status = child.wait().context("failed to wait for child process")?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("GUI process exited with status: {}", status);
    }
}

async fn port_open(host: &str, port: u16) -> bool {
    tokio::net::TcpStream::connect((host, port)).await.is_ok()
}

fn open_browser(url: &str) {
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
}

fn resolve_bin_dir(args: &Args) -> Result<PathBuf> {
    if let Some(dir) = &args.bin_dir {
        return Ok(dir.clone());
    }

    // Try the directory that contains this binary first; this is the common
    // case when running a cargo-built binary or an installed package.
    let mut exe =
        std::env::current_exe().with_context(|| "could not determine current executable path")?;
    exe.pop();
    let candidate = exe.canonicalize().unwrap_or(exe);
    if candidate.join("lanlink-tui").exists() {
        return Ok(candidate);
    }

    // Fall back to the cargo target directory layout.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .context("could not determine workspace root")?;
    let target_dir = match args.profile {
        Profile::Debug => workspace_root.join("target/debug"),
        Profile::Release => workspace_root.join("target/release"),
    };
    Ok(target_dir)
}

fn mode_str(mode: Mode) -> &'static str {
    match mode {
        Mode::Tui => "tui",
        Mode::Web => "web",
        Mode::Desktop => "desktop",
    }
}
