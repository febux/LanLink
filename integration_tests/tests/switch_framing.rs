use integration_tests::{eth_frame, network_psk, node_id, TEST_TIMEOUT};
use lanlink_switch::{Switch, UdpOutcome};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

#[tokio::test]
async fn switch_exchanges_ethernet_frame_over_loopback_udp() {
    let psk = lanlink_crypto::StaticSession::new(network_psk(1));
    let local_a = node_id(1);
    let local_b = node_id(2);

    let socket_a = UdpSocket::bind("127.0.0.1:0").await.expect("bind a");
    let socket_b = UdpSocket::bind("127.0.0.1:0").await.expect("bind b");
    let addr_a = socket_a.local_addr().unwrap();
    let addr_b = socket_b.local_addr().unwrap();

    let mut a = Switch::new(local_a, psk.clone());
    let mut b = Switch::new(local_b, psk.clone());
    a.add_peer(local_b, addr_b);
    b.add_peer(local_a, addr_a);

    let src_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let dst_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x02];
    let payload = vec![0xab; 64];
    let frame = eth_frame(src_mac, dst_mac, &payload);

    let outgoing = a.on_tap_frame(&frame).expect("a encodes frame");
    assert!(!outgoing.is_empty());
    for pkt in &outgoing {
        socket_a
            .send_to(&pkt.packet, pkt.addr)
            .await
            .expect("send a");
    }

    let mut buf = vec![0u8; 2048];
    let (n, from) = timeout(TEST_TIMEOUT, socket_b.recv_from(&mut buf))
        .await
        .expect("b receives packet")
        .expect("recv_from b");
    assert_eq!(from, addr_a);

    let decoded = match b.on_packet(addr_a, &buf[..n]).expect("b decodes") {
        UdpOutcome::Frame(f) => f,
        other => panic!("expected Frame, got {other:?}"),
    };
    assert_eq!(decoded, frame);

    // B should have learned A's source MAC.
    assert_eq!(
        b.peer_stats()
            .iter()
            .find(|s| s.node_id == local_a)
            .map(|s| s.addr),
        Some(addr_a)
    );
}

#[tokio::test]
async fn switch_reassembles_fragmented_frame_over_loopback_udp() {
    let psk = lanlink_crypto::StaticSession::new(network_psk(2));
    let local_a = node_id(1);
    let local_b = node_id(2);

    let socket_a = UdpSocket::bind("127.0.0.1:0").await.expect("bind a");
    let socket_b = UdpSocket::bind("127.0.0.1:0").await.expect("bind b");
    let addr_a = socket_a.local_addr().unwrap();
    let addr_b = socket_b.local_addr().unwrap();

    let mut a = Switch::new(local_a, psk.clone());
    let mut b = Switch::new(local_b, psk.clone());
    a.add_peer(local_b, addr_b);
    b.add_peer(local_a, addr_a);

    // Payload large enough to force at least two inner chunks.
    let big_payload = vec![0xcd; 1500];
    let src_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x03];
    let dst_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x04];
    let frame = eth_frame(src_mac, dst_mac, &big_payload);

    let outgoing = a.on_tap_frame(&frame).expect("a encodes big frame");
    assert!(
        outgoing.len() >= 2,
        "large frame should be split into multiple packets"
    );

    // Send all chunks from A to B.
    for pkt in &outgoing {
        socket_a
            .send_to(&pkt.packet, pkt.addr)
            .await
            .expect("send chunk");
    }

    // Collect chunks until reassembly completes.
    let mut assembled: Option<Vec<u8>> = None;
    let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
    let mut buf = vec![0u8; 2048];
    while assembled.is_none() && tokio::time::Instant::now() < deadline {
        let (n, from) = timeout(Duration::from_millis(500), socket_b.recv_from(&mut buf))
            .await
            .expect("b receives chunk")
            .expect("recv_from b");
        assert_eq!(from, addr_a);
        match b.on_packet(addr_a, &buf[..n]).expect("b handles chunk") {
            UdpOutcome::Frame(f) => assembled = Some(f),
            UdpOutcome::Ignored => {}
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    assert_eq!(assembled.expect("frame was reassembled"), frame);
}
