use axum::body::Body;
use std::net::SocketAddr;
use tower::ServiceExt;

#[tokio::test]
async fn candidate_update_returns_peer_list() {
    let pool = coordinator::db::connect_test().await.expect("db");
    let app = coordinator::app(pool, vec![], vec![]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr: SocketAddr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Create a network.
    let create_req = lanlink_proto::CreateNetworkRequest {
        name: "punch-test".to_string(),
        password: "secret".into(),
        is_public: false,
    };
    let create_resp: lanlink_proto::CreateNetworkResponse = client
        .post(format!("{}/networks", base))
        .json(&create_req)
        .send()
        .await
        .expect("create")
        .json()
        .await
        .expect("create json");

    let network_id = create_resp.network_id.0;

    // Join as node A.
    let join_a = lanlink_proto::JoinNetworkRequest {
        password: "secret".into(),
        node_id: lanlink_types::NodeId([1u8; 8]),
        public_key: vec![],
        direct_candidates: vec!["192.168.1.10:9000".parse().unwrap()],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
        shared_subnet: String::new(),
    };
    let resp_a: lanlink_proto::JoinNetworkResponse = client
        .post(format!("{}/networks/{}/join", base, network_id))
        .json(&join_a)
        .send()
        .await
        .expect("join a")
        .json()
        .await
        .expect("join a json");
    assert!(resp_a.peers.is_empty());

    // Join as node B.
    let join_b = lanlink_proto::JoinNetworkRequest {
        password: "secret".into(),
        node_id: lanlink_types::NodeId([2u8; 8]),
        public_key: vec![],
        direct_candidates: vec!["192.168.1.20:9000".parse().unwrap()],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
        shared_subnet: String::new(),
    };
    let resp_b: lanlink_proto::JoinNetworkResponse = client
        .post(format!("{}/networks/{}/join", base, network_id))
        .json(&join_b)
        .send()
        .await
        .expect("join b")
        .json()
        .await
        .expect("join b json");
    assert_eq!(resp_b.peers.len(), 1);

    // Node A updates its candidates and should see node B.
    let update = lanlink_proto::PresenceUpdate {
        node_id: lanlink_types::NodeId([1u8; 8]),
        direct_candidates: vec![
            "192.168.1.10:9000".parse().unwrap(),
            "203.0.113.5:9000".parse().unwrap(),
        ],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
    };
    let peers: Vec<lanlink_proto::PeerInfo> = client
        .post(format!("{}/networks/{}/candidates", base, network_id))
        .header("Authorization", "Bearer secret")
        .json(&update)
        .send()
        .await
        .expect("update candidates")
        .json()
        .await
        .expect("candidates json");

    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].node_id, lanlink_types::NodeId([2u8; 8]));
    assert!(peers[0]
        .direct_candidates
        .contains(&"192.168.1.20:9000".parse().unwrap()));
}

#[tokio::test]
async fn update_candidates_requires_auth() {
    let pool = coordinator::db::connect_test().await.expect("db");
    let app = coordinator::app(pool, vec![], vec![]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr: SocketAddr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let create: lanlink_proto::CreateNetworkResponse = client
        .post(format!("{}/networks", base))
        .json(&lanlink_proto::CreateNetworkRequest {
            name: "auth-test".to_string(),
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

    client
        .post(format!("{}/networks/{}/join", base, network_id))
        .json(&lanlink_proto::JoinNetworkRequest {
            password: "secret".into(),
            node_id: lanlink_types::NodeId([1u8; 8]),
            public_key: vec![],
            direct_candidates: vec!["192.168.1.10:9000".parse().unwrap()],
            relay_candidates: vec![],
            tcp_candidates: vec![],
            tcp_relay_candidates: vec![],
            shared_subnet: String::new(),
        })
        .send()
        .await
        .expect("join");

    let update = lanlink_proto::PresenceUpdate {
        node_id: lanlink_types::NodeId([1u8; 8]),
        direct_candidates: vec!["192.168.1.10:9000".parse().unwrap()],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
    };

    // Missing password returns 401.
    let resp = client
        .post(format!("{}/networks/{}/candidates", base, network_id))
        .json(&update)
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 401);

    // Invalid password returns 401.
    let resp = client
        .post(format!("{}/networks/{}/candidates", base, network_id))
        .header("Authorization", "Bearer wrong")
        .json(&update)
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 401);

    // Non-member node_id returns 403 even with a valid password.
    let non_member = lanlink_proto::PresenceUpdate {
        node_id: lanlink_types::NodeId([9u8; 8]),
        ..update.clone()
    };
    let resp = client
        .post(format!("{}/networks/{}/candidates", base, network_id))
        .header("Authorization", "Bearer secret")
        .json(&non_member)
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn directory_lists_only_public_networks() {
    let pool = coordinator::db::connect_test().await.expect("db");
    let app = coordinator::app(pool, vec![], vec![]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr: SocketAddr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Create a public and a private network.
    let public_resp: lanlink_proto::CreateNetworkResponse = client
        .post(format!("{}/networks", base))
        .json(&lanlink_proto::CreateNetworkRequest {
            name: "public-gaming".to_string(),
            password: "secret".into(),
            is_public: true,
        })
        .send()
        .await
        .expect("create public")
        .json()
        .await
        .expect("public json");

    client
        .post(format!("{}/networks", base))
        .json(&lanlink_proto::CreateNetworkRequest {
            name: "private-lan".to_string(),
            password: "secret".into(),
            is_public: false,
        })
        .send()
        .await
        .expect("create private");

    // Join the public network so it has an online member.
    let join = lanlink_proto::JoinNetworkRequest {
        password: "secret".into(),
        node_id: lanlink_types::NodeId([3u8; 8]),
        public_key: vec![],
        direct_candidates: vec!["192.168.1.30:9000".parse().unwrap()],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
        shared_subnet: String::new(),
    };
    client
        .post(format!(
            "{}/networks/{}/join",
            base, public_resp.network_id.0
        ))
        .json(&join)
        .send()
        .await
        .expect("join public");

    // Directory should show only the public network with one member.
    let directory: Vec<lanlink_proto::PublicNetworkInfo> = client
        .get(format!("{}/directory", base))
        .send()
        .await
        .expect("directory")
        .json()
        .await
        .expect("directory json");

    assert_eq!(directory.len(), 1);
    assert_eq!(directory[0].name, "public-gaming");
    assert_eq!(directory[0].network_id, public_resp.network_id);
    assert!(directory[0].is_public);
    assert_eq!(directory[0].member_count, 1);
}

#[tokio::test]
async fn resolve_network_creates_and_returns_existing() {
    let pool = coordinator::db::connect_test().await.expect("db");
    let app = coordinator::app(pool, vec![], vec![]);

    // First call creates the network.
    let req = lanlink_proto::ResolveNetworkRequest {
        name: "resolve-test".to_string(),
        password: "secret".into(),
        is_public: true,
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks/resolve")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp: lanlink_proto::ResolveNetworkResponse = serde_json::from_slice(&body).unwrap();
    assert!(resp.created);
    assert!(resp.is_public);
    let id = resp.network_id.0;

    // Second call with the same name+password returns the existing network.
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks/resolve")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp: lanlink_proto::ResolveNetworkResponse = serde_json::from_slice(&body).unwrap();
    assert!(!resp.created);
    assert_eq!(resp.network_id.0, id);

    // Wrong password is rejected.
    let bad = lanlink_proto::ResolveNetworkRequest {
        name: "resolve-test".to_string(),
        password: "wrong".into(),
        is_public: true,
    };
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks/resolve")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&bad).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn telemetry_records_and_aggregates() {
    let pool = coordinator::db::connect_test().await.expect("db");
    let app = coordinator::app(pool, vec![], vec![]);

    let report = lanlink_proto::telemetry::TelemetryReport {
        version: "0.1.0".to_string(),
        platform: "x86_64-linux".to_string(),
        uptime_seconds: 3600,
        peer_count: 4,
        direct_peer_count: 3,
        relay_peer_count: 1,
        gateway_enabled: false,
        firewall_enabled: true,
        tx_bytes: 1024,
        rx_bytes: 2048,
        timestamp: "2026-10-11T00:00:00Z".to_string(),
    };

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/telemetry")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&report).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let metrics: lanlink_proto::telemetry::AggregatedMetrics =
        serde_json::from_slice(&body).unwrap();
    assert_eq!(metrics.report_count, 1);
    assert!((metrics.avg_peer_count - 4.0).abs() < f64::EPSILON);
    assert!((metrics.avg_direct_peers - 3.0).abs() < f64::EPSILON);
    assert!((metrics.avg_relay_peers - 1.0).abs() < f64::EPSILON);
    assert!((metrics.firewall_fraction - 1.0).abs() < f64::EPSILON);
    assert_eq!(metrics.versions.len(), 1);
    assert_eq!(metrics.versions[0].version, "0.1.0");
}

#[tokio::test]
async fn create_network_duplicate_returns_conflict() {
    let pool = coordinator::db::connect_test().await.expect("db");
    let app = coordinator::app(pool, vec![], vec![]);

    let req = lanlink_proto::CreateNetworkRequest {
        name: "duplicate-network".to_string(),
        password: "secret".into(),
        is_public: false,
    };

    // First creation succeeds.
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    // Second creation with the same name returns 409 CONFLICT.
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn create_network_db_error_returns_internal_server_error() {
    let pool = coordinator::db::connect_test().await.expect("db");
    pool.close().await;
    let app = coordinator::app(pool, vec![], vec![]);

    let req = lanlink_proto::CreateNetworkRequest {
        name: "db-error-network".to_string(),
        password: "secret".into(),
        is_public: false,
    };

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn resolve_network_db_error_returns_internal_server_error() {
    let pool = coordinator::db::connect_test().await.expect("db");
    pool.close().await;
    let app = coordinator::app(pool, vec![], vec![]);

    let req = lanlink_proto::ResolveNetworkRequest {
        name: "resolve-db-error".to_string(),
        password: "secret".into(),
        is_public: false,
    };

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks/resolve")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn aggregate_metrics_returns_defaults_on_empty_table() {
    let pool = coordinator::db::connect_test().await.expect("db");

    let metrics = coordinator::db::aggregate_metrics(&pool, 3600)
        .await
        .expect("aggregate on empty table");

    assert_eq!(metrics.report_count, 0);
    assert_eq!(metrics.avg_peer_count, 0.0);
    assert_eq!(metrics.avg_direct_peers, 0.0);
    assert_eq!(metrics.avg_relay_peers, 0.0);
    assert_eq!(metrics.gateway_fraction, 0.0);
    assert_eq!(metrics.firewall_fraction, 0.0);
    assert_eq!(metrics.total_tx_bytes, 0);
    assert_eq!(metrics.total_rx_bytes, 0);
    assert!(metrics.versions.is_empty());
    assert!(metrics.platforms.is_empty());
}
