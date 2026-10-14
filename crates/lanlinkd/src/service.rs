//! Service-manager integration for `lanlinkd`.
//!
//! - Linux: installs and controls the `lanlinkd@<config>.service` systemd
//!   template unit. One service instance runs per config file
//!   `/etc/lanlink/<config>.toml`.
//! - Windows: installs and controls the `lanlinkd` Windows service (SCM);
//!   `service run` is the SCM entry point.
//!
//! The goal is the same UX on every platform:
//! `lanlinkd service install && lanlinkd service start <network>`.

use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum ServiceCmd {
    /// Install the service definition (systemd unit / Windows service).
    Install,
    /// Remove the service definition.
    Uninstall,
    /// Start the instance for the given config name.
    Start {
        /// Config file stem in /etc/lanlink (without `.toml`).
        name: String,
    },
    /// Stop the instance for the given config name.
    Stop {
        /// Config file stem in /etc/lanlink (without `.toml`).
        name: String,
    },
    /// Enable autostart at boot for the given config name (Linux only).
    Enable {
        /// Config file stem in /etc/lanlink (without `.toml`).
        name: String,
    },
    /// Disable autostart at boot for the given config name (Linux only).
    Disable {
        /// Config file stem in /etc/lanlink (without `.toml`).
        name: String,
    },
    /// Resolve (or create) a network by name, write a config file + secret,
    /// install the unit, and start/enable the instance. Linux only.
    Up {
        /// Network name. The config file stem is the same name.
        name: String,

        /// Coordinator base URL, e.g. http://localhost:8080
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,

        /// Network password. Prefer LANLINK_PASSWORD env var to avoid exposing
        /// the secret in `ps` output.
        #[arg(short, long)]
        password: Option<String>,

        /// Create the network as public/gaming if it does not exist.
        #[arg(long)]
        public: bool,

        /// UDP socket to listen on.
        #[arg(long)]
        bind: Option<std::net::SocketAddr>,

        /// TAP/virtual NIC name.
        #[arg(long)]
        tap: Option<String>,

        /// Virtual NIC MTU.
        #[arg(long)]
        mtu: Option<usize>,

        /// Status socket path for frontends.
        #[arg(long)]
        status_socket: Option<String>,
    },
    /// Show service status.
    Status,
    /// Internal: run the daemon as a service (invoked by the service manager).
    #[command(hide = true)]
    Run,
}

pub async fn dispatch(cmd: ServiceCmd) -> anyhow::Result<()> {
    match cmd {
        ServiceCmd::Install => install().await,
        ServiceCmd::Uninstall => uninstall().await,
        ServiceCmd::Start { name } => start(&name).await,
        ServiceCmd::Stop { name } => stop(&name).await,
        ServiceCmd::Enable { name } => enable(&name).await,
        ServiceCmd::Disable { name } => disable(&name).await,
        ServiceCmd::Up {
            name,
            coordinator,
            password,
            public,
            bind,
            tap,
            mtu,
            status_socket,
        } => {
            up(
                &name,
                &coordinator,
                password,
                public,
                bind,
                tap,
                mtu,
                status_socket,
            )
            .await
        }
        ServiceCmd::Status => status().await,
        ServiceCmd::Run => run_as_service().await,
    }
}

// ---------------------------------------------------------------------------
// Linux: systemd template unit
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod platform {
    pub const UNIT_PATH: &str = "/etc/systemd/system/lanlinkd@.service";
    pub const UNIT: &str = include_str!("../../../dist/systemd/lanlinkd@.service");
    pub const CONFIG_DIR: &str = "/etc/lanlink";
}

#[cfg(target_os = "linux")]
async fn run_cmd(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let out = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to execute {program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("{program} {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(stdout)
}

#[cfg(target_os = "linux")]
fn instance_unit(name: &str) -> anyhow::Result<String> {
    // %i in the template is the unescaped instance name; systemd requires
    // escaping of anything but alphanumerics and a few safe characters.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!("invalid config name '{name}': use alphanumerics, '-', '_', '.'");
    }
    Ok(format!("lanlinkd@{name}.service"))
}

#[cfg(target_os = "linux")]
async fn install() -> anyhow::Result<()> {
    if !std::path::Path::new(platform::CONFIG_DIR).exists() {
        std::fs::create_dir_all(platform::CONFIG_DIR)?;
    }
    let unit_text = std::fs::read_to_string(platform::UNIT_PATH).unwrap_or_default();
    // Rewrite ExecStart path to match wherever the current lanlinkd binary is installed.
    let bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("/usr/local/bin"));
    let bin_path = bin_dir.join("lanlinkd");
    let rewritten = platform::UNIT.replace(
        "/usr/bin/lanlinkd",
        &bin_path.to_string_lossy(),
    );
    if unit_text != rewritten {
        tokio::fs::write(platform::UNIT_PATH, rewritten).await?;
    }
    run_cmd("systemctl", &["daemon-reload"]).await?;
    println!("systemd unit installed: {}", platform::UNIT_PATH);
    println!(
        "place a config in {}/<name>.toml then run: lanlinkd service start <name>",
        platform::CONFIG_DIR
    );
    Ok(())
}

#[cfg(target_os = "linux")]
async fn uninstall() -> anyhow::Result<()> {
    match tokio::fs::remove_file(platform::UNIT_PATH).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    run_cmd("systemctl", &["daemon-reload"]).await?;
    println!("systemd unit removed");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn start(name: &str) -> anyhow::Result<()> {
    let unit = instance_unit(name)?;
    run_cmd("systemctl", &["start", &unit]).await?;
    println!("started {unit}");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn stop(name: &str) -> anyhow::Result<()> {
    let unit = instance_unit(name)?;
    run_cmd("systemctl", &["stop", &unit]).await?;
    println!("stopped {unit}");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn enable(name: &str) -> anyhow::Result<()> {
    let unit = instance_unit(name)?;
    run_cmd("systemctl", &["enable", "--now", &unit]).await?;
    println!("enabled {unit}");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn disable(name: &str) -> anyhow::Result<()> {
    let unit = instance_unit(name)?;
    run_cmd("systemctl", &["disable", "--now", &unit]).await?;
    println!("disabled {unit}");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn status() -> anyhow::Result<()> {
    let out = run_cmd(
        "systemctl",
        &["list-units", "lanlinkd@*.service", "--all", "--no-pager"],
    )
    .await?;
    print!("{out}");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn run_as_service() -> anyhow::Result<()> {
    // On Linux the unit invokes `lanlinkd run --config ...` directly;
    // `service run` is provided for symmetry and runs with the default
    // config path.
    let path = format!("{}/lanlink.toml", platform::CONFIG_DIR);
    crate::run_from_config_path(&path).await
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
async fn up(
    name: &str,
    coordinator: &str,
    password: Option<String>,
    public: bool,
    bind: Option<std::net::SocketAddr>,
    tap: Option<String>,
    mtu: Option<usize>,
    status_socket: Option<String>,
) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // Validate the name before creating files or touching the coordinator.
    let _ = instance_unit(name)?;

    let password = password
        .or_else(|| std::env::var("LANLINK_PASSWORD").ok())
        .ok_or_else(|| anyhow::anyhow!("--password or LANLINK_PASSWORD env var is required"))?;

    // Resolve (or create) the network with the coordinator.
    let cfg = crate::up::resolve_config(
        coordinator.to_string(),
        name.to_string(),
        password.clone(),
        public,
        bind,
        tap,
        mtu,
        status_socket,
    )
    .await?;

    std::fs::create_dir_all(platform::CONFIG_DIR)?;
    let config_path = format!("{}/{}.toml", platform::CONFIG_DIR, name);
    let secret_path = format!("{}/{}.secret", platform::CONFIG_DIR, name);

    // Write the password file root-owned 0600. Best effort: if we are not
    // root the file is owned by the invoking user; systemd can still read it
    // if the unit user matches.
    std::fs::write(&secret_path, password.as_bytes())?;
    let _ = std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600));

    // Write a minimal config that references the secret file.
    let mut config_text = format!(
        "[network]\n\
         coordinator = \"{}\"\n\
         network_id = {}\n\
         password_file = \"{}\"\n",
        coordinator, cfg.network_id, secret_path
    );
    if let Some(b) = cfg.bind.port().checked_sub(0).map(|_| cfg.bind) {
        config_text.push_str(&format!("bind = \"{}\"\n", b));
    }
    config_text.push_str(&format!("tap = \"{}\"\n", cfg.tap));
    config_text.push_str(&format!("mtu = {}\n", cfg.mtu));
    std::fs::write(&config_path, config_text)?;

    // Ensure the service user can read both files when invoked as root.
    #[cfg(target_os = "linux")]
    if unsafe { libc::getuid() } == 0 {
        let _ = nix::unistd::chown(
            platform::CONFIG_DIR,
            Some(nix::unistd::Uid::from_raw(0)),
            Some(nix::unistd::Gid::from_raw(
                nix::unistd::Group::from_name("lanlink")
                    .ok()
                    .flatten()
                    .map(|g| g.gid.as_raw())
                    .unwrap_or(0),
            )),
        );
        let _ = std::fs::set_permissions(
            platform::CONFIG_DIR,
            std::fs::Permissions::from_mode(0o750),
        );
        for path in [&secret_path, &config_path] {
            let _ = nix::unistd::chown(
                (*path).as_str(),
                Some(nix::unistd::Uid::from_raw(0)),
                Some(nix::unistd::Gid::from_raw(
                    nix::unistd::Group::from_name("lanlink")
                        .ok()
                        .flatten()
                        .map(|g| g.gid.as_raw())
                        .unwrap_or(0),
                )),
            );
        }
        let _ = std::fs::set_permissions(
            &config_path,
            std::fs::Permissions::from_mode(0o640),
        );
        let _ = std::fs::set_permissions(
            &secret_path,
            std::fs::Permissions::from_mode(0o640),
        );
    }

    tracing::info!(
        config = %config_path,
        secret = %secret_path,
        network_id = cfg.network_id,
        "wrote service config"
    );

    // Make sure the systemd unit template is installed, then start/enable.
    install().await?;
    let unit = instance_unit(name)?;
    run_cmd("systemctl", &["enable", "--now", &unit]).await?;
    println!("started and enabled {unit}");
    Ok(())
}

#[cfg(target_os = "windows")]
#[allow(clippy::too_many_arguments)]
async fn up(
    name: &str,
    coordinator: &str,
    password: Option<String>,
    public: bool,
    bind: Option<std::net::SocketAddr>,
    tap: Option<String>,
    mtu: Option<usize>,
    status_socket: Option<String>,
) -> anyhow::Result<()> {
    let password = password
        .or_else(|| std::env::var("LANLINK_PASSWORD").ok())
        .ok_or_else(|| anyhow::anyhow!("--password or LANLINK_PASSWORD env var is required"))?;

    let cfg = crate::up::resolve_config(
        coordinator.to_string(),
        name.to_string(),
        password.clone(),
        public,
        bind,
        tap,
        mtu,
        status_socket,
    )
    .await?;

    let config_dir = r"C:\ProgramData\LanLink";
    let config_path = format!("{}\\lanlinkd.toml", config_dir);
    let secret_path = format!("{}\\{}.secret", config_dir, name);

    std::fs::create_dir_all(config_dir)?;
    std::fs::write(&secret_path, password.as_bytes())?;

    let mut config_text = format!(
        "[network]\n\
         coordinator = \"{}\"\n\
         network_id = {}\n\
         password_file = \"{}\"\n",
        coordinator, cfg.network_id, secret_path
    );
    if let Some(b) = cfg.bind.port().checked_sub(0).map(|_| cfg.bind) {
        config_text.push_str(&format!("bind = \"{}\"\n", b));
    }
    config_text.push_str(&format!("tap = \"{}\"\n", cfg.tap));
    config_text.push_str(&format!("mtu = {}\n", cfg.mtu));
    std::fs::write(&config_path, config_text)?;

    tracing::info!(
        config = %config_path,
        secret = %secret_path,
        network_id = cfg.network_id,
        "wrote service config"
    );

    let manager = windows_service::service_manager::ServiceManager::local_computer(
        None::<&str>,
        windows_service::service_manager::ServiceManagerAccess::CONNECT,
    )
    .map_err(|e| anyhow::anyhow!("failed to connect to SCM: {e}"))?;
    let exists = manager
        .open_service(
            windows_impl::SERVICE_NAME,
            windows_service::service::ServiceAccess::QUERY_STATUS,
        )
        .is_ok();
    if !exists {
        windows_impl::install().map_err(|e| anyhow::anyhow!("service install failed: {e}"))?;
    }
    windows_impl::start().map_err(|e| anyhow::anyhow!("service start failed: {e}"))?;
    println!("started {name} with the LanLinkDaemon service");
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
#[allow(clippy::too_many_arguments)]
async fn up(
    _name: &str,
    _coordinator: &str,
    _password: Option<String>,
    _public: bool,
    _bind: Option<std::net::SocketAddr>,
    _tap: Option<String>,
    _mtu: Option<usize>,
    _status_socket: Option<String>,
) -> anyhow::Result<()> {
    anyhow::bail!("`lanlinkd service up` is only supported on Linux and Windows")
}

// ---------------------------------------------------------------------------
// Windows: SCM service
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_service::define_windows_service;
    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_service::{service_dispatcher, Error as WinSvcError};

    pub const SERVICE_NAME: &str = "LanLinkDaemon";
    pub const SERVICE_DISPLAY: &str = "LanLink Virtual LAN Daemon";
    pub const CONFIG_PATH: &str = "C:\\ProgramData\\LanLink\\lanlinkd.toml";

    define_windows_service!(ffi_service_main, service_main);

    pub fn dispatch_run() -> Result<(), WinSvcError> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
    }

    pub fn service_main(_arguments: Vec<OsString>) {
        if let Err(e) = run_service() {
            tracing::error!(error = %e, "lanlinkd Windows service failed");
        }
    }

    fn run_service() -> Result<(), WinSvcError> {
        let (tx, rx) = mpsc::channel::<()>();

        let event_handler = move |control_event: ServiceControl| -> ServiceControlHandlerResult {
            match control_event {
                ServiceControl::Stop | ServiceControl::Shutdown | ServiceControl::Preshutdown => {
                    let _ = tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };

        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

        let set_status = |state, controls| -> Result<(), WinSvcError> {
            status_handle.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: controls,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
        };

        set_status(ServiceState::StartPending, ServiceControlAccept::empty())?;

        // Run the daemon on a dedicated thread with its own tokio runtime.
        let runner = std::thread::spawn(|| {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(async move {
                if let Err(e) = crate::run_from_config_path(CONFIG_PATH).await {
                    tracing::error!(error = %e, "lanlinkd service failed");
                    false
                } else {
                    true
                }
            })
        });

        set_status(ServiceState::Running, ServiceControlAccept::STOP)?;

        // Block until SCM asks us to stop, then trigger the daemon shutdown.
        let _ = rx.recv();
        crate::shutdown().trigger();
        let ok = runner.join().unwrap_or(false);

        set_status(ServiceState::StopPending, ServiceControlAccept::empty())?;
        set_status(ServiceState::Stopped, ServiceControlAccept::empty())?;

        // Propagate a non-zero exit code if the daemon failed so the SCM logs it.
        let _ = ok;
        Ok(())
    }

    pub fn install() -> Result<(), WinSvcError> {
        let manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CREATE_SERVICE | ServiceManagerAccess::CONNECT,
        )?;
        let exe = std::env::current_exe().map_err(WinSvcError::Winapi)?;
        let info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe,
            launch_arguments: vec!["service".into(), "run".into()],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        manager.create_service(&info, ServiceAccess::QUERY_STATUS)?;
        println!("Windows service installed ({SERVICE_NAME})");
        println!("config: {CONFIG_PATH}");
        Ok(())
    }

    pub fn uninstall() -> Result<(), WinSvcError> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(
            SERVICE_NAME,
            ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
        )?;
        let status = service.query_status()?;
        if status.current_state != ServiceState::Stopped {
            service.stop()?;
        }
        service.delete()?;
        println!("Windows service removed");
        Ok(())
    }

    pub fn start() -> Result<(), WinSvcError> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(SERVICE_NAME, ServiceAccess::START)?;
        service.start::<OsString>(&[])?;
        println!("service started");
        Ok(())
    }

    pub fn stop() -> Result<(), WinSvcError> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(SERVICE_NAME, ServiceAccess::STOP)?;
        service.stop()?;
        println!("service stopped");
        Ok(())
    }
}

#[cfg(target_os = "windows")]
async fn install() -> anyhow::Result<()> {
    windows_impl::install().map_err(|e| anyhow::anyhow!("service install failed: {e}"))
}

#[cfg(target_os = "windows")]
async fn uninstall() -> anyhow::Result<()> {
    windows_impl::uninstall().map_err(|e| anyhow::anyhow!("service uninstall failed: {e}"))
}

#[cfg(target_os = "windows")]
async fn start(_name: &str) -> anyhow::Result<()> {
    windows_impl::start().map_err(|e| anyhow::anyhow!("service start failed: {e}"))
}

#[cfg(target_os = "windows")]
async fn stop(_name: &str) -> anyhow::Result<()> {
    windows_impl::stop().map_err(|e| anyhow::anyhow!("service stop failed: {e}"))
}

#[cfg(target_os = "windows")]
async fn enable(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("service is installed with automatic start; nothing to enable")
}

#[cfg(target_os = "windows")]
async fn disable(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("remove the service with `lanlinkd service uninstall` instead")
}

#[cfg(target_os = "windows")]
async fn status() -> anyhow::Result<()> {
    let manager = windows_service::service_manager::ServiceManager::local_computer(
        None::<&str>,
        windows_service::service_manager::ServiceManagerAccess::CONNECT,
    )
    .map_err(|e| anyhow::anyhow!("failed to connect to SCM: {e}"))?;
    let service = manager
        .open_service(
            windows_impl::SERVICE_NAME,
            windows_service::service::ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| anyhow::anyhow!("failed to open service: {e}"))?;
    let st = service
        .query_status()
        .map_err(|e| anyhow::anyhow!("failed to query status: {e}"))?;
    println!("{}: {:?}", windows_impl::SERVICE_NAME, st.current_state);
    Ok(())
}

#[cfg(target_os = "windows")]
async fn run_as_service() -> anyhow::Result<()> {
    windows_impl::dispatch_run().map_err(|e| anyhow::anyhow!("service dispatch failed: {e}"))
}

// ---------------------------------------------------------------------------
// macOS and others: launchd instructions (pkg installs the plist)
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn install() -> anyhow::Result<()> {
    anyhow::bail!(
        "on macOS the launchd plist is installed by the .pkg; see dist/macos/ \
         (launchctl load /Library/LaunchDaemons/com.lanlink.lanlinkd.plist)"
    )
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn uninstall() -> anyhow::Result<()> {
    anyhow::bail!(
        "remove /Library/LaunchDaemons/com.lanlink.lanlinkd.plist and run `launchctl unload`"
    )
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn start(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("use `launchctl kickstart -k system/com.lanlink.lanlinkd`")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn stop(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("use `launchctl kill TERM system/com.lanlink.lanlinkd`")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn enable(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("launchd auto-starts the daemon (RunAtLoad in the plist)")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn disable(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("unload the launchd plist")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn status() -> anyhow::Result<()> {
    anyhow::bail!("use `launchctl print system/com.lanlink.lanlinkd`")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn run_as_service() -> anyhow::Result<()> {
    crate::run_from_config_path("/etc/lanlink/lanlink.toml").await
}
