use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use termenbox_core::protocol::{ClientMessage, ServerMessage};

use crate::link::{Inbound, Link};
use crate::AppState;

const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// The next message within the auth window; `None` on timeout, close, or unparseable input.
async fn next(link: &mut Link) -> Option<ClientMessage> {
    // Terminal bytes before the handshake is done are not a handshake.
    let Inbound::Json(text) = tokio::time::timeout(AUTH_TIMEOUT, link.rx.recv())
        .await
        .ok()??
    else {
        return None;
    };
    serde_json::from_str(&text).ok()
}

async fn send(link: &Link, msg: &ServerMessage) -> bool {
    link.tx.send(serde_json::to_string(msg).unwrap()).await
}

/// What a claim would add: the name, how workspaces run, as which OS user, and the version. Nothing secret.
fn claim_preview(state: &AppState) -> ServerMessage {
    let (mode, run_user) = split_mode(&state.mode);
    ServerMessage::ClaimPreview {
        name: state.server_name.clone(),
        mode,
        run_user,
        version: crate::version(),
    }
}

/// `container` -> (`container`, ""); `host:alice` -> (`host`, `alice`).
fn split_mode(label: &str) -> (String, String) {
    match label.split_once(':') {
        Some((mode, user)) => (mode.to_string(), user.to_string()),
        None => (label.to_string(), String::new()),
    }
}

/// One client connection from first byte to last, on any transport.
/// Browsers cannot set headers on a WebSocket and a data channel has none, so the handshake is in-band:
/// `hello`, a `challenge` carrying a fresh nonce and this server's id, then `auth` with the device
/// certificate and a signature over the challenge. Nothing else is read until that checks out and the
/// user owns this server. Returning drops the link, which makes the transport close the connection.
pub async fn run(mut link: Link, state: AppState) {
    let Some(ClientMessage::Hello { protocol: client }) = next(&mut link).await else {
        send(
            &link,
            &ServerMessage::Error {
                message: "Authenticate first".into(),
            },
        )
        .await;
        return;
    };
    // A client that announces no version is an older one: it is spoken to in the legacy vocabulary (legacy.rs).
    let protocol = client.min(termenbox_core::ext::PROTOCOL_VERSION);
    if protocol == 0 {
        link.tx.enable_legacy();
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let nonce = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = ServerMessage::Challenge {
        nonce: nonce.clone(),
        server_id: state.auth.server_id().to_string(),
    };
    if !send(&link, &challenge).await {
        return;
    }
    let (cert, signature, claim, lanes) = match next(&mut link).await {
        // Decision 0117: the dialog asks what it is about to add. One answer, then the connection ends; nothing is
        // bound and the token stays valid.
        Some(ClientMessage::ClaimPreview { server_token }) => {
            let reply = if state
                .auth
                .preview_claim(&server_token, link.peer_ip.as_deref())
            {
                claim_preview(&state)
            } else {
                ServerMessage::Error {
                    message: "This claim link is not valid (any more)".into(),
                }
            };
            send(&link, &reply).await;
            return;
        }
        Some(ClientMessage::Auth {
            cert,
            signature,
            server_token,
            channels,
        }) => (
            cert,
            signature,
            Some(server_token).filter(|t| !t.is_empty()),
            channels,
        ),
        Some(_) => {
            send(
                &link,
                &ServerMessage::Error {
                    message: "Authenticate first".into(),
                },
            )
            .await;
            return;
        }
        None => return,
    };

    let identity = match state.auth.check(
        &cert,
        &signature,
        &nonce,
        claim.as_deref(),
        link.peer_ip.as_deref(),
    ) {
        Ok(i) => i,
        Err(e) => {
            send(
                &link,
                &ServerMessage::Error {
                    message: e.message().into(),
                },
            )
            .await;
            return;
        }
    };

    tracing::info!("Authenticated {}", identity.login);
    let ok = ServerMessage::AuthOk {
        protocol,
        control_channel: protocol >= 1,
        user: identity.login.clone(),
        channels: if lanes { vec!["term".into()] } else { vec![] },
        tabs: protocol >= 2,
        server_name: state.server_name.clone(),
        mode: state.mode.clone(),
        version: crate::version(),
        backends: state.workspaces.backends(),
        control_mode: state.workspaces.control_mode(),
        tmux_version: state.workspaces.tmux_version().to_string(),
        prefix: state.workspaces.prefix().to_string(),
        acp_agents: state.workspaces.acp_agents(),
        update_available: state.workspaces.update_available(),
    };
    if !send(&link, &ok).await {
        return;
    }
    // The signed candidate bundle (decision 0123), in-band after authentication: the client verifies it against the
    // pinned server key and keeps it as its list of addresses to race.
    if let Some(bundle) = state.paths.get().and_then(|p| p.current_fresh()) {
        if !send(&link, &ServerMessage::Candidates { bundle }).await {
            return;
        }
    }
    // Queued behind `auth_ok`, so the client reads it before the first raw byte.
    if lanes {
        link.tx.enable_lanes();
    }

    if let Err(e) = state.workspaces.serve(link, identity, &state.auth).await {
        tracing::error!("Workspace error: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::split_mode;

    #[test]
    fn the_run_mode_label_splits_into_mode_and_run_user() {
        assert_eq!(split_mode("container"), ("container".into(), "".into()));
        assert_eq!(split_mode("host:alice"), ("host".into(), "alice".into()));
    }
}
