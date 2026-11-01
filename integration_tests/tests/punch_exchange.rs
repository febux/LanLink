use integration_tests::{network_psk, node_id, wait_for_tcp};
use lanlink_punch::{Candidate, Puncher};
use lanlink_relay::{encode_bind, RelayServer};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

async fn start_coordinator() -> (SocketAddr, reqwest::Client) {
    let pool = coordinator::db::connect_test().await.expect("test db");
    let app = coordinator::app(pool, vec![], vec![]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    wait_for_tcp(addr).await.expect("coordinator did not start");
    (addr, reqwest::Client::new())
}

#[tokio::test]
async fn punchers_verify_reply_path_via_coordinator_candidates() {
    // Shared PSK and node ids.
    let session = Arc::new(lanlink_crypto::StaticSession::new(network_psk(3)));
    let local_a = node_id(1);
    let local_b = node_id(2);

    // Real UDP sockets for the two peers.
    let socket_a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("bind a"));
    let socket_b = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("bind b"));
    let addr_a = socket_a.local_addr().unwrap();
    let addr_b = socket_b.local_addr().unwrap();

    // A local relay server is available (content-blind fallback path).
    let relay = RelayServer::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind relay");
    let relay_addr = relay.local_addr().unwrap();
    tokio::spawn(relay.run());

    // In-process coordinator that will exchange candidates.
    let (coord_addr, client) = start_coordinator().await;
    let base = format!("http://{coord_addr}");

    let create: lanlink_proto::CreateNetworkResponse = client
        .post(format!("{base}/networks"))
        .json(&lanlink_proto::CreateNetworkRequest {
            name: "punch-test".to_string(),
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

    // Helper to join a peer with its direct and relay candidates.
    async fn join(
        client: &reqwest::Client,
        base: &str,
        network_id: u64,
        node: lanlink_types::NodeId,
        direct: SocketAddr,
        relay: SocketAddr,
    ) {
        client
            .post(format!("{base}/networks/{network_id}/join"))
            .json(&lanlink_proto::JoinNetworkRequest {
                password: "secret".into(),
                node_id: node,
                public_key: vec![],
                direct_candidates: vec![direct],
                relay_candidates: vec![relay],
                tcp_candidates: vec![],
                tcp_relay_candidates: vec![],
                shared_subnet: String::new(),
            })
            .send()
            .await
            .expect("join")
            .error_for_status()
            .expect("join 2xx");
    }

    join(&client, &base, network_id, local_a, addr_a, relay_addr).await;
    join(&client, &base, network_id, local_b, addr_b, relay_addr).await;

    // A refreshes its candidates and fetches the current peer list from the
    // coordinator, which includes B's advertised addresses.
    let update = lanlink_proto::PresenceUpdate {
        node_id: local_a,
        direct_candidates: vec![addr_a],
        relay_candidates: vec![relay_addr],
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
    let peer_b = peers.into_iter().next().unwrap();
    assert_eq!(peer_b.node_id, local_b);

    // Punchers for A and B; empty STUN list keeps the test local.
    let puncher_a = Arc::new(Puncher::new(
        local_a,
        socket_a.clone(),
        (*session).clone(),
        vec![],
    ));
    let puncher_b = Arc::new(Puncher::new(
        local_b,
        socket_b.clone(),
        (*session).clone(),
        vec![],
    ));

    // B handles exactly one incoming PUNCH_PROBE and replies.
    let b_handle = tokio::spawn({
        let socket_b = socket_b.clone();
        let puncher_b = puncher_b.clone();
        async move {
            let mut buf = vec![0u8; 512];
            let (n, from) = timeout(Duration::from_secs(5), socket_b.recv_from(&mut buf))
                .await
                .expect("b recv timeout")
                .expect("b recv");
            puncher_b.on_punch_probe(from, &buf[..n]).await.ok()
        }
    });

    // A starts a real punch attempt against B's direct candidate so the
    // expected reply cookie is registered before we process the reply.
    let remote_candidates: Vec<Candidate> = peer_b
        .direct_candidates
        .into_iter()
        .map(|addr| Candidate {
            addr,
            kind: lanlink_punch::CandidateKind::Host,
        })
        .collect();
    assert!(!remote_candidates.is_empty());

    let punch_handle = tokio::spawn({
        let puncher_a = puncher_a.clone();
        async move { puncher_a.punch_peer(local_b, remote_candidates).await }
    });

    // Wait for B to process the probe.
    let _ = b_handle.await;

    // A should now receive a PUNCH_REPLY and report the verified path.
    let mut buf = vec![0u8; 512];
    let (n, from) = timeout(Duration::from_secs(5), socket_a.recv_from(&mut buf))
        .await
        .expect("a recv timeout")
        .expect("a recv");
    assert_eq!(from, addr_b);

    let path = puncher_a
        .on_punch_reply(from, &buf[..n])
        .expect("decode reply")
        .expect("reply accepted");
    assert_eq!(path.peer, local_b);
    assert_eq!(path.addr, addr_b);

    punch_handle.abort();
}

#[tokio::test]
async fn relay_server_forwards_between_bound_clients() {
    let node_a = node_id(10);
    let node_b = node_id(11);

    let relay = RelayServer::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind relay");
    let relay_addr = relay.local_addr().unwrap();
    tokio::spawn(relay.run());

    let socket_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let socket_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    socket_a
        .send_to(&encode_bind(node_a), relay_addr)
        .await
        .unwrap();
    socket_b
        .send_to(&encode_bind(node_b), relay_addr)
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut data = vec![0u8; 4 + lanlink_types::NODE_ID_LEN + 4];
    data[0] = lanlink_types::WIRE_PROTOCOL_VERSION;
    data[1] = lanlink_types::DATA_PACKET_TYPE;
    data[4..4 + lanlink_types::NODE_ID_LEN].copy_from_slice(&node_b.0);
    data[4 + lanlink_types::NODE_ID_LEN..].copy_from_slice(b"ping");

    socket_a.send_to(&data, relay_addr).await.unwrap();

    let mut buf = [0u8; 256];
    let (n, from) = timeout(Duration::from_secs(5), socket_b.recv_from(&mut buf))
        .await
        .expect("recv timeout")
        .expect("recv");
    assert_eq!(from, relay_addr);
    assert_eq!(&buf[..n], &data);
}
