//! TCP transport manager for `lanlinkd`.
//!
//! Handles both direct TCP connections to peers and TCP relay connections.
//! All TCP traffic uses the same length-prefixed framing: a 16-bit big-endian
//! length followed by a LanLink outer packet. The switch does not know whether
//! a packet arrived over UDP or TCP; it just sees the raw outer packet.

use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_FRAME_LEN: usize = 2048;

/// Command sent to the TCP transport task.
#[derive(Debug)]
pub enum TcpCommand {
    /// Send `packet` to `addr` (direct or relay). If no connection exists,
    /// the packet is dropped.
    Send(SocketAddr, Vec<u8>),
    /// Open a new outgoing connection to `addr`.
    Connect(SocketAddr),
}

/// Manager handle used by the daemon to inject packets and request connects.
#[derive(Debug, Clone)]
pub struct TcpTransport {
    tx: tokio::sync::mpsc::UnboundedSender<TcpCommand>,
}

impl TcpTransport {
    /// Bind a TCP listener on `addr` and start the background transport task.
    /// Incoming packets are delivered to `incoming` with the remote address as
    /// the source.
    pub async fn bind(
        addr: SocketAddr,
        incoming: tokio::sync::mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>,
    ) -> anyhow::Result<(Self, SocketAddr)> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(run_transport(listener, incoming, rx));
        Ok((Self { tx }, local_addr))
    }

    /// Enqueue a packet for transmission to `addr`.
    pub fn send(&self, addr: SocketAddr, packet: Vec<u8>) {
        let _ = self.tx.send(TcpCommand::Send(addr, packet));
    }

    /// Request an outgoing connection to `addr`.
    pub fn connect(&self, addr: SocketAddr) {
        let _ = self.tx.send(TcpCommand::Connect(addr));
    }
}

async fn run_transport(
    listener: TcpListener,
    incoming: tokio::sync::mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<TcpCommand>,
) {
    let mut connections: HashMap<SocketAddr, tokio::sync::mpsc::UnboundedSender<Vec<u8>>> =
        HashMap::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, from)) => {
                        add_connection(stream, from, &mut connections, &incoming);
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "TCP accept failed");
                    }
                }
            }
            cmd = commands.recv() => {
                match cmd {
                    Some(TcpCommand::Send(addr, packet)) => {
                        if let Some(tx) = connections.get(&addr) {
                            let _ = tx.send(packet);
                        }
                    }
                    Some(TcpCommand::Connect(addr)) => {
                        if connections.contains_key(&addr) {
                            continue;
                        }
                        match TcpStream::connect(addr).await {
                            Ok(stream) => {
                                add_connection(stream, addr, &mut connections, &incoming);
                            }
                            Err(e) => {
                                tracing::debug!(%addr, error = %e, "TCP connect failed");
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

fn add_connection(
    stream: TcpStream,
    addr: SocketAddr,
    connections: &mut HashMap<SocketAddr, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    incoming: &tokio::sync::mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>,
) {
    let (read_half, write_half) = stream.into_split();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    connections.insert(addr, tx);
    let incoming = incoming.clone();
    tokio::spawn(connection_reader(read_half, addr, incoming));
    tokio::spawn(connection_writer(write_half, addr, rx));
}

/// Read length-prefixed LanLink packets from the read half of a connection.
async fn connection_reader(
    mut read_half: tokio::net::tcp::OwnedReadHalf,
    from: SocketAddr,
    incoming: tokio::sync::mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>,
) {
    loop {
        match read_frame(&mut read_half).await {
            Ok(Some(frame)) => {
                if incoming.send((from, frame)).is_err() {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(%from, error = %e, "TCP connection read failed");
                break;
            }
        }
    }
}

async fn connection_writer(
    mut write_half: tokio::net::tcp::OwnedWriteHalf,
    addr: SocketAddr,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(packet) = rx.recv().await {
        if let Err(e) = write_frame(&mut write_half, &packet).await {
            tracing::debug!(%addr, error = %e, "TCP connection write failed");
            break;
        }
    }
}

async fn read_frame(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
) -> std::io::Result<Option<Vec<u8>>> {
    let len = match reader.read_u16().await {
        Ok(n) => n as usize,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    };
    if len == 0 || len > MAX_FRAME_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame length out of range",
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

async fn write_frame(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
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
