//! Gateway mode for LanLink.
//!
//! A gateway node shares its physical LAN with the virtual network. This crate
//! implements the Linux plumbing: proxy-ARP, ip_forward and nftables MASQUERADE,
//! plus automatic route installation on non-gateway peers.

use std::io;
use std::net::Ipv4Addr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("platform command failed: {0}")]
    Command(io::Error),
    #[error("command rejected: {0}")]
    CommandRejected(String),
    #[error("invalid subnet: {0}")]
    InvalidSubnet(String),
    #[error("gateway mode not supported on this platform")]
    Unsupported,
}

/// Configuration for a node acting as a gateway.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub enabled: bool,
    /// Subnet behind this node (e.g. "192.168.1.0/24").
    pub shared_subnet: String,
    /// This node's virtual IP inside the LanLink network.
    pub virtual_ip: Ipv4Addr,
    /// Virtual TAP interface name.
    pub interface: String,
    /// Physical interface used to reach the shared subnet.
    pub phys_interface: String,
}

/// State maintained while gateway mode is active.
pub struct GatewayManager {
    config: GatewayConfig,
    mac: [u8; 6],
}

impl GatewayManager {
    /// Create a manager from config. If not enabled, all methods are no-ops.
    pub fn new(config: GatewayConfig) -> Self {
        let mac = mac_from_virtual_ip(config.virtual_ip);
        Self { config, mac }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Enable kernel forwarding and install the MASQUERADE rule. Idempotent.
    #[cfg(target_os = "linux")]
    pub async fn enable(&self) -> Result<(), GatewayError> {
        if !self.config.enabled {
            return Ok(());
        }
        enable_ip_forward().await?;
        install_masquerade(
            &self.config.interface,
            &self.config.phys_interface,
            &self.config.shared_subnet,
        )
        .await?;
        tracing::info!(
            subnet = %self.config.shared_subnet,
            phys = %self.config.phys_interface,
            "gateway mode enabled"
        );
        Ok(())
    }

    /// Remove the MASQUERADE rule. (We leave ip_forward untouched because it
    /// may be required by other software.)
    #[cfg(target_os = "linux")]
    pub async fn disable(&self) -> Result<(), GatewayError> {
        let table = masq_table_name(&self.config.interface);
        let script = format!("delete table ip {}\n", table);
        match run_nft(&script).await {
            Ok(()) => Ok(()),
            Err(GatewayError::CommandRejected(ref stderr))
                if stderr.contains("No such file") || stderr.contains("does not exist") =>
            {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn enable(&self) -> Result<(), GatewayError> {
        if self.config.enabled {
            Err(GatewayError::Unsupported)
        } else {
            Ok(())
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn disable(&self) -> Result<(), GatewayError> {
        Ok(())
    }

    /// Inspect an Ethernet frame arriving from a peer (before writing it to the
    /// TAP). If it is an ARP request for an IPv4 address inside the shared
    /// subnet, consume it and return an ARP reply frame to be written to the TAP.
    /// Otherwise return the original frame unchanged.
    pub fn process_inbound_frame(&self, frame: &[u8]) -> Option<Vec<u8>> {
        if !self.config.enabled {
            return Some(frame.to_vec());
        }
        match self.proxy_arp_reply(frame) {
            Some(reply) => Some(reply),
            None => Some(frame.to_vec()),
        }
    }

    /// Generate a proxy-ARP reply if the frame is an ARP request for an address
    /// inside the shared subnet.
    fn proxy_arp_reply(&self, frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < 28 + 14 {
            return None;
        }
        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        if ethertype != 0x0806 {
            return None;
        }
        let arp = &frame[14..];
        if arp.len() < 28 {
            return None;
        }
        let hw_type = u16::from_be_bytes([arp[0], arp[1]]);
        let proto_type = u16::from_be_bytes([arp[2], arp[3]]);
        let hw_len = arp[4];
        let proto_len = arp[5];
        let opcode = u16::from_be_bytes([arp[6], arp[7]]);
        if hw_type != 1 || proto_type != 0x0800 || hw_len != 6 || proto_len != 4 || opcode != 1 {
            return None;
        }
        let target_ip = Ipv4Addr::new(arp[24], arp[25], arp[26], arp[27]);
        if !ip_in_subnet(target_ip, &self.config.shared_subnet).unwrap_or(false) {
            return None;
        }
        let sender_mac = [arp[8], arp[9], arp[10], arp[11], arp[12], arp[13]];
        let sender_ip = Ipv4Addr::new(arp[14], arp[15], arp[16], arp[17]);
        Some(build_arp_reply(self.mac, target_ip, sender_mac, sender_ip))
    }
}

/// Build an ARP reply Ethernet frame.
fn build_arp_reply(
    sender_mac: [u8; 6],
    sender_ip: Ipv4Addr,
    target_mac: [u8; 6],
    target_ip: Ipv4Addr,
) -> Vec<u8> {
    let mut frame = Vec::with_capacity(42);
    // Ethernet header.
    frame.extend_from_slice(&target_mac); // destination
    frame.extend_from_slice(&sender_mac); // source
    frame.extend_from_slice(&0x0806u16.to_be_bytes()); // ARP

    // ARP payload.
    frame.extend_from_slice(&1u16.to_be_bytes()); // HW type Ethernet
    frame.extend_from_slice(&0x0800u16.to_be_bytes()); // proto IPv4
    frame.push(6); // HW len
    frame.push(4); // proto len
    frame.extend_from_slice(&2u16.to_be_bytes()); // reply
    frame.extend_from_slice(&sender_mac);
    frame.extend_from_slice(&sender_ip.octets());
    frame.extend_from_slice(&target_mac);
    frame.extend_from_slice(&target_ip.octets());
    frame
}

fn mac_from_virtual_ip(ip: Ipv4Addr) -> [u8; 6] {
    let o = ip.octets();
    [0x02, 0x26, o[0], o[1], o[2], o[3]]
}

fn ip_in_subnet(addr: Ipv4Addr, subnet: &str) -> Result<bool, GatewayError> {
    let (net, prefix) = parse_subnet(subnet)?;
    let mask = if prefix == 0 {
        0u32
    } else {
        (!0u32) << (32 - prefix)
    };
    Ok((u32::from(addr) & mask) == (u32::from(net) & mask))
}

fn parse_subnet(subnet: &str) -> Result<(Ipv4Addr, u8), GatewayError> {
    let parts: Vec<&str> = subnet.split('/').collect();
    if parts.len() != 2 {
        return Err(GatewayError::InvalidSubnet(subnet.to_string()));
    }
    let net: Ipv4Addr = parts[0]
        .parse()
        .map_err(|_| GatewayError::InvalidSubnet(subnet.to_string()))?;
    let prefix: u8 = parts[1]
        .parse()
        .map_err(|_| GatewayError::InvalidSubnet(subnet.to_string()))?;
    if prefix > 32 {
        return Err(GatewayError::InvalidSubnet(subnet.to_string()));
    }
    Ok((net, prefix))
}

#[cfg(target_os = "linux")]
async fn enable_ip_forward() -> Result<(), GatewayError> {
    use tokio::fs;
    fs::write("/proc/sys/net/ipv4/ip_forward", "1")
        .await
        .map_err(GatewayError::Command)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn masq_table_name(interface: &str) -> String {
    interface
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(target_os = "linux")]
async fn install_masquerade(
    interface: &str,
    phys_interface: &str,
    shared_subnet: &str,
) -> Result<(), GatewayError> {
    let table = masq_table_name(interface);
    // Delete any previous table first (e.g. left over from a crashed run) so
    // re-applying never duplicates the MASQUERADE rule.
    let delete = format!("delete table ip {table}\n");
    match run_nft(&delete).await {
        Ok(()) => {}
        Err(GatewayError::CommandRejected(ref stderr))
            if stderr.contains("No such file") || stderr.contains("does not exist") => {}
        Err(e) => return Err(e),
    }
    let script = format!(
        "table ip {table} {{
    chain postrouting {{
        type nat hook postrouting priority 100; policy accept;
        oifname \"{phys_interface}\" ip saddr {shared_subnet} masquerade
    }}
}}\n",
    );
    run_nft(&script).await
}

#[cfg(target_os = "linux")]
async fn run_nft(script: &str) -> Result<(), GatewayError> {
    use tokio::process::Command;

    let mut child = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(GatewayError::Command)?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    tokio::io::AsyncWriteExt::write_all(&mut stdin, script.as_bytes())
        .await
        .map_err(GatewayError::Command)?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .map_err(GatewayError::Command)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GatewayError::CommandRejected(stderr));
    }
    Ok(())
}

/// Manage routes on a non-gateway peer so that traffic to a gateway-shared
/// subnet is routed through the gateway's virtual IP.
///
/// The manager is stateful: `sync_routes` adds newly-desired routes and
/// removes previously-installed ones that are no longer wanted, so a clean
/// shutdown leaves no stale routes behind.
pub struct RouteManager {
    interface: String,
    current: std::sync::Mutex<Vec<(Ipv4Addr, String)>>,
}

impl RouteManager {
    pub fn new(interface: impl Into<String>) -> Self {
        Self {
            interface: interface.into(),
            current: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Apply routes for all gateway peers, removing routes that were
    /// previously installed but are no longer in the desired set.
    pub async fn sync_routes(&self, gateways: &[(Ipv4Addr, String)]) -> Result<(), GatewayError> {
        let stale: Vec<String> = {
            let mut current = self.current.lock().unwrap();
            let stale = current
                .iter()
                .filter(|r| !gateways.contains(r))
                .map(|(_, subnet)| subnet.clone())
                .collect();
            current.retain(|r| gateways.contains(r));
            stale
        };

        for subnet in &stale {
            #[cfg(target_os = "linux")]
            if let Err(e) = del_route(subnet).await {
                tracing::warn!(%subnet, error = %e, "failed to remove stale route");
            }
            #[cfg(not(target_os = "linux"))]
            let _ = subnet;
        }

        for (via, subnet) in gateways {
            #[cfg(target_os = "linux")]
            if let Err(e) = add_or_replace_route(subnet, *via, &self.interface).await {
                tracing::warn!(%subnet, %via, error = %e, "failed to install route");
            }
            #[cfg(not(target_os = "linux"))]
            let _ = (via, subnet);
        }

        *self.current.lock().unwrap() = gateways.to_vec();
        Ok(())
    }

    /// Remove routes left behind by a previous (crashed) run of the daemon:
    /// every route on the interface that was not installed by the kernel
    /// (i.e. lacks `proto kernel` / `scope link`). Call once at startup,
    /// before the first `sync_routes`.
    pub async fn flush_stale(&self) -> Result<(), GatewayError> {
        #[cfg(target_os = "linux")]
        {
            use tokio::process::Command;
            let output = Command::new("ip")
                .args(["route", "show", "dev", &self.interface])
                .output()
                .await
                .map_err(GatewayError::Command)?;
            if !output.status.success() {
                // Interface may not exist yet — nothing to flush.
                return Ok(());
            }
            let text = String::from_utf8_lossy(&output.stdout);
            for dst in parse_managed_routes(&text) {
                if let Err(e) = del_route(&dst).await {
                    tracing::warn!(%dst, error = %e, "failed to flush stale route");
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {}
        Ok(())
    }
}

/// Extract route destinations that were not installed by the kernel from
/// `ip route show dev <iface>` output.
#[cfg(target_os = "linux")]
fn parse_managed_routes(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !l.contains("proto kernel") && !l.contains("scope link"))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

#[cfg(target_os = "linux")]
async fn del_route(subnet: &str) -> Result<(), GatewayError> {
    use tokio::process::Command;

    let output = Command::new("ip")
        .args(["route", "del", subnet])
        .output()
        .await
        .map_err(GatewayError::Command)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GatewayError::CommandRejected(stderr));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
async fn add_or_replace_route(
    subnet: &str,
    via: Ipv4Addr,
    interface: &str,
) -> Result<(), GatewayError> {
    use tokio::process::Command;

    // Try to delete an existing route first (ignore failure).
    let _ = del_route(subnet).await;

    let output = Command::new("ip")
        .args([
            "route",
            "add",
            subnet,
            "via",
            &via.to_string(),
            "dev",
            interface,
        ])
        .output()
        .await
        .map_err(GatewayError::Command)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GatewayError::CommandRejected(stderr));
    }
    Ok(())
}

/// Convenience: build the list of gateway peers from a coordinator peer list.
pub fn gateway_routes(peers: &[lanlink_proto::PeerInfo]) -> Vec<(Ipv4Addr, String)> {
    peers
        .iter()
        .filter(|p| p.is_gateway && !p.shared_subnet.is_empty())
        .map(|p| (p.virtual_ip.0, p.shared_subnet.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_in_subnet_works() {
        assert!(ip_in_subnet("192.168.1.5".parse().unwrap(), "192.168.1.0/24").unwrap());
        assert!(!ip_in_subnet("192.168.2.5".parse().unwrap(), "192.168.1.0/24").unwrap());
    }

    #[test]
    fn arp_request_for_shared_subnet_generates_reply() {
        let cfg = GatewayConfig {
            enabled: true,
            shared_subnet: "192.168.1.0/24".to_string(),
            virtual_ip: "26.0.0.2".parse().unwrap(),
            interface: "lanlink0".to_string(),
            phys_interface: "eth0".to_string(),
        };
        let mgr = GatewayManager::new(cfg);

        // Build ARP request frame.
        let mut frame = Vec::with_capacity(42);
        frame.extend_from_slice(&[0xff; 6]); // broadcast dst
        frame.extend_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x01]); // src
        frame.extend_from_slice(&0x0806u16.to_be_bytes()); // ARP
        frame.extend_from_slice(&1u16.to_be_bytes()); // eth
        frame.extend_from_slice(&0x0800u16.to_be_bytes()); // ipv4
        frame.push(6);
        frame.push(4);
        frame.extend_from_slice(&1u16.to_be_bytes()); // request
        frame.extend_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x01]); // sender mac
        frame.extend_from_slice(&"26.0.0.1".parse::<Ipv4Addr>().unwrap().octets()); // sender ip
        frame.extend_from_slice(&[0x00; 6]); // target mac (unknown)
        frame.extend_from_slice(&"192.168.1.5".parse::<Ipv4Addr>().unwrap().octets()); // target ip

        let reply = mgr.process_inbound_frame(&frame).unwrap();
        assert!(reply.len() >= 42);
        assert_eq!(u16::from_be_bytes([reply[12], reply[13]]), 0x0806);
        let arp = &reply[14..];
        assert_eq!(u16::from_be_bytes([arp[6], arp[7]]), 2); // reply
        assert_eq!(
            arp[14..18],
            "192.168.1.5".parse::<Ipv4Addr>().unwrap().octets()
        );
    }

    #[test]
    fn non_arp_frame_passes_through() {
        let cfg = GatewayConfig {
            enabled: true,
            shared_subnet: "192.168.1.0/24".to_string(),
            virtual_ip: "26.0.0.2".parse().unwrap(),
            interface: "lanlink0".to_string(),
            phys_interface: "eth0".to_string(),
        };
        let mgr = GatewayManager::new(cfg);
        let frame = vec![
            0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x02, 0x08, 0x00,
            0x45,
        ];
        let out = mgr.process_inbound_frame(&frame).unwrap();
        assert_eq!(out, frame);
    }

    #[test]
    fn gateway_routes_excludes_non_gateways() {
        let peers = vec![
            lanlink_proto::PeerInfo {
                node_id: lanlink_proto::NodeId([1; 8]),
                virtual_ip: lanlink_proto::VirtualIp("26.0.0.2".parse().unwrap()),
                public_key: vec![],
                direct_candidates: vec![],
                relay_candidates: vec![],
                tcp_candidates: vec![],
                tcp_relay_candidates: vec![],
                is_gateway: true,
                shared_subnet: "10.0.0.0/24".to_string(),
            },
            lanlink_proto::PeerInfo {
                node_id: lanlink_proto::NodeId([2; 8]),
                virtual_ip: lanlink_proto::VirtualIp("26.0.0.3".parse().unwrap()),
                public_key: vec![],
                direct_candidates: vec![],
                relay_candidates: vec![],
                tcp_candidates: vec![],
                tcp_relay_candidates: vec![],
                is_gateway: false,
                shared_subnet: String::new(),
            },
        ];
        let routes = gateway_routes(&peers);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].0, "26.0.0.2".parse::<Ipv4Addr>().unwrap());
        assert_eq!(routes[0].1, "10.0.0.0/24");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_managed_routes_skips_kernel_routes() {
        let output = "\
26.0.0.0/24 proto kernel scope link src 26.0.0.5
10.1.0.0/16 via 26.0.0.2 dev lanlink0
192.168.4.0/24 via 26.0.0.9 dev lanlink0
";
        let managed = parse_managed_routes(output);
        assert_eq!(managed, vec!["10.1.0.0/16", "192.168.4.0/24"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_managed_routes_ignores_link_scope() {
        let output = "fe80::/64 proto kernel metric 256 pref medium\n26.5.0.0/24 via 26.0.0.3 dev lanlink0\n";
        let managed = parse_managed_routes(output);
        assert_eq!(managed, vec!["26.5.0.0/24"]);
    }
}
