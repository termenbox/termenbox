//! TURN client for the assisted connection (DECISION-0140, step 2.6): when the server's NAT blocks inbound packets
//! (WSL2 in NAT mode, a symmetric NAT) the server allocates a relay on the owner's Cloudflare TURN key and offers
//! the relayed address as a candidate. The `turn` crate (FACT-0060) owns the allocation: it refreshes it at half its
//! lifetime, creates a permission for each peer the first time we send to it, and releases it on close.
//!
//! The relay uses its own UDP socket to the TURN server, so nothing here touches the transport socket. Packets from
//! peers come out through the inbox the transport handed us; packets to peers go in through `send`.
#![allow(dead_code)] // allocated by the signal client of step 2.5

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};
use turn::client::{Client, ClientConfig};
use webrtc_util::Conn;

/// How long the TURN server gets to answer an allocation.
const ALLOCATE_TIMEOUT: Duration = Duration::from_secs(5);

/// Short-lived credentials for one allocation (minted by step 2.5, GUARDRAIL relay-credentials-are-owner-scoped).
/// `Debug` hides the password: credentials never reach a log.
#[derive(Clone)]
pub struct Credentials {
    /// `host:port` of the TURN server, UDP.
    pub server: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

/// A datagram a peer sent to our relayed address.
#[derive(Debug)]
pub struct Packet {
    /// Which allocation it arrived on.
    pub relay: SocketAddr,
    pub peer: SocketAddr,
    pub data: Vec<u8>,
}

/// One live allocation. Dropping it releases the allocation.
pub struct Relay {
    addr: SocketAddr,
    local: SocketAddr,
    out: mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>,
    client: Arc<Client>,
    conn: Arc<dyn Conn + Send + Sync>,
    tasks: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for Relay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relay").field("addr", &self.addr).finish()
    }
}

impl Relay {
    /// The address peers send to: the relay candidate.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The local address of the socket that talks to the TURN server (the candidate's base).
    pub fn local(&self) -> SocketAddr {
        self.local
    }

    /// Queues a datagram for `peer`. The first one to a peer's IP also creates its permission, which is what lets that
    /// peer's replies in.
    pub fn send(&self, peer: SocketAddr, data: &[u8]) {
        let _ = self.out.send((peer, data.to_vec()));
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
        let (client, conn) = (self.client.clone(), self.conn.clone());
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async move {
                // Closing the relayed connection is what tells the TURN server to release the allocation.
                let _ = conn.close().await;
                let _ = client.close().await;
            });
        }
    }
}

/// Allocates a relay and starts moving packets: whatever arrives on it goes to `inbox`.
pub async fn allocate(creds: &Credentials, inbox: mpsc::UnboundedSender<Packet>) -> Result<Relay> {
    tokio::time::timeout(ALLOCATE_TIMEOUT, open(creds, inbox))
        .await
        .map_err(|_| anyhow!("the TURN server did not answer"))?
}

/// `allocate`, except that a refusal is not an error: the session just has no relay candidate (and the reason goes
/// to the log without the credentials).
pub async fn allocate_or_none(
    creds: &Credentials,
    inbox: mpsc::UnboundedSender<Packet>,
) -> Option<Relay> {
    match allocate(creds, inbox).await {
        Ok(r) => Some(r),
        Err(e) => {
            warn!("no TURN relay for this session: {e:#}");
            None
        }
    }
}

async fn open(creds: &Credentials, inbox: mpsc::UnboundedSender<Packet>) -> Result<Relay> {
    let server = tokio::net::lookup_host(&creds.server)
        .await
        .with_context(|| format!("cannot resolve {}", creds.server))?
        .next()
        .ok_or_else(|| anyhow!("{} has no address", creds.server))?;
    let bind: SocketAddr = if server.is_ipv4() {
        "0.0.0.0:0".parse()?
    } else {
        "[::]:0".parse()?
    };
    let socket = Arc::new(UdpSocket::bind(bind).await?);
    let local = socket.local_addr()?;
    let client = Arc::new(
        Client::new(ClientConfig {
            stun_serv_addr: creds.server.clone(),
            turn_serv_addr: creds.server.clone(),
            username: creds.username.clone(),
            password: creds.password.clone(),
            realm: String::new(),
            software: String::new(),
            rto_in_ms: 0,
            conn: socket,
            vnet: None,
        })
        .await?,
    );
    client.listen().await?;
    let conn = match client.allocate().await {
        Ok(c) => Arc::new(c) as Arc<dyn Conn + Send + Sync>,
        Err(e) => {
            let _ = client.close().await;
            return Err(anyhow!("the TURN server refused the allocation: {e}"));
        }
    };
    let addr = conn.local_addr()?;
    debug!("TURN relay allocated at {addr}");

    let (out, mut queue) = mpsc::unbounded_channel::<(SocketAddr, Vec<u8>)>();
    let writer = {
        let conn = conn.clone();
        tokio::spawn(async move {
            while let Some((peer, data)) = queue.recv().await {
                if let Err(e) = conn.send_to(&data, peer).await {
                    debug!("relay send to {peer} failed: {e}");
                }
            }
        })
    };
    let reader = {
        let conn = conn.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2000];
            while let Ok((n, peer)) = conn.recv_from(&mut buf).await {
                let packet = Packet {
                    relay: addr,
                    peer,
                    data: buf[..n].to_vec(),
                };
                if inbox.send(packet).is_err() {
                    return;
                }
            }
        })
    };
    Ok(Relay {
        addr,
        local,
        out,
        client,
        conn,
        tasks: vec![writer, reader],
    })
}

#[cfg(test)]
pub mod testing {
    //! A TURN server on loopback, the `turn` crate's own, with one fixed user.
    use super::*;
    use turn::auth::{generate_auth_key, AuthHandler};
    use turn::relay::relay_static::RelayAddressGeneratorStatic;
    use turn::server::config::{ConnConfig, ServerConfig};
    use turn::server::Server;
    use webrtc_util::vnet::net::Net;

    pub const REALM: &str = "fake.turn";
    pub const USER: &str = "owner";
    pub const PASS: &str = "s3cret";

    struct OneUser;

    impl AuthHandler for OneUser {
        fn auth_handle(
            &self,
            username: &str,
            realm: &str,
            _src: SocketAddr,
        ) -> std::result::Result<Vec<u8>, turn::Error> {
            if username == USER {
                Ok(generate_auth_key(USER, realm, PASS))
            } else {
                Err(turn::Error::ErrNoSuchUser)
            }
        }
    }

    /// The server keeps running while the returned value lives; `closed` reports each allocation it released.
    pub struct Fake {
        pub addr: SocketAddr,
        pub closed: mpsc::Receiver<turn::allocation::AllocationInfo>,
        _server: Server,
    }

    pub async fn fake() -> Fake {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let (tx, closed) = mpsc::channel(8);
        let server = Server::new(ServerConfig {
            conn_configs: vec![ConnConfig {
                conn: Arc::new(socket),
                relay_addr_generator: Box::new(RelayAddressGeneratorStatic {
                    relay_address: "127.0.0.1".parse().unwrap(),
                    address: "127.0.0.1".into(),
                    net: Arc::new(Net::new(None)),
                }),
            }],
            realm: REALM.into(),
            auth_handler: Arc::new(OneUser),
            channel_bind_timeout: Duration::from_secs(60),
            alloc_close_notify: Some(tx),
        })
        .await
        .unwrap();
        Fake {
            addr,
            closed,
            _server: server,
        }
    }

    pub fn credentials(fake: &Fake, password: &str) -> Credentials {
        Credentials {
            server: fake.addr.to_string(),
            username: USER.into(),
            password: password.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn credentials_never_print_the_password() {
        let c = Credentials {
            server: "turn.example:3478".into(),
            username: "u".into(),
            password: "hunter2hunter2".into(),
        };
        assert!(!format!("{c:?}").contains("hunter2"));
    }

    #[tokio::test]
    async fn turn_relays_both_ways_and_releases_on_drop() {
        let mut fake = fake().await;
        let (tx, mut inbox) = mpsc::unbounded_channel();
        let relay = allocate(&credentials(&fake, PASS), tx).await.unwrap();
        assert!(relay.addr().ip().is_loopback());

        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut buf = [0u8; 64];
        // Sending to the peer opens the permission its reply needs.
        relay.send(peer.local_addr().unwrap(), b"ping");
        let (n, from) = tokio::time::timeout(Duration::from_secs(2), peer.recv_from(&mut buf))
            .await
            .expect("the peer gets the relayed datagram")
            .unwrap();
        assert_eq!(&buf[..n], b"ping");
        assert_eq!(from, relay.addr());

        peer.send_to(b"pong", relay.addr()).await.unwrap();
        let got = tokio::time::timeout(Duration::from_secs(2), inbox.recv())
            .await
            .expect("the relay hands over the peer's reply")
            .unwrap();
        assert_eq!(got.data, b"pong");
        assert_eq!(got.peer, peer.local_addr().unwrap());
        assert_eq!(got.relay, relay.addr());

        drop(relay);
        tokio::time::timeout(Duration::from_secs(3), fake.closed.recv())
            .await
            .expect("dropping the relay releases the allocation")
            .unwrap();
    }

    #[tokio::test]
    async fn turn_rejected_credentials_give_an_error_and_no_relay() {
        let fake = fake().await;
        let (tx, _inbox) = mpsc::unbounded_channel();
        let wrong = credentials(&fake, "not-the-password");
        assert!(allocate(&wrong, tx.clone()).await.is_err());
        assert!(allocate_or_none(&wrong, tx).await.is_none());
    }

    #[tokio::test]
    async fn turn_server_that_never_answers_times_out() {
        // A socket nobody reads from.
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (tx, _inbox) = mpsc::unbounded_channel();
        let creds = Credentials {
            server: silent.local_addr().unwrap().to_string(),
            username: USER.into(),
            password: PASS.into(),
        };
        assert!(allocate(&creds, tx).await.is_err());
    }
}
