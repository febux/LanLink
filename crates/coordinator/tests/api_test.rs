use axum::body::Body;
use tower::ServiceExt;

#[tokio::test]
async fn create_and_join_network() {
    let pool = coordinator::db::connect_test().await.unwrap();
    let app = coordinator::app(pool, vec![], vec![]);

    // Create network
    let create_req = lanlink_proto::CreateNetworkRequest {
        name: "testnet".to_string(),
        password: "secret".to_string(),
        is_public: true,
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&create_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_resp: lanlink_proto::CreateNetworkResponse = serde_json::from_slice(&body).unwrap();
    let network_id = create_resp.network_id.0;
    assert!(network_id > 0);

    // Join with correct password
    let join_req = lanlink_proto::JoinNetworkRequest {
        password: "secret".to_string(),
        node_id: lanlink_proto::NodeId([1; 8]),
        public_key: vec![],
        direct_candidates: vec!["127.0.0.1:9000".parse().unwrap()],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
        shared_subnet: String::new(),
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/networks/{}/join", network_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&join_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let join_resp: lanlink_proto::JoinNetworkResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(join_resp.network_id.0, network_id);
    assert_eq!(join_resp.peers.len(), 0); // first peer has no other peers
    assert!(join_resp.is_public);

    // Join with wrong password should fail
    let bad_req = lanlink_proto::JoinNetworkRequest {
        password: "wrong".to_string(),
        node_id: lanlink_proto::NodeId([2; 8]),
        public_key: vec![],
        direct_candidates: vec![],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
        shared_subnet: String::new(),
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/networks/{}/join", network_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&bad_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);

    // Add a second member and verify list
    let join_req2 = lanlink_proto::JoinNetworkRequest {
        password: "secret".to_string(),
        node_id: lanlink_proto::NodeId([3; 8]),
        public_key: vec![],
        direct_candidates: vec!["127.0.0.1:9001".parse().unwrap()],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
        shared_subnet: "10.0.0.0/24".to_string(),
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/networks/{}/join", network_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&join_req2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/networks/{}/members", network_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let members: Vec<lanlink_proto::PeerInfo> = serde_json::from_slice(&body).unwrap();
    assert_eq!(members.len(), 2);
    let gateway = members
        .iter()
        .find(|m| m.is_gateway)
        .expect("gateway member");
    assert_eq!(gateway.shared_subnet, "10.0.0.0/24");
}

#[tokio::test]
async fn leave_network_removes_member_and_rejects_invalid_requests() {
    let pool = coordinator::db::connect_test().await.unwrap();
    let app = coordinator::app(pool, vec![], vec![]);

    // Create network
    let create_req = lanlink_proto::CreateNetworkRequest {
        name: "leavenet".to_string(),
        password: "secret".to_string(),
        is_public: false,
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&create_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_resp: lanlink_proto::CreateNetworkResponse = serde_json::from_slice(&body).unwrap();
    let network_id = create_resp.network_id.0;

    // Join a member
    let join_req = lanlink_proto::JoinNetworkRequest {
        password: "secret".to_string(),
        node_id: lanlink_proto::NodeId([7; 8]),
        public_key: vec![],
        direct_candidates: vec!["127.0.0.1:9000".parse().unwrap()],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
        shared_subnet: String::new(),
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/networks/{}/join", network_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&join_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Leave with wrong password is rejected
    let bad_leave = lanlink_proto::LeaveNetworkRequest {
        password: "wrong".to_string(),
        node_id: join_req.node_id,
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/networks/{}/leave", network_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&bad_leave).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);

    // Leave with unknown node id is rejected
    let unknown_leave = lanlink_proto::LeaveNetworkRequest {
        password: "secret".to_string(),
        node_id: lanlink_proto::NodeId([8; 8]),
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/networks/{}/leave", network_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&unknown_leave).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

    // Valid leave removes the member
    let leave_req = lanlink_proto::LeaveNetworkRequest {
        password: "secret".to_string(),
        node_id: join_req.node_id,
    };
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/networks/{}/leave", network_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&leave_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);

    // Member list is now empty
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/networks/{}/members", network_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let members: Vec<lanlink_proto::PeerInfo> = serde_json::from_slice(&body).unwrap();
    assert_eq!(members.len(), 0);

    // Leave against a non-existent network is rejected
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/networks/999999/leave")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&leave_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}
