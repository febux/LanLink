use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::rngs::OsRng;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::net::Ipv4Addr;
use std::str::FromStr;

pub async fn connect(path: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(16)
        .connect_with(opts)
        .await?;
    ensure_schema(&pool).await?;
    Ok(pool)
}

pub async fn connect_test() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new().in_memory(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    ensure_schema(&pool).await?;
    Ok(pool)
}

async fn ensure_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS networks (
            id INTEGER PRIMARY KEY,
            name TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            subnet TEXT NOT NULL,
            relay_servers TEXT NOT NULL DEFAULT '[]',
            is_public INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS members (
            id INTEGER PRIMARY KEY,
            network_id INTEGER NOT NULL,
            node_id BLOB NOT NULL,
            public_key TEXT NOT NULL,
            virtual_ip TEXT NOT NULL,
            direct_candidates TEXT NOT NULL DEFAULT '[]',
            relay_candidates TEXT NOT NULL DEFAULT '[]',
            is_gateway INTEGER NOT NULL DEFAULT 0,
            shared_subnet TEXT NOT NULL DEFAULT '',
            last_seen INTEGER NOT NULL,
            UNIQUE(network_id, node_id)
        );

        CREATE INDEX IF NOT EXISTS idx_members_network ON members(network_id);

        CREATE TABLE IF NOT EXISTS telemetry (
            id INTEGER PRIMARY KEY,
            received_at INTEGER NOT NULL,
            version TEXT NOT NULL,
            platform TEXT NOT NULL,
            uptime_seconds INTEGER NOT NULL,
            peer_count INTEGER NOT NULL,
            direct_peer_count INTEGER NOT NULL,
            relay_peer_count INTEGER NOT NULL,
            gateway_enabled INTEGER NOT NULL,
            firewall_enabled INTEGER NOT NULL,
            tx_bytes INTEGER NOT NULL,
            rx_bytes INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_telemetry_received ON telemetry(received_at);
        "#,
    )
    .execute(pool)
    .await?;

    // Migration: add relay columns to databases created before M4/M5.
    let _ = sqlx::query("ALTER TABLE networks ADD COLUMN relay_servers TEXT NOT NULL DEFAULT '[]'")
        .execute(pool)
        .await;
    let _ =
        sqlx::query("ALTER TABLE members ADD COLUMN relay_candidates TEXT NOT NULL DEFAULT '[]'")
            .execute(pool)
            .await;
    // Migration: add gateway/firewall columns for M6.
    let _ = sqlx::query("ALTER TABLE networks ADD COLUMN is_public INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE members ADD COLUMN is_gateway INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE members ADD COLUMN shared_subnet TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await;

    Ok(())
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e:?}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let argon2 = Argon2::default();
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    argon2.verify_password(password.as_bytes(), &parsed).is_ok()
}

pub async fn create_network(
    pool: &SqlitePool,
    name: &str,
    password_hash: &str,
    relay_servers: &str,
    is_public: bool,
) -> anyhow::Result<i64> {
    let subnet = "26.0.0.0/24".to_string(); // M3: single default subnet
    let id = sqlx::query(
        "INSERT INTO networks (name, password_hash, subnet, relay_servers, is_public) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(name)
    .bind(password_hash)
    .bind(subnet)
    .bind(relay_servers)
    .bind(if is_public { 1 } else { 0 })
    .execute(pool)
    .await?
    .last_insert_rowid();

    // Use a deterministic subnet based on the assigned id.
    let a = ((id >> 8) & 0xff) as u8;
    let b = (id & 0xff) as u8;
    let subnet = format!("26.{a}.{b}.0/24");
    sqlx::query("UPDATE networks SET subnet = ? WHERE id = ?")
        .bind(subnet)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(id)
}

pub async fn get_network_password_hash(
    pool: &SqlitePool,
    network_id: i64,
) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT password_hash FROM networks WHERE id = ?")
        .bind(network_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0))
}

pub async fn get_network_subnet(
    pool: &SqlitePool,
    network_id: i64,
) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT subnet FROM networks WHERE id = ?")
        .bind(network_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0))
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_member(
    pool: &SqlitePool,
    network_id: i64,
    node_id: &[u8; 8],
    public_key: &str,
    direct_candidates: &str,
    relay_candidates: &str,
    is_gateway: bool,
    shared_subnet: &str,
) -> anyhow::Result<Ipv4Addr> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // If member already exists, just update candidates and last_seen and return its IP.
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT virtual_ip FROM members WHERE network_id = ? AND node_id = ?")
            .bind(network_id)
            .bind(node_id.as_slice())
            .fetch_optional(pool)
            .await?;

    if let Some(ip) = existing {
        let ip = Ipv4Addr::from_str(&ip.0)?;
        sqlx::query(
            "UPDATE members SET direct_candidates = ?, relay_candidates = ?, is_gateway = ?, shared_subnet = ?, last_seen = ? WHERE network_id = ? AND node_id = ?",
        )
        .bind(direct_candidates)
        .bind(relay_candidates)
        .bind(if is_gateway { 1 } else { 0 })
        .bind(shared_subnet)
        .bind(now)
        .bind(network_id)
        .bind(node_id.as_slice())
        .execute(pool)
        .await?;
        return Ok(ip);
    }

    let subnet = match get_network_subnet(pool, network_id).await? {
        Some(s) => s,
        None => anyhow::bail!("network not found"),
    };
    let ip = assign_ip(pool, network_id, &subnet).await?;

    sqlx::query(
        "INSERT INTO members (network_id, node_id, public_key, virtual_ip, direct_candidates, relay_candidates, is_gateway, shared_subnet, last_seen)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(network_id)
    .bind(node_id.as_slice())
    .bind(public_key)
    .bind(ip.to_string())
    .bind(direct_candidates)
    .bind(relay_candidates)
    .bind(if is_gateway { 1 } else { 0 })
    .bind(shared_subnet)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(ip)
}

async fn assign_ip(pool: &SqlitePool, network_id: i64, subnet: &str) -> anyhow::Result<Ipv4Addr> {
    let parts: Vec<&str> = subnet.split('/').collect();
    if parts.len() != 2 {
        anyhow::bail!("invalid subnet format");
    }
    let base = Ipv4Addr::from_str(parts[0])?;
    let prefix: u8 = parts[1].parse()?;
    if prefix != 24 {
        anyhow::bail!("only /24 subnets supported in M3");
    }

    let base_octets = base.octets();
    let mut used = std::collections::HashSet::new();
    used.insert(0);
    used.insert(255);
    used.insert(1); // reserve .1 for coordinator/gateway semantics

    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT virtual_ip FROM members WHERE network_id = ?")
            .bind(network_id)
            .fetch_all(pool)
            .await?;

    for row in rows {
        let ip = Ipv4Addr::from_str(&row.0)?;
        used.insert(ip.octets()[3]);
    }

    for host in 2u8..=254 {
        if !used.contains(&host) {
            return Ok(Ipv4Addr::new(
                base_octets[0],
                base_octets[1],
                base_octets[2],
                host,
            ));
        }
    }

    anyhow::bail!("no free IP in subnet")
}

pub async fn list_peers(
    pool: &SqlitePool,
    network_id: i64,
    exclude_node_id: &[u8; 8],
) -> anyhow::Result<Vec<lanlink_proto::PeerInfo>> {
    let rows: Vec<(Vec<u8>, String, String, String, i64, String)> = sqlx::query_as(
        "SELECT node_id, virtual_ip, direct_candidates, relay_candidates, is_gateway, shared_subnet FROM members
         WHERE network_id = ? AND node_id != ?",
    )
    .bind(network_id)
    .bind(exclude_node_id.as_slice())
    .fetch_all(pool)
    .await?;

    let mut peers = Vec::new();
    for (node_id_blob, ip_str, direct_json, relay_json, is_gateway, shared_subnet) in rows {
        if node_id_blob.len() != 8 {
            continue;
        }
        let mut node_id = [0u8; 8];
        node_id.copy_from_slice(&node_id_blob);
        let direct_candidates: Vec<std::net::SocketAddr> =
            serde_json::from_str(&direct_json).unwrap_or_default();
        let relay_candidates: Vec<std::net::SocketAddr> =
            serde_json::from_str(&relay_json).unwrap_or_default();
        peers.push(lanlink_proto::PeerInfo {
            node_id: lanlink_proto::NodeId(node_id),
            virtual_ip: lanlink_proto::VirtualIp(Ipv4Addr::from_str(&ip_str)?),
            direct_candidates,
            relay_candidates,
            is_gateway: is_gateway != 0,
            shared_subnet,
        });
    }
    Ok(peers)
}

pub async fn update_candidates(
    pool: &SqlitePool,
    network_id: i64,
    node_id: &[u8; 8],
    direct_candidates: &str,
    relay_candidates: &str,
) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    sqlx::query(
        "UPDATE members SET direct_candidates = ?, relay_candidates = ?, last_seen = ? WHERE network_id = ? AND node_id = ?",
    )
    .bind(direct_candidates)
    .bind(relay_candidates)
    .bind(now)
    .bind(network_id)
    .bind(node_id.as_slice())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_network_relay_servers(
    pool: &SqlitePool,
    network_id: i64,
) -> anyhow::Result<Vec<std::net::SocketAddr>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT relay_servers FROM networks WHERE id = ?")
        .bind(network_id)
        .fetch_optional(pool)
        .await?;
    let json = row.map(|r| r.0).unwrap_or_else(|| "[]".to_string());
    Ok(serde_json::from_str(&json).unwrap_or_default())
}

pub async fn get_network_is_public(pool: &SqlitePool, network_id: i64) -> anyhow::Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT is_public FROM networks WHERE id = ?")
        .bind(network_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0 != 0).unwrap_or(false))
}

pub async fn network_exists(pool: &SqlitePool, network_id: i64) -> anyhow::Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM networks WHERE id = ?")
        .bind(network_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Look up a network by name. Returns `(id, subnet, password_hash, is_public)`
/// if the name exists.
pub async fn get_network_by_name(
    pool: &SqlitePool,
    name: &str,
) -> anyhow::Result<Option<(i64, String, String, i64)>> {
    let row: Option<(i64, String, String, i64)> =
        sqlx::query_as("SELECT id, subnet, password_hash, is_public FROM networks WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

/// List public networks for the directory server.  Counts members seen in the
/// last 5 minutes as "online".
pub async fn list_public_networks(
    pool: &SqlitePool,
) -> anyhow::Result<Vec<lanlink_proto::PublicNetworkInfo>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let cutoff = now - 5 * 60;

    let rows: Vec<(i64, String, String, i64, i64)> = sqlx::query_as(
        r#"
        SELECT n.id, n.name, n.subnet, n.is_public,
               COALESCE((SELECT COUNT(*) FROM members m WHERE m.network_id = n.id AND m.last_seen >= ?), 0) as member_count
        FROM networks n
        WHERE n.is_public = 1
        ORDER BY member_count DESC, n.name ASC
        "#,
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for (id, name, subnet, is_public, member_count) in rows {
        out.push(lanlink_proto::PublicNetworkInfo {
            network_id: lanlink_proto::NetworkId(id as u64),
            name,
            subnet,
            member_count: member_count as u64,
            is_public: is_public != 0,
        });
    }
    Ok(out)
}

pub async fn record_telemetry(
    pool: &SqlitePool,
    report: &lanlink_proto::telemetry::TelemetryReport,
) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    sqlx::query(
        "INSERT INTO telemetry
         (received_at, version, platform, uptime_seconds, peer_count, direct_peer_count,
          relay_peer_count, gateway_enabled, firewall_enabled, tx_bytes, rx_bytes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(now)
    .bind(&report.version)
    .bind(&report.platform)
    .bind(report.uptime_seconds as i64)
    .bind(report.peer_count as i64)
    .bind(report.direct_peer_count as i64)
    .bind(report.relay_peer_count as i64)
    .bind(if report.gateway_enabled { 1 } else { 0 })
    .bind(if report.firewall_enabled { 1 } else { 0 })
    .bind(report.tx_bytes as i64)
    .bind(report.rx_bytes as i64)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn aggregate_metrics(
    pool: &SqlitePool,
    window_seconds: i64,
) -> anyhow::Result<lanlink_proto::telemetry::AggregatedMetrics> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let start = now - window_seconds;

    let row: (i64, f64, f64, f64, f64, f64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*),
            AVG(peer_count),
            AVG(direct_peer_count),
            AVG(relay_peer_count),
            AVG(gateway_enabled),
            AVG(firewall_enabled),
            SUM(tx_bytes),
            SUM(rx_bytes)
        FROM telemetry
        WHERE received_at >= ?
        "#,
    )
    .bind(start)
    .fetch_one(pool)
    .await?;

    let versions: Vec<(String, i64)> = sqlx::query_as(
        "SELECT version, COUNT(*) FROM telemetry WHERE received_at >= ? GROUP BY version ORDER BY COUNT(*) DESC",
    )
    .bind(start)
    .fetch_all(pool)
    .await?;

    let platforms: Vec<(String, i64)> = sqlx::query_as(
        "SELECT platform, COUNT(*) FROM telemetry WHERE received_at >= ? GROUP BY platform ORDER BY COUNT(*) DESC",
    )
    .bind(start)
    .fetch_all(pool)
    .await?;

    Ok(lanlink_proto::telemetry::AggregatedMetrics {
        window_start: start,
        report_count: row.0 as u64,
        avg_peer_count: row.1,
        avg_direct_peers: row.2,
        avg_relay_peers: row.3,
        gateway_fraction: row.4,
        firewall_fraction: row.5,
        total_tx_bytes: row.6 as u64,
        total_rx_bytes: row.7 as u64,
        versions: versions
            .into_iter()
            .map(|(v, c)| lanlink_proto::telemetry::VersionCount {
                version: v,
                count: c as u64,
            })
            .collect(),
        platforms: platforms
            .into_iter()
            .map(|(p, c)| lanlink_proto::telemetry::PlatformCount {
                platform: p,
                count: c as u64,
            })
            .collect(),
    })
}
