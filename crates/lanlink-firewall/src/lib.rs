//! Per-network inbound firewall profile for LanLink.
//!
//! Public/gaming networks enable a default-deny style profile that drops common
//! Windows/Linux services (SMB, RDP, SSH, NetBIOS) on the virtual TAP interface.
//! The implementation uses `nftables` on Linux; other platforms return
//! `FirewallError::Unsupported`.

use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FirewallError {
    #[error("nftables command failed: {0}")]
    Nft(io::Error),
    #[error("nftables rejected the rules: {0}")]
    NftRejected(String),
    #[error("firewall not supported on this platform")]
    Unsupported,
}

/// Inbound firewall profile applied to one virtual NIC.
#[derive(Debug, Clone)]
pub struct FirewallProfile {
    /// If false, the profile is a no-op.
    pub enabled: bool,
    /// Kernel interface name (e.g. `lanlink0`).
    pub interface: String,
    /// TCP destination ports to drop on ingress.
    pub block_tcp_ports: Vec<u16>,
    /// UDP destination ports to drop on ingress.
    pub block_udp_ports: Vec<u16>,
    /// Allow ICMP echo-request through.
    pub allow_ping: bool,
}

impl FirewallProfile {
    /// Default public/gaming profile. Blocks SMB, NetBIOS, RDP and SSH inbound.
    pub fn public(interface: impl Into<String>) -> Self {
        Self {
            enabled: true,
            interface: interface.into(),
            block_tcp_ports: vec![139, 445, 3389, 22],
            block_udp_ports: vec![137, 138],
            allow_ping: true,
        }
    }

    /// Disabled profile that never installs rules.
    pub fn disabled(interface: impl Into<String>) -> Self {
        Self {
            enabled: false,
            interface: interface.into(),
            block_tcp_ports: Vec::new(),
            block_udp_ports: Vec::new(),
            allow_ping: true,
        }
    }

    /// Name of the nftables table used for this interface.
    #[cfg(target_os = "linux")]
    fn table_name(&self) -> String {
        sanitize_table_name(&self.interface)
    }

    /// Atomically install the firewall rules via `nft -f -`. Any previous rules
    /// for the same interface (e.g. left over from a crashed run) are replaced,
    /// making this safe to call repeatedly.
    #[cfg(target_os = "linux")]
    pub async fn apply(&self) -> Result<(), FirewallError> {
        if !self.enabled {
            return self.clear().await;
        }
        // Delete any previous table first so re-application never duplicates
        // rules (nft `table` blocks merge into existing tables).
        let _ = self.clear().await;
        let script = self.build_script();
        run_nft(&script).await
    }

    /// Remove the nftables table for this interface.
    #[cfg(target_os = "linux")]
    pub async fn clear(&self) -> Result<(), FirewallError> {
        let table = self.table_name();
        let script = format!("delete table inet {}\n", table);
        // Deleting a non-existent table returns ENOENT; ignore it.
        match run_nft(&script).await {
            Ok(()) => Ok(()),
            Err(FirewallError::NftRejected(ref stderr))
                if stderr.contains("No such file") || stderr.contains("does not exist") =>
            {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn apply(&self) -> Result<(), FirewallError> {
        Err(FirewallError::Unsupported)
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn clear(&self) -> Result<(), FirewallError> {
        Err(FirewallError::Unsupported)
    }

    /// Build an nftables script for this profile.
    #[cfg(target_os = "linux")]
    fn build_script(&self) -> String {
        let table = self.table_name();
        let mut script = format!("table inet {} {{", table);
        script.push_str(
            "    chain input {
        type filter hook input priority 0; policy accept;\n",
        );
        // Only evaluate packets arriving on the virtual interface.
        script.push_str(&format!(
            "        iifname != \"{}\" accept\n",
            self.interface
        ));
        script.push_str("        ct state established,related accept\n");
        if self.allow_ping {
            script.push_str("        ip protocol icmp icmp type echo-request accept\n");
        }
        if !self.block_tcp_ports.is_empty() {
            let set = port_set(&self.block_tcp_ports);
            script.push_str(&format!("        tcp dport {} drop\n", set));
        }
        if !self.block_udp_ports.is_empty() {
            let set = port_set(&self.block_udp_ports);
            script.push_str(&format!("        udp dport {} drop\n", set));
        }
        script.push_str("    }\n");
        script.push_str("}\n");
        script
    }
}

#[cfg(target_os = "linux")]
fn port_set(ports: &[u16]) -> String {
    let parts: Vec<String> = ports.iter().map(|p| p.to_string()).collect();
    format!("{{ {} }}", parts.join(", "))
}

#[cfg(target_os = "linux")]
fn sanitize_table_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(target_os = "linux")]
async fn run_nft(script: &str) -> Result<(), FirewallError> {
    use tokio::process::Command;

    let mut child = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(FirewallError::Nft)?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    // Write script to stdin.
    tokio::io::AsyncWriteExt::write_all(&mut stdin, script.as_bytes())
        .await
        .map_err(FirewallError::Nft)?;
    // Close stdin to signal EOF.
    drop(stdin);

    let output = child.wait_with_output().await.map_err(FirewallError::Nft)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(FirewallError::NftRejected(stderr));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_profile_has_expected_blocks() {
        let p = FirewallProfile::public("lanlink0");
        assert!(p.enabled);
        assert!(p.block_tcp_ports.contains(&445));
        assert!(p.block_tcp_ports.contains(&22));
        assert!(p.block_udp_ports.contains(&137));
    }

    #[test]
    fn disabled_profile_is_noop() {
        let p = FirewallProfile::disabled("lanlink0");
        assert!(!p.enabled);
        assert!(p.block_tcp_ports.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_script_contains_interface_and_ports() {
        let p = FirewallProfile::public("lanlink0");
        let script = p.build_script();
        assert!(script.contains("lanlink0"));
        assert!(script.contains("tcp dport"));
        assert!(script.contains("udp dport"));
        assert!(script.contains("icmp type echo-request accept"));
    }
}
