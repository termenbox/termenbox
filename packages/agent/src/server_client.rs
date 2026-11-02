use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error, info};
use url::Url;

use crate::pty::PtyManager;
use crate::tmux::TmuxManager;
use termenbox_core::protocol::{ClientMessage, ServerMessage};

pub struct ServerClient {
    server_url: String,
    server_token: String,
    pty_manager: Arc<PtyManager>,
    _tmux_manager: Arc<TmuxManager>,
    reconnect_delay: Duration,
}

impl ServerClient {
    pub fn new(
        server_url: String,
        server_token: String,
        pty_manager: Arc<PtyManager>,
        tmux_manager: Arc<TmuxManager>,
    ) -> Self {
        Self {
            server_url,
            server_token,
            pty_manager,
            _tmux_manager: tmux_manager,
            reconnect_delay: Duration::from_secs(5),
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            info!("Connecting to server: {}", self.server_url);
            match self.connect().await {
                Ok(_) => info!("Disconnected from server"),
                Err(e) => error!("Connection error: {}", e),
            }

            info!("Reconnecting in {:?}...", self.reconnect_delay);
            tokio::time::sleep(self.reconnect_delay).await;
            self.reconnect_delay = std::cmp::min(self.reconnect_delay * 2, Duration::from_secs(60));
        }
    }

    async fn connect(&self) -> Result<()> {
        let url = Url::parse(&self.server_url)?;
        let (ws_stream, _) = connect_async(url).await?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        let auth_msg = ClientMessage::Auth {
            // The parked agent has no device key (issue #19); it cannot pass the certificate handshake.
            cert: String::new(),
            signature: String::new(),
            server_token: self.server_token.clone(),
            channels: false,
        };
        ws_tx
            .send(Message::Text(serde_json::to_string(&auth_msg)?))
            .await?;

        let (pty_tx, mut pty_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let mut current_workspace: Option<String> = None;

        loop {
            tokio::select! {
                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            debug!("Received from server: {}", text);
                            if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                                if let Err(e) = self.handle_server_message(server_msg, &mut current_workspace, &mut ws_tx, &pty_tx).await {
                                    error!("Error handling server message: {}", e);
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("Server closed connection");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                data = pty_rx.recv() => {
                    if let Some(output) = data {
                        let encoded = BASE64.encode(&output);
                        let msg = ClientMessage::Input { data: encoded };
                        if ws_tx.send(Message::Text(serde_json::to_string(&msg)?)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_server_message(
        &self,
        msg: ServerMessage,
        current_workspace: &mut Option<String>,
        ws_tx: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        _pty_tx: &mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<()> {
        match msg {
            ServerMessage::AuthOk {
                user, server_name, ..
            } => {
                info!("Authenticated as {} on {}", user, server_name);
                let msg = ClientMessage::ListWorkspaces;
                ws_tx
                    .send(Message::Text(serde_json::to_string(&msg)?))
                    .await?;
            }
            ServerMessage::Challenge { .. }
            | ServerMessage::DeviceRevoked { .. }
            | ServerMessage::UninstallDone { .. }
            | ServerMessage::UpdateRequested { .. }
            | ServerMessage::UpdateStatus { .. }
            | ServerMessage::UpdateCheckRequested {}
            | ServerMessage::UpdateState { .. }
            | ServerMessage::Candidates { .. }
            | ServerMessage::DirListing { .. }
            | ServerMessage::DirError { .. }
            | ServerMessage::UploadReady { .. }
            | ServerMessage::UploadProgress { .. }
            | ServerMessage::UploadDone { .. }
            | ServerMessage::UploadDenied { .. }
            | ServerMessage::UploadFailed { .. }
            | ServerMessage::UploadCancelled { .. }
            | ServerMessage::ConductorTerminalOpened { .. }
            | ServerMessage::ConductorTerminalClosed { .. }
            | ServerMessage::StatusFrame { .. } => {}
            ServerMessage::Workspaces { workspaces } => {
                info!("Received {} workspaces", workspaces.len());
                for workspace in workspaces {
                    debug!("Workspace: {} ({})", workspace.name, workspace.id);
                }
            }
            ServerMessage::WorkspaceAttached { workspace_id, .. } => {
                info!("Attached to workspace: {}", workspace_id);
                *current_workspace = Some(workspace_id.clone());

                let (_tx, _rx): (
                    mpsc::UnboundedSender<Vec<u8>>,
                    mpsc::UnboundedReceiver<Vec<u8>>,
                ) = mpsc::unbounded_channel();
                let _ = self.pty_manager.attach(&workspace_id).await;
            }
            ServerMessage::Output { data: _ } => {
                // Output from server to terminal - not used in agent
            }
            ServerMessage::AgentEvent {
                event,
                agent,
                workspace_id,
            } => {
                info!("Agent event: {} - {} - {}", event, agent, workspace_id);
            }
            ServerMessage::AgentSnapshot { .. }
            | ServerMessage::AgentEvents { .. }
            | ServerMessage::Activity { .. }
            | ServerMessage::Attention { .. }
            | ServerMessage::PushInfo { .. }
            | ServerMessage::PushSettings { .. }
            | ServerMessage::PushSettingsRejected { .. }
            | ServerMessage::Snapshot { .. }
            | ServerMessage::WindowList { .. }
            | ServerMessage::Scrollback { .. }
            | ServerMessage::SessionView { .. }
            | ServerMessage::AgentHooks { .. }
            | ServerMessage::AcpView(_)
            | ServerMessage::History { .. }
            | ServerMessage::HistoryDeleted { .. }
            | ServerMessage::SearchHit { .. }
            | ServerMessage::SearchDone { .. }
            | ServerMessage::SessionResumed { .. }
            | ServerMessage::SessionWindow { .. }
            | ServerMessage::ServerHealth { .. }
            | ServerMessage::HealthHistory { .. }
            | ServerMessage::PreviousExit { .. }
            | ServerMessage::DiagnosticsBundle { .. }
            | ServerMessage::ClaimPreview { .. }
            | ServerMessage::TabStates { .. }
            | ServerMessage::Unknown => {}
            ServerMessage::Error { message } => {
                info!("Server error: {}", message);
            }
            ServerMessage::Pong
            | ServerMessage::ControlAck { .. }
            | ServerMessage::AcpEvents { .. }
            | ServerMessage::ConductorState { .. }
            | ServerMessage::AttentionStats { .. } => {}
        }
        Ok(())
    }
}
