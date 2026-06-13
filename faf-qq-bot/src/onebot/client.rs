use crate::onebot::types::{Action, Event};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::{connect_async_tls_with_config, MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, warn};

const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Handle to the OneBot client. Used to send actions upstream.
#[derive(Clone)]
pub struct OneBotClient {
    action_tx: mpsc::Sender<Action>,
}

impl OneBotClient {
    /// Send an action upstream. Returns an error if the connection is lost.
    pub async fn send(&self, action: Action) -> Result<()> {
        self.action_tx
            .send(action)
            .await
            .context("OneBot send channel closed; connection may be lost")?;
        Ok(())
    }
}

/// Connect to a OneBot 11 WebSocket endpoint and spawn read/write tasks.
///
/// Returns a receiver of incoming events and a client handle for sending actions.
/// The tasks will automatically reconnect on transient failures.
pub async fn connect(
    ws_url: impl Into<String>,
    access_token: impl Into<String>,
) -> Result<(mpsc::Receiver<Event>, OneBotClient)> {
    let ws_url = ws_url.into();
    let access_token = access_token.into();

    let (event_tx, event_rx) = mpsc::channel::<Event>(256);
    let (action_tx, action_rx) = mpsc::channel::<Action>(256);

    tokio::spawn(connection_loop(ws_url, access_token, event_tx, action_rx));

    Ok((event_rx, OneBotClient { action_tx }))
}

async fn connection_loop(
    ws_url: String,
    access_token: String,
    event_tx: mpsc::Sender<Event>,
    mut action_rx: mpsc::Receiver<Action>,
) {
    let mut reconnect_delay = RECONNECT_DELAY;

    loop {
        match connect_once(&ws_url, &access_token).await {
            Ok((ws_stream, _)) => {
                info!("connected to OneBot WebSocket");
                reconnect_delay = RECONNECT_DELAY;

                let (mut write, mut read) = ws_stream.split();

                let read_fut = async {
                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(WsMessage::Text(text)) => {
                                debug!(%text, "received websocket text");
                                match serde_json::from_str::<Event>(&text) {
                                    Ok(event) => {
                                        if event_tx.send(event).await.is_err() {
                                            warn!("event receiver dropped; stopping read loop");
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        debug!(error = %e, text = %text, "failed to parse OneBot event");
                                    }
                                }
                            }
                            Ok(WsMessage::Close(_)) => {
                                warn!("websocket closed by server");
                                break;
                            }
                            Ok(_) => {}
                            Err(e) => {
                                error!(error = %e, "websocket read error");
                                break;
                            }
                        }
                    }
                };

                let write_fut = async {
                    while let Some(action) = action_rx.recv().await {
                        let payload = match serde_json::to_string(&action) {
                            Ok(p) => p,
                            Err(e) => {
                                error!(error = %e, "failed to serialize action");
                                continue;
                            }
                        };
                        debug!(%payload, "sending action");
                        if let Err(e) =
                            timeout(SEND_TIMEOUT, write.send(WsMessage::Text(payload))).await
                        {
                            error!(error = %e, "failed to send action");
                            break;
                        }
                    }
                };

                tokio::select! {
                    _ = read_fut => {}
                    _ = write_fut => {}
                }

                warn!("websocket connection lost; reconnecting...");
            }
            Err(e) => {
                error!(error = %e, "failed to connect to OneBot WebSocket");
            }
        }

        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = std::cmp::min(reconnect_delay * 2, Duration::from_secs(60));
    }
}

async fn connect_once(
    ws_url: &str,
    access_token: &str,
) -> Result<(
    WebSocketStream<MaybeTlsStream<TcpStream>>,
    tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
)> {
    let mut request = ws_url
        .into_client_request()
        .context("invalid OneBot WebSocket URL")?;

    if !access_token.is_empty() {
        let auth_value = format!("Bearer {access_token}");
        request
            .headers_mut()
            .insert("Authorization", auth_value.parse()?);
    }

    let (stream, response) =
        connect_async_tls_with_config(request, None, false, None::<tokio_tungstenite::Connector>)
            .await
            .context("websocket handshake failed")?;

    Ok((stream, response))
}
