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
    #[error("windows firewall command failed: {0}")]
    WinCommand(io::Error),
    #[error("windows firewall rejected the rules: {0}")]
    WinRejected(String),
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

    #[cfg(target_os = "windows")]
    pub async fn apply(&self) -> Result<(), FirewallError> {
        if !self.enabled {
            return self.clear().await;
        }
        // Remove rules from a previous (possibly crashed) run first so
        // re-application never duplicates rules; then install fresh ones.
        let script = windows::build_apply_script(self);
        windows::run_powershell(&script).await
    }

    #[cfg(target_os = "windows")]
    pub async fn clear(&self) -> Result<(), FirewallError> {
        let script = windows::build_clear_script(&self.interface);
        windows::run_powershell(&script).await
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub async fn apply(&self) -> Result<(), FirewallError> {
        Err(FirewallError::Unsupported)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
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

/// Windows implementation of the inbound firewall profile.
///
/// Uses the WFP-backed `NetSecurity` cmdlets (PowerShell): `New-NetFirewallRule`
/// creates per-interface block/allow rules grouped under one rule Group so that
/// `Remove-NetFirewallRule -Group` can tear everything down atomically. This
/// mirrors the nftables semantics: rules only evaluate traffic arriving on the
/// virtual interface; the Windows firewall is stateful, so established/related
/// return traffic is allowed automatically.
#[cfg(any(target_os = "windows", test))]
pub mod windows {
    #[cfg(target_os = "windows")]
    use super::FirewallError;
    use super::FirewallProfile;

    /// NetFirewall rule group used for all rules of one interface. Alphanumeric
    /// only, since the group name is embedded in PowerShell single-quoted
    /// strings and used as a -Group filter.
    pub fn rule_group(interface: &str) -> String {
        let sanitized: String = interface
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        format!("LanLink_{sanitized}")
    }

    /// Escape a value for use inside a PowerShell single-quoted string (double
    /// any single quote).
    fn ps_quote(value: &str) -> String {
        value.replace('\'', "''")
    }

    /// Build the PowerShell script that (re)installs the profile's rules.
    pub fn build_apply_script(profile: &FirewallProfile) -> String {
        let group = rule_group(&profile.interface);
        let iface = ps_quote(&profile.interface);
        let mut script = String::from("$ErrorActionPreference = 'Stop'\n");
        // Replace any previous rules for this interface (crash recovery /
        // re-application), like the nftables `delete table` on Linux.
        script.push_str(&format!(
            "Remove-NetFirewallRule -Group '{group}' -ErrorAction SilentlyContinue\n"
        ));
        for &port in &profile.block_tcp_ports {
            script.push_str(&format!(
                "New-NetFirewallRule -DisplayName 'LanLink: block TCP {port} inbound on {iface}' \
                 -Group '{group}' -Direction Inbound -Action Block -Protocol TCP -LocalPort {port} \
                 -InterfaceAlias '{iface}' -Profile Any | Out-Null\n"
            ));
        }
        for &port in &profile.block_udp_ports {
            script.push_str(&format!(
                "New-NetFirewallRule -DisplayName 'LanLink: block UDP {port} inbound on {iface}' \
                 -Group '{group}' -Direction Inbound -Action Block -Protocol UDP -LocalPort {port} \
                 -InterfaceAlias '{iface}' -Profile Any | Out-Null\n"
            ));
        }
        if profile.allow_ping {
            script.push_str(&format!(
                "New-NetFirewallRule -DisplayName 'LanLink: allow ping inbound on {iface}' \
                 -Group '{group}' -Direction Inbound -Action Allow -Protocol ICMPv4 -IcmpType 8 \
                 -InterfaceAlias '{iface}' -Profile Any | Out-Null\n"
            ));
        }
        script
    }

    /// Build the PowerShell script that removes all rules for an interface.
    pub fn build_clear_script(interface: &str) -> String {
        let group = rule_group(interface);
        format!(
            "$ErrorActionPreference = 'Stop'\n\
             Remove-NetFirewallRule -Group '{group}' -ErrorAction SilentlyContinue\n"
        )
    }

    /// Run a PowerShell script with the NetSecurity module available.
    #[cfg(target_os = "windows")]
    pub(crate) async fn run_powershell(script: &str) -> Result<(), FirewallError> {
        use tokio::process::Command;

        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .output()
            .await
            .map_err(FirewallError::WinCommand)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(FirewallError::WinRejected(stderr));
        }
        Ok(())
    }
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

    #[cfg(any(target_os = "windows", test))]
    mod windows_tests {
        use super::*;

        #[test]
        fn rule_group_sanitizes_interface_name() {
            assert_eq!(windows::rule_group("lanlink0"), "LanLink_lanlink0");
            assert_eq!(windows::rule_group("we'ird name"), "LanLink_we_ird_name");
        }

        #[test]
        fn apply_script_contains_block_and_allow_rules() {
            let p = FirewallProfile::public("lanlink0");
            let script = windows::build_apply_script(&p);
            // Idempotent re-application: previous group removed first.
            assert!(script.contains("Remove-NetFirewallRule -Group 'LanLink_lanlink0'"));
            // One block rule per configured port on the interface.
            for port in [139, 445, 3389, 22] {
                assert!(script.contains(&format!("-Protocol TCP -LocalPort {port}")));
            }
            for port in [137, 138] {
                assert!(script.contains(&format!("-Protocol UDP -LocalPort {port}")));
            }
            assert!(script.contains("-Action Block"));
            assert!(script.contains("-InterfaceAlias 'lanlink0'"));
            // Ping (ICMPv4 echo request) is allowed through.
            assert!(script.contains("-Protocol ICMPv4 -IcmpType 8"));
            assert!(script.contains("-Action Allow"));
        }

        #[test]
        fn apply_script_escapes_interface_names() {
            let p = FirewallProfile::public("my adapt'er");
            let script = windows::build_apply_script(&p);
            assert!(script.contains("-InterfaceAlias 'my adapt''er'"));
        }

        #[test]
        fn clear_script_removes_rule_group() {
            let script = windows::build_clear_script("lanlink0");
            assert!(script.contains("Remove-NetFirewallRule -Group 'LanLink_lanlink0'"));
        }
    }
}
