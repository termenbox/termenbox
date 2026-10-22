use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State,
    },
    http::HeaderMap,
};

use crate::link::{Inbound, LANE_TERM, pair};

const PING_EVERY: Duration = Duration::from_secs(20);
/// Queued terminal chunks are joined into frames of about this size.
const TERM_FRAME: usize = 16 * 1024;

/// WebSocket adapter: pumps the socket into a `Link`. Used behind a tunnel and in tests.
pub async fn handle_websocket(
    mut socket: WebSocket,
    State(state): State<crate::AppState>,
    headers: HeaderMap,
) {
    // Behind the Cloudflare tunnel the peer address is the tunnel; the real client is in this header.
    let ip = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let (l, mut end) = pair(ip, None);
    let session = tokio::spawn(crate::connection::run(l, state));
    let mut ping = tokio::time::interval(PING_EVERY);

    loop {
        tokio::select! {
            biased;
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    if end.to_session.send(Inbound::Json(text)).await.is_err() { break; }
                }
                // Binary frames start with a lane tag; a tag this server does not know is ignored.
                Some(Ok(Message::Binary(frame))) => {
                    if frame.first() == Some(&LANE_TERM)
                        && end.to_session.send(Inbound::Term(frame[1..].to_vec())).await.is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
            out = end.from_session.recv() => match out {
                Some(text) => {
                    if socket.send(Message::Text(text)).await.is_err() { break; }
                }
                None => {
                    // The session ended (for example an auth error was just sent): close properly.
                    // Dropping the socket right away would reset the connection and the browser
                    // could lose the last message.
                    let _ = socket.send(Message::Close(None)).await;
                    let _ = tokio::time::timeout(Duration::from_secs(1), socket.recv()).await;
                    break;
                }
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new())).await.is_err() { break; }
            }
            // Last in the biased order: terminal bytes never go ahead of a waiting status message or a ping.
            term = end.term_from_session.recv() => match term {
                Some(mut frame) => {
                    // Under pressure the queued chunks travel as one frame.
                    while frame.len() < TERM_FRAME {
                        match end.term_from_session.try_recv() {
                            Ok(more) => frame.extend(more),
                            Err(_) => break,
                        }
                    }
                    let mut tagged = Vec::with_capacity(frame.len() + 1);
                    tagged.push(LANE_TERM);
                    tagged.extend(frame);
                    if socket.send(Message::Binary(tagged)).await.is_err() { break; }
                }
                None => break,
            },
        }
    }
    drop(end);
    let _ = session.await;
}
