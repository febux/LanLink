//! Cross-platform virtual NIC abstraction for LanLink.
//!
//! The same layer-2 switch core is fed raw Ethernet frames regardless of OS.
//! Backends:
//!
//! - Linux: kernel TAP device (`/dev/net/tun` in TAP mode) via the `tun` crate.
//! - Windows: Wintun ring-based adapter (WireGuard project driver) — stub for now.
//! - macOS: utun raw Ethernet interface — stub for now.

use std::io;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Error)]
pub enum NicError {
    #[error("virtual NIC creation failed: {0}")]
    Create(io::Error),
    #[error("platform operation failed: {0}")]
    Platform(String),
    #[error("unsupported platform")]
    Unsupported,
}

/// Configuration for one virtual Ethernet adapter.
#[derive(Debug, Clone)]
pub struct NicConfig {
    pub name: String,
    pub mtu: usize,
    pub ipv4: std::net::Ipv4Addr,
    pub netmask: std::net::Ipv4Addr,
}

impl Default for NicConfig {
    fn default() -> Self {
        Self {
            name: "lanlink0".to_string(),
            mtu: 1400,
            ipv4: std::net::Ipv4Addr::new(26, 0, 0, 1),
            netmask: std::net::Ipv4Addr::new(255, 0, 0, 0),
        }
    }
}

/// Platform-agnostic virtual NIC trait.
#[async_trait::async_trait]
pub trait VirtualNic: Send {
    /// Read one raw Ethernet frame into `buf`. Returns the number of bytes read.
    async fn read_frame(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Write one raw Ethernet frame.
    async fn write_frame(&mut self, frame: &[u8]) -> io::Result<()>;

    /// Return the interface name.
    fn name(&self) -> &str;
}

/// Factory: create the best backend for the current platform.
pub fn create_nic(config: NicConfig) -> Result<Box<dyn VirtualNic>, NicError> {
    #[cfg(target_os = "linux")]
    {
        LinuxTun::create(config).map(|d| Box::new(d) as Box<dyn VirtualNic>)
    }
    #[cfg(target_os = "windows")]
    {
        WindowsWintun::create(config).map(|d| Box::new(d) as Box<dyn VirtualNic>)
    }
    #[cfg(target_os = "macos")]
    {
        MacosUtun::create(config).map(|d| Box::new(d) as Box<dyn VirtualNic>)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Err(NicError::Unsupported)
    }
}

// ---------------------------------------------------------------------------
// Linux TAP backend
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub struct LinuxTun {
    name: String,
    dev: tun::AsyncDevice,
}

#[cfg(target_os = "linux")]
impl LinuxTun {
    pub fn create(config: NicConfig) -> Result<Self, NicError> {
        // LanLink switches raw Ethernet frames, so the device must be a TAP
        // (layer 2), not a TUN (layer 3). Older versions of this backend left
        // the layer at the crate default, which is TUN on Linux.
        let mut builder = tun::Configuration::default();
        builder.tun_name(&config.name).layer(tun::Layer::L2);

        let dev = tun::create_as_async(&builder).map_err(|e| {
            NicError::Create(io::Error::other(format!(
                "create TAP device '{}': {e}",
                config.name
            )))
        })?;

        // Configure the address/mask/mtu with `ip` (iproute2 / netlink) rather
        // than the tun crate's legacy SIOCSIF* ioctls. Those ioctls reject
        // classless netmasks such as 26.0.0.x/24 because of historic classful
        // checks; netlink is classless and matches how the gateway code already
        // configures routes.
        let prefix = netmask_to_prefix(config.netmask);
        run_ip(&[
            "addr",
            "add",
            &format!("{}/{}", config.ipv4, prefix),
            "dev",
            &config.name,
        ])
        .map_err(|e| {
            NicError::Create(io::Error::other(format!(
                "add address on '{}': {e}",
                config.name
            )))
        })?;

        run_ip(&[
            "link",
            "set",
            "dev",
            &config.name,
            "mtu",
            &config.mtu.to_string(),
            "up",
        ])
        .map_err(|e| {
            NicError::Create(io::Error::other(format!("bring up '{}': {e}", config.name)))
        })?;

        tracing::info!(
            name = %config.name,
            ip = %config.ipv4,
            prefix,
            mtu = config.mtu,
            "created Linux TAP device"
        );
        Ok(Self {
            name: config.name,
            dev,
        })
    }
}

fn netmask_to_prefix(mask: std::net::Ipv4Addr) -> u8 {
    mask.octets().iter().map(|b| b.count_ones() as u8).sum()
}

fn run_ip(args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("ip")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run 'ip {}': {e}", args.join(" ")))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "'ip {}' failed ({}): {}",
            args.join(" "),
            output.status,
            stderr.trim()
        ))
    }
}

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl VirtualNic for LinuxTun {
    async fn read_frame(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.dev.read(buf).await
    }

    async fn write_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        let n = self.dev.write(frame).await?;
        if n != frame.len() {
            return Err(io::Error::other("short write to TAP"));
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ---------------------------------------------------------------------------
// Windows Wintun backend (stub)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub struct WindowsWintun {
    name: String,
}

#[cfg(target_os = "windows")]
impl WindowsWintun {
    pub fn create(config: NicConfig) -> Result<Self, NicError> {
        Err(NicError::Platform(
            "Wintun backend not yet implemented".to_string(),
        ))
    }
}

#[cfg(target_os = "windows")]
#[async_trait::async_trait]
impl VirtualNic for WindowsWintun {
    async fn read_frame(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        unimplemented!("Wintun backend")
    }

    async fn write_frame(&mut self, _frame: &[u8]) -> io::Result<()> {
        unimplemented!("Wintun backend")
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ---------------------------------------------------------------------------
// macOS utun backend (stub)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub struct MacosUtun {
    name: String,
}

#[cfg(target_os = "macos")]
impl MacosUtun {
    pub fn create(config: NicConfig) -> Result<Self, NicError> {
        Err(NicError::Platform(
            "macOS utun backend not yet implemented".to_string(),
        ))
    }
}

#[cfg(target_os = "macos")]
#[async_trait::async_trait]
impl VirtualNic for MacosUtun {
    async fn read_frame(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        unimplemented!("macOS utun backend")
    }

    async fn write_frame(&mut self, _frame: &[u8]) -> io::Result<()> {
        unimplemented!("macOS utun backend")
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_26_net() {
        let cfg = NicConfig::default();
        assert_eq!(cfg.ipv4.octets()[0], 26);
    }
}
