pub mod types;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, warn};
use types::{Action, Event};

pub type EventRx = mpsc::Receiver<Event>;
pub type ActionTx = mpsc::UnboundedSender<Action>;

pub async fn connect(
    ws_url: impl Into<String>,
    access_token: impl Into<String>,
) -> Result<(EventRx, ActionTx)> {
    let ws_url = ws_url.into();
    let access_token = access_token.into();

    let (event_tx, event_rx) = mpsc::channel::<Event>(256);
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();

    tokio::spawn(async move {
        let mut backoff_secs = 5u64;
        loop {
            match connect_once(&ws_url, &access_token).await {
                Ok((mut ws_writer, mut ws_reader)) => {
                    info!("connected to OneBot WebSocket");
                    backoff_secs = 5;

                    loop {
                        tokio::select! {
                            Some(action) = action_rx.recv() => {
                                let payload = match serde_json::to_string(&action) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        error!(error = %e, "failed to serialize action");
                                        continue;
                                    }
                                };
                                if let Err(e) = ws_writer.send(Message::Text(payload)).await {
                                    error!(error = %e, "failed to send action");
                                    break;
                                }
                            }
                            msg = ws_reader.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        debug!(text = %text, "received OneBot message");
                                        match serde_json::from_str::<Event>(&text) {
                                            Ok(event) => {
                                                if event_tx.send(event).await.is_err() {
                                                    warn!("event receiver dropped; exiting connection loop");
                                                    return;
                                                }
                                            }
                                            Err(e) => {
                                                debug!(error = %e, text = %text, "failed to parse OneBot event");
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) | None => {
                                        warn!("OneBot WebSocket closed");
                                        break;
                                    }
                                    Some(Ok(_)) => {}
                                    Some(Err(e)) => {
                                        error!(error = %e, "OneBot WebSocket error");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "failed to connect to OneBot WebSocket");
                }
            }

            warn!(secs = backoff_secs, "reconnecting to OneBot WebSocket");
            tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(60);
        }
    });

    Ok((event_rx, action_tx))
}

async fn connect_once(
    ws_url: &str,
    access_token: &str,
) -> Result<(
    futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
)> {
    let parsed = url::Url::parse(ws_url).context("invalid OneBot WebSocket URL")?;
    let host = parsed.host_str().unwrap_or("localhost").to_string();
    let port = parsed.port_or_known_default().unwrap_or(3001);

    let request = format!("{}:{}", host, port);

    // Use the plain URL when no access token is required. This avoids scheme
    // handling issues in tokio-tungstenite when building an http::Request.
    let (ws_stream, _) = if access_token.is_empty() {
        tokio_tungstenite::connect_async(ws_url)
            .await
            .context("OneBot WebSocket handshake failed")?
    } else {
        let scheme = match parsed.scheme() {
            "ws" => "http",
            "wss" => "https",
            s => s,
        };
        let request_uri = format!(
            "{}://{}{}{}",
            scheme,
            request,
            parsed.path(),
            parsed
                .query()
                .map(|q| format!("?{}", q))
                .unwrap_or_default()
        );

        let req = http::Request::builder()
            .uri(&request_uri)
            .header("Host", &request)
            .header("Authorization", format!("Bearer {access_token}"))
            .body(())?;

        tokio_tungstenite::connect_async(req)
            .await
            .context("OneBot WebSocket handshake failed")?
    };

    Ok(ws_stream.split())
}
