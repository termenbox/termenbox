use std::time::Duration;

use async_trait::async_trait;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State,
    },
    http::HeaderMap,
};

use crate::link::{self, Inbound, LinkEnd, LANE_TERM, LANE_UPLOAD};

const PING_EVERY: Duration = Duration::from_secs(20);
/// Queued terminal chunks are joined into frames of about this size. It is also the longest a control message waits
/// behind bulk traffic on this one TCP stream: the frame being written (guardrail `control-never-waits-behind-bulk`).
pub(crate) const TERM_FRAME: usize = 16 * 1024;

/// What the pump reads from a socket.
pub(crate) enum Rx {
    Text(String),
    Term(Vec<u8>),
    Upload(Vec<u8>),
    /// The peer is gone.
    Closed,
    /// A frame that means nothing here (a pong, a binary frame of an unknown lane).
    Other,
}

/// What the pump writes to a socket.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum Tx {
    Text(String),
    Term(Vec<u8>),
    Ping,
    /// A proper close; the peer may still send its own.
    Close,
}

/// The socket as the pump sees it, so the order of what it writes can be tested without a network.
#[async_trait]
pub(crate) trait Socket: Send {
    async fn read(&mut self) -> Rx;
    /// False when the write failed.
    async fn write(&mut self, frame: Tx) -> bool;
    /// Waits briefly for the peer's answer to a close.
    async fn linger(&mut self);
}

#[async_trait]
impl Socket for WebSocket {
    async fn read(&mut self) -> Rx {
        match self.recv().await {
            Some(Ok(Message::Text(text))) => Rx::Text(text),
            // Binary frames start with a lane tag; a tag this server does not know is ignored.
            Some(Ok(Message::Binary(frame))) if frame.first() == Some(&LANE_TERM) => {
                Rx::Term(frame[1..].to_vec())
            }
            Some(Ok(Message::Binary(frame))) if frame.first() == Some(&LANE_UPLOAD) => {
                Rx::Upload(frame[1..].to_vec())
            }
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => Rx::Closed,
            _ => Rx::Other,
        }
    }

    async fn write(&mut self, frame: Tx) -> bool {
        let m = match frame {
            Tx::Text(t) => Message::Text(t),
            Tx::Term(b) => Message::Binary(b),
            Tx::Ping => Message::Ping(Vec::new()),
            Tx::Close => Message::Close(None),
        };
        self.send(m).await.is_ok()
    }

    async fn linger(&mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.recv()).await;
    }
}

/// WebSocket adapter: pumps the socket into a `Link`. Used behind a tunnel and in tests.
pub async fn handle_websocket(
    socket: WebSocket,
    State(state): State<crate::AppState>,
    headers: HeaderMap,
) {
    // Behind the Cloudflare tunnel the peer address is the tunnel; the real client is in this header.
    let ip = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let (l, end) = link::pair(ip, None);
    let workspace = tokio::spawn(crate::connection::run(l, state));
    pump(socket, end, PING_EVERY).await;
    let _ = workspace.await;
}

/// Moves frames between a socket and a link. What goes out is chosen in strict order: control replies, then status
/// messages, a ping, and terminal bytes last, so on this one TCP stream a control reply waits at most for the frame
/// being written (at most `TERM_FRAME` bytes of terminal output, or one status message).
pub(crate) async fn pump<S: Socket>(mut socket: S, mut end: LinkEnd, ping_every: Duration) {
    let mut ping = tokio::time::interval(ping_every);
    let mut control_open = true;

    loop {
        tokio::select! {
            biased;
            msg = socket.read() => match msg {
                Rx::Text(text) => {
                    if end.to_workspace.send(Inbound::Json(text)).await.is_err() { break; }
                }
                Rx::Term(bytes) => {
                    if end.to_workspace.send(Inbound::Term(bytes)).await.is_err() { break; }
                }
                Rx::Upload(bytes) => {
                    if end.to_workspace.send(Inbound::Upload(bytes)).await.is_err() { break; }
                }
                Rx::Closed => break,
                Rx::Other => {}
            },
            // Control replies are never behind anything else.
            out = end.control_from_workspace.recv(), if control_open => match out {
                Some(text) => {
                    if !socket.write(Tx::Text(text)).await { break; }
                }
                // Closed and drained. The status queue may still hold the last message (an error before the close),
                // so the pump ends there, not here.
                None => control_open = false,
            },
            out = end.from_workspace.recv() => match out {
                Some(text) => {
                    if !socket.write(Tx::Text(text)).await { break; }
                }
                None => {
                    // The workspace ended (for example an auth error was just sent): close properly.
                    // Dropping the socket right away would reset the connection and the browser
                    // could lose the last message.
                    close_gracefully(&mut socket).await;
                    break;
                }
            },
            _ = ping.tick() => {
                if !socket.write(Tx::Ping).await { break; }
            }
            // Last in the biased order: terminal bytes never go ahead of a waiting control or status message or a ping.
            term = end.term_from_workspace.recv() => match term {
                Some(mut frame) => {
                    // Under pressure the queued chunks travel as one frame.
                    while frame.len() < TERM_FRAME {
                        match end.term_from_workspace.try_recv() {
                            Ok(more) => frame.extend(more),
                            Err(_) => break,
                        }
                    }
                    let mut tagged = Vec::with_capacity(frame.len() + 1);
                    tagged.push(LANE_TERM);
                    tagged.extend(frame);
                    if !socket.write(Tx::Term(tagged)).await { break; }
                }
                None => break,
            },
        }
    }
}

async fn close_gracefully<S: Socket>(socket: &mut S) {
    let _ = socket.write(Tx::Close).await;
    socket.linger().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{pair, LinkTx};
    use std::sync::{Arc, Mutex};

    /// A socket that writes into a vector, reads nothing, and calls `on_write` with the number of frames written so
    /// far, so a test can queue a control message while the pump is busy with bulk traffic.
    struct Recorder {
        written: Arc<Mutex<Vec<Tx>>>,
        on_write: Option<Box<dyn FnMut(usize) -> Option<tokio::task::JoinHandle<()>> + Send>>,
    }

    #[async_trait]
    impl Socket for Recorder {
        async fn read(&mut self) -> Rx {
            std::future::pending().await
        }
        async fn write(&mut self, frame: Tx) -> bool {
            let n = {
                let mut w = self.written.lock().unwrap();
                w.push(frame);
                w.len()
            };
            if let Some(f) = self.on_write.as_mut() {
                if let Some(h) = f(n) {
                    let _ = h.await;
                }
            }
            true
        }
        async fn linger(&mut self) {}
    }

    fn fill(tx: &LinkTx) -> impl std::future::Future<Output = ()> + '_ {
        async move {
            for i in 0..40 {
                assert!(
                    tx.send(format!("{{\"type\":\"agent_events\",\"n\":{i}}}"))
                        .await
                );
            }
        }
    }

    #[tokio::test]
    async fn control_a_control_message_is_written_before_the_next_bulk_frame() {
        let (link, end) = pair(None, None);
        let tx = link.tx.clone();
        tx.enable_lanes();
        // The event queue and the byte queue are both full of bulk frames.
        fill(&tx).await;
        for _ in 0..60 {
            tx.reserve_output()
                .await
                .unwrap()
                .send(vec![b'x'; 20 * 1024]);
        }
        let written = Arc::new(Mutex::new(Vec::new()));
        let queue_control = {
            let tx = tx.clone();
            Box::new(move |n: usize| {
                // While the pump writes its 3rd frame, a cancel's ack is queued.
                (n == 3).then(|| {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        assert!(
                            tx.send_control(r#"{"type":"control_ack","req":1,"ok":true}"#.into())
                                .await
                        );
                    })
                })
            }) as Box<dyn FnMut(usize) -> Option<tokio::task::JoinHandle<()>> + Send>
        };
        let socket = Recorder {
            written: written.clone(),
            on_write: Some(queue_control),
        };
        let pump = tokio::spawn(pump(socket, end, Duration::from_secs(3600)));
        for _ in 0..200 {
            if written.lock().unwrap().len() >= 12 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        drop(link);
        pump.abort();
        let w = written.lock().unwrap().clone();
        let at = w
            .iter()
            .position(|f| matches!(f, Tx::Text(t) if t.contains("control_ack")))
            .expect("the ack was written");
        // Queued while frame 3 was being written: it is frame 4, ahead of every remaining event and terminal frame.
        assert_eq!(
            at,
            3,
            "{:?}",
            w.iter()
                .take(8)
                .map(|f| match f {
                    Tx::Text(t) => t.chars().take(30).collect(),
                    Tx::Term(b) => format!("term {}", b.len()),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<String>>()
        );
        let bulk_after = w.len() - at - 1;
        assert!(bulk_after > 0, "bulk frames were still waiting");
    }

    #[tokio::test]
    async fn control_status_still_goes_before_terminal_bytes_and_a_terminal_frame_is_bounded() {
        let (link, end) = pair(None, None);
        let tx = link.tx.clone();
        tx.enable_lanes();
        for _ in 0..10 {
            tx.reserve_output()
                .await
                .unwrap()
                .send(vec![b'x'; 40 * 1024]);
        }
        assert!(tx.send(r#"{"type":"pong"}"#.into()).await);
        let written = Arc::new(Mutex::new(Vec::new()));
        let socket = Recorder {
            written: written.clone(),
            on_write: None,
        };
        let pump = tokio::spawn(pump(socket, end, Duration::from_secs(3600)));
        for _ in 0..200 {
            if written.lock().unwrap().len() >= 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        pump.abort();
        let w = written.lock().unwrap().clone();
        assert_eq!(w[0], Tx::Text(r#"{"type":"pong"}"#.into()));
        // A queued chunk is never split, and chunks are only joined up to the frame size: a frame is one chunk here.
        // (The first tick of the ping timer is immediate; a ping is not terminal output.)
        assert!(w
            .iter()
            .skip(1)
            .filter(|f| **f != Tx::Ping)
            .all(|f| matches!(f, Tx::Term(b) if b.len() <= 40 * 1024 + TERM_FRAME + 1)));
    }
}
