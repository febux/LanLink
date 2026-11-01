use integration_tests::node_identity;
use lanlink_crypto::{Handshake, HandshakeAction};

#[tokio::test]
async fn noise_identity_session_roundtrip() {
    let alice = node_identity(1);
    let bob = node_identity(2);

    let mut alice_hs = Handshake::new_initiator(&alice, bob.public_key()).expect("alice initiator");
    let mut bob_hs = Handshake::new_responder(&bob).expect("bob responder");

    // -> e
    let m1 = alice_hs.write_message().expect("m1");
    let action = bob_hs.read_message(&m1).expect("bob m1");
    let m2 = match action {
        HandshakeAction::Send {
            message,
            session: None,
        } => message,
        other => panic!("expected responder reply, got {other:?}"),
    };

    // <- e, ee, s, es
    let action = alice_hs.read_message(&m2).expect("alice m2");
    let (m3, mut alice_session) = match action {
        HandshakeAction::Send {
            message,
            session: Some(s),
        } => (message, s),
        other => panic!("expected initiator completion and m3, got {other:?}"),
    };

    // -> s, se
    let action = bob_hs.read_message(&m3).expect("bob m3");
    let mut bob_session = match action {
        HandshakeAction::Complete(s) => s,
        other => panic!("expected bob complete, got {other:?}"),
    };

    // Transport round-trip.
    let plaintext = b"hello from alice";
    let ciphertext = alice_session.encrypt(plaintext).expect("encrypt");
    let decrypted = bob_session.decrypt(&ciphertext).expect("decrypt");
    assert_eq!(decrypted, plaintext);

    let reply = bob_session.encrypt(b"ack from bob").expect("encrypt reply");
    assert_eq!(
        alice_session.decrypt(&reply).expect("decrypt reply"),
        b"ack from bob".as_slice()
    );
}

#[tokio::test]
async fn noise_identity_node_ids_match() {
    let id = node_identity(7);
    let node_id = id.node_id();
    let pk = id.public_key();
    assert_eq!(node_id.0, pk[..lanlink_types::NODE_ID_LEN]);
}
