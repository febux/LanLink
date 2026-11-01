use integration_tests::wait_for_tcp;
use std::net::SocketAddr;

async fn start_test_server() -> (SocketAddr, reqwest::Client) {
    let pool = coordinator::db::connect_test().await.expect("test db");
    let app = coordinator::app(pool, vec![], vec![]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_tcp(addr).await.expect("coordinator did not start");
    (addr, reqwest::Client::new())
}

#[tokio::test]
async fn coordinator_create_join_list_members() {
    let (addr, client) = start_test_server().await;
    let base = format!("http://{addr}");

    let create: lanlink_proto::CreateNetworkResponse = client
        .post(format!("{base}/networks"))
        .json(&lanlink_proto::CreateNetworkRequest {
            name: "coord-api-test".to_string(),
            password: "secret".into(),
            is_public: false,
        })
        .send()
        .await
        .expect("create")
        .json()
        .await
        .expect("create json");
    assert!(create.network_id.0 > 0);
    let network_id = create.network_id.0;

    let join: lanlink_proto::JoinNetworkResponse = client
        .post(format!("{base}/networks/{network_id}/join"))
        .json(&lanlink_proto::JoinNetworkRequest {
            password: "secret".into(),
            node_id: lanlink_types::NodeId([1u8; 8]),
            public_key: vec![],
            direct_candidates: vec!["127.0.0.1:9001".parse().unwrap()],
            relay_candidates: vec![],
            tcp_candidates: vec![],
            tcp_relay_candidates: vec![],
            shared_subnet: String::new(),
        })
        .send()
        .await
        .expect("join a")
        .json()
        .await
        .expect("join a json");
    assert_eq!(join.network_id.0, network_id);
    assert!(join.peers.is_empty());

    let join_b: lanlink_proto::JoinNetworkResponse = client
        .post(format!("{base}/networks/{network_id}/join"))
        .json(&lanlink_proto::JoinNetworkRequest {
            password: "secret".into(),
            node_id: lanlink_types::NodeId([2u8; 8]),
            public_key: vec![],
            direct_candidates: vec!["127.0.0.1:9002".parse().unwrap()],
            relay_candidates: vec![],
            tcp_candidates: vec![],
            tcp_relay_candidates: vec![],
            shared_subnet: String::new(),
        })
        .send()
        .await
        .expect("join b")
        .json()
        .await
        .expect("join b json");
    assert_eq!(join_b.peers.len(), 1);

    let resp = client
        .get(format!("{base}/networks/{network_id}/members"))
        .send()
        .await
        .expect("list members no auth");
    assert_eq!(resp.status().as_u16(), 401);

    let resp = client
        .get(format!("{base}/networks/{network_id}/members"))
        .header("Authorization", "wrong")
        .send()
        .await
        .expect("list members wrong auth");
    assert_eq!(resp.status().as_u16(), 401);

    let members: Vec<lanlink_proto::PeerInfo> = client
        .get(format!("{base}/networks/{network_id}/members"))
        .header("Authorization", "secret")
        .send()
        .await
        .expect("list members")
        .json()
        .await
        .expect("members json");
    assert_eq!(members.len(), 2);

    // The password may also be supplied as a query parameter.
    let members: Vec<lanlink_proto::PeerInfo> = client
        .get(format!(
            "{base}/networks/{network_id}/members?password=secret"
        ))
        .send()
        .await
        .expect("list members via query")
        .json()
        .await
        .expect("members json");
    assert_eq!(members.len(), 2);
}

#[tokio::test]
async fn coordinator_update_candidates_returns_peers() {
    let (addr, client) = start_test_server().await;
    let base = format!("http://{addr}");

    let create: lanlink_proto::CreateNetworkResponse = client
        .post(format!("{base}/networks"))
        .json(&lanlink_proto::CreateNetworkRequest {
            name: "candidate-test".to_string(),
            password: "secret".into(),
            is_public: false,
        })
        .send()
        .await
        .expect("create")
        .json()
        .await
        .expect("create json");
    let network_id = create.network_id.0;

    // Join two nodes.
    for node in [1u8, 2u8] {
        let mut id = [0u8; 8];
        id[7] = node;
        client
            .post(format!("{base}/networks/{network_id}/join"))
            .json(&lanlink_proto::JoinNetworkRequest {
                password: "secret".into(),
                node_id: lanlink_types::NodeId(id),
                public_key: vec![],
                direct_candidates: vec![format!("127.0.0.1:900{node}").parse().unwrap()],
                relay_candidates: vec![],
                tcp_candidates: vec![],
                tcp_relay_candidates: vec![],
                shared_subnet: String::new(),
            })
            .send()
            .await
            .expect("join");
    }

    let update = lanlink_proto::PresenceUpdate {
        node_id: {
            let mut id = [0u8; 8];
            id[7] = 1;
            lanlink_types::NodeId(id)
        },
        direct_candidates: vec![
            "127.0.0.1:9001".parse().unwrap(),
            "203.0.113.5:9001".parse().unwrap(),
        ],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
    };
    let peers: Vec<lanlink_proto::PeerInfo> = client
        .post(format!("{base}/networks/{network_id}/candidates"))
        .header("Authorization", "Bearer secret")
        .json(&update)
        .send()
        .await
        .expect("update candidates")
        .json()
        .await
        .expect("candidates json");
    assert_eq!(peers.len(), 1);
    let expected_b = {
        let mut id = [0u8; 8];
        id[7] = 2;
        lanlink_types::NodeId(id)
    };
    assert_eq!(peers[0].node_id, expected_b);
}

#[tokio::test]
async fn coordinator_resolve_creates_and_reuses_network() {
    let (addr, client) = start_test_server().await;
    let base = format!("http://{addr}");

    let req = lanlink_proto::ResolveNetworkRequest {
        name: "resolve-test".to_string(),
        password: "secret".into(),
        is_public: true,
    };

    let first: lanlink_proto::ResolveNetworkResponse = client
        .post(format!("{base}/networks/resolve"))
        .json(&req)
        .send()
        .await
        .expect("resolve first")
        .json()
        .await
        .expect("resolve json");
    assert!(first.created);
    let id = first.network_id.0;

    let second: lanlink_proto::ResolveNetworkResponse = client
        .post(format!("{base}/networks/resolve"))
        .json(&req)
        .send()
        .await
        .expect("resolve second")
        .json()
        .await
        .expect("resolve json");
    assert!(!second.created);
    assert_eq!(second.network_id.0, id);

    let bad = lanlink_proto::ResolveNetworkRequest {
        name: "resolve-test".to_string(),
        password: "wrong".into(),
        is_public: true,
    };
    let resp = client
        .post(format!("{base}/networks/resolve"))
        .json(&bad)
        .send()
        .await
        .expect("resolve bad");
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn coordinator_telemetry_records_and_metrics_aggregates() {
    let (addr, client) = start_test_server().await;
    let base = format!("http://{addr}");

    let report = lanlink_proto::telemetry::TelemetryReport {
        version: "0.6.1".to_string(),
        platform: "x86_64-linux".to_string(),
        uptime_seconds: 1234,
        peer_count: 2,
        direct_peer_count: 1,
        relay_peer_count: 1,
        gateway_enabled: false,
        firewall_enabled: true,
        tx_bytes: 1024,
        rx_bytes: 2048,
        timestamp: "2026-10-21T00:00:00Z".to_string(),
    };

    let resp = client
        .post(format!("{base}/telemetry"))
        .json(&report)
        .send()
        .await
        .expect("record telemetry");
    assert_eq!(resp.status().as_u16(), 201);

    let metrics: lanlink_proto::telemetry::AggregatedMetrics = client
        .get(format!("{base}/metrics"))
        .send()
        .await
        .expect("metrics")
        .json()
        .await
        .expect("metrics json");
    assert_eq!(metrics.report_count, 1);
    assert!((metrics.avg_peer_count - 2.0).abs() < f64::EPSILON);
    assert!((metrics.avg_direct_peers - 1.0).abs() < f64::EPSILON);
    assert!((metrics.avg_relay_peers - 1.0).abs() < f64::EPSILON);
    assert_eq!(metrics.versions.len(), 1);
}
