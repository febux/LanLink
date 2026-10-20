//! TCP relay server for LanLink — content-blind fallback path over TCP.
//!
//! The relay server accepts length-prefixed frames on a TCP connection. The
//! first frame must be a relay bind/keepalive control message (same format as
//! the UDP relay bind). After binding, the server associates the connection
//! with the client's `node_id`. Subsequent length-prefixed LanLink packets are
//! routed by reading the receiver node id in the LanLink outer header.
//!
//! Wire format:
//!
//! ```text
//! [length: u16 BE]
//! [payload: length bytes]
//! ```
//!
//! The payload is either a relay control packet (`LLR\x01` magic) or a raw
//! LanLink outer packet. The relay never decrypts traffic.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

const MAX_FRAME_LEN: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum TcpRelayError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame too large")]
    FrameTooLarge,
    #[error("malformed control packet")]
    MalformedControl,
}

/// A bound TCP relay server.
pub struct TcpRelayServer {
    listener: TcpListener,
    state: Arc<Mutex<RelayState>>,
}

struct RelayState {
    bindings: HashMap<lanlink_proto::NodeId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
}

impl TcpRelayServer {
    /// Bind a new TCP relay server to `addr`.
    pub async fn bind(addr: SocketAddr) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!(%addr, "TCP relay server listening");
        Ok(Self {
            listener,
            state: Arc::new(Mutex::new(RelayState {
                bindings: HashMap::new(),
            })),
        })
    }

    /// Return the local bound address.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Run the relay accept loop until shutdown.
    pub async fn run(self) -> anyhow::Result<()> {
        loop {
            let (stream, from) = match self.listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!(error = %e, "TCP relay accept failed");
                    continue;
                }
            };
            let state = self.state.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_client(stream, from, state).await {
                    tracing::debug!(%from, error = %e, "TCP relay client disconnected");
                }
            });
        }
    }

    /// Return a snapshot of currently-bound node ids (useful for diagnostics).
    pub async fn bindings(&self) -> Vec<lanlink_proto::NodeId> {
        self.state.lock().await.bindings.keys().copied().collect()
    }
}

async fn handle_client(
    stream: TcpStream,
    from: SocketAddr,
    state: Arc<Mutex<RelayState>>,
) -> Result<(), TcpRelayError> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (mut read_half, mut write_half) = stream.into_split();

    // First frame must bind this connection to a node id.
    let first_frame = read_frame(&mut read_half)
        .await?
        .ok_or(TcpRelayError::MalformedControl)?;
    let control = crate::decode_control(&first_frame).ok_or(TcpRelayError::MalformedControl)?;
    let node_id = control.node_id;
    tracing::info!(?node_id, %from, "TCP relay client bound");

    {
        let mut s = state.lock().await;
        s.bindings.insert(node_id, tx);
    }

    // Forward packets from other clients to this connection.
    let forward_task = tokio::spawn(async move {
        while let Some(packet) = rx.recv().await {
            if let Err(e) = write_frame(&mut write_half, &packet).await {
                tracing::debug!(%from, error = %e, "TCP relay forward failed");
                break;
            }
        }
    });

    // Read length-prefixed LanLink packets and route them.
    loop {
        let frame = match read_frame(&mut read_half).await {
            Ok(Some(f)) => f,
            Ok(None) => break, // peer closed
            Err(e) => {
                tracing::debug!(%from, error = %e, "TCP relay read failed");
                break;
            }
        };

        // Keepalive / rebind control packet?
        if crate::decode_control(&frame).is_some() {
            continue;
        }

        if frame.len() < 4 + lanlink_proto::NODE_ID_LEN {
            tracing::debug!(%from, len = frame.len(), "TCP relay frame too short");
            continue;
        }

        let mut receiver = [0u8; lanlink_proto::NODE_ID_LEN];
        receiver.copy_from_slice(&frame[4..4 + lanlink_proto::NODE_ID_LEN]);
        let receiver = lanlink_proto::NodeId(receiver);

        let s = state.lock().await;
        if let Some(sender) = s.bindings.get(&receiver) {
            let bytes = frame.len();
            let _ = sender.send(frame);
            tracing::trace!(?receiver, %from, bytes, "TCP relay forwarded packet");
        } else {
            tracing::debug!(?receiver, %from, "TCP relay no binding for receiver");
        }
    }

    forward_task.abort();
    let mut s = state.lock().await;
    s.bindings.remove(&node_id);
    Ok(())
}

/// Read one length-prefixed frame. Returns `None` when the peer has closed
/// cleanly.
async fn read_frame(
    reader: &mut (impl AsyncReadExt + Unpin),
) -> Result<Option<Vec<u8>>, TcpRelayError> {
    let len = match reader.read_u16().await {
        Ok(n) => n as usize,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if len == 0 || len > MAX_FRAME_LEN {
        return Err(TcpRelayError::FrameTooLarge);
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

/// Write a length-prefixed frame.
async fn write_frame(
    writer: &mut (impl AsyncWriteExt + Unpin),
    payload: &[u8],
) -> std::io::Result<()> {
    if payload.len() > u16::MAX as usize {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "payload exceeds u16 length",
        ));
    }
    let len = (payload.len() as u16).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(payload).await?;
    Ok(())
}

/// TCP relay client helpers.
pub mod client {
    use super::*;

    /// Connect to a TCP relay server, send a bind message, and return a sink
    /// for outgoing length-prefixed packets plus a stream of incoming packets.
    pub async fn connect(
        addr: SocketAddr,
        node_id: lanlink_proto::NodeId,
    ) -> anyhow::Result<(
        tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    )> {
        let stream = TcpStream::connect(addr).await?;
        let (mut read_half, mut write_half) = stream.into_split();

        // Send the bind message length-prefixed.
        let bind = crate::encode_bind(node_id);
        write_frame(&mut write_half, &bind).await?;

        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

        // Writer task: send outgoing packets to the relay.
        tokio::spawn(async move {
            while let Some(packet) = out_rx.recv().await {
                if let Err(e) = write_frame(&mut write_half, &packet).await {
                    tracing::debug!(%addr, error = %e, "TCP relay client write failed");
                    break;
                }
            }
        });

        // Reader task: deliver incoming packets from the relay.
        tokio::spawn(async move {
            loop {
                match read_frame(&mut read_half).await {
                    Ok(Some(frame)) => {
                        if in_tx.send(frame).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::debug!(%addr, error = %e, "TCP relay client read failed");
                        break;
                    }
                }
            }
        });

        Ok((out_tx, in_rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn node(id: u8) -> lanlink_proto::NodeId {
        let mut b = [0u8; 8];
        b[7] = id;
        lanlink_proto::NodeId(b)
    }

    fn fake_lanlink_packet(receiver: lanlink_proto::NodeId) -> Vec<u8> {
        let mut data = vec![0u8; 4 + lanlink_proto::NODE_ID_LEN + 4];
        data[0] = lanlink_proto::WIRE_PROTOCOL_VERSION;
        data[1] = lanlink_proto::DATA_PACKET_TYPE;
        data[4..4 + lanlink_proto::NODE_ID_LEN].copy_from_slice(&receiver.0);
        data[4 + lanlink_proto::NODE_ID_LEN..].copy_from_slice(b"ping");
        data
    }

    #[tokio::test]
    async fn tcp_relay_forwards_between_bound_clients() {
        let server = TcpRelayServer::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind TCP relay");
        let addr = server.local_addr().unwrap();
        tokio::spawn(server.run());

        let node_a = node(1);
        let node_b = node(2);

        let (tx_a, _rx_a) = client::connect(addr, node_a).await.unwrap();
        let (_tx_b, mut rx_b) = client::connect(addr, node_b).await.unwrap();

        // Give the server a moment to process the binds.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // A sends a fake LanLink packet addressed to B.
        let packet = fake_lanlink_packet(node_b);
        tx_a.send(packet.clone()).unwrap();

        let received = tokio::time::timeout(Duration::from_secs(2), rx_b.recv())
            .await
            .expect("timeout")
            .expect("received packet");
        assert_eq!(received, packet);
    }
}
