use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneBot11Config {
    pub network: NetworkConfig,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub music_sign_url: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub enable_local_file2_url: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub parse_mult_msg: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub http_servers: Vec<HttpServer>,
    pub http_clients: Vec<HttpClient>,
    pub websocket_servers: Vec<WebSocketServer>,
    pub websocket_clients: Vec<WebSocketClient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServer {
    pub name: String,
    pub enable: bool,
    pub port: u16,
    pub host: String,
    pub enable_cors: bool,
    pub enable_websocket: bool,
    pub message_post_format: String,
    pub token: String,
    pub debug: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpClient {
    pub name: String,
    pub enable: bool,
    pub url: String,
    pub message_post_format: String,
    pub report_self_message: bool,
    pub token: String,
    pub debug: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketServer {
    pub name: String,
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub message_post_format: String,
    pub report_self_message: bool,
    pub token: String,
    pub enable_force_push_event: bool,
    pub debug: bool,
    pub heart_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketClient {
    pub name: String,
    pub enable: bool,
    pub url: String,
    pub message_post_format: String,
    pub report_self_message: bool,
    pub reconnect_interval: u64,
    pub token: String,
    pub debug: bool,
    pub heart_interval: u64,
}

impl OneBot11Config {
    pub fn with_ws_server(port: u16) -> Self {
        Self {
            network: NetworkConfig {
                http_servers: vec![],
                http_clients: vec![],
                websocket_servers: vec![WebSocketServer {
                    name: "qqbot-ws".to_string(),
                    enable: true,
                    host: "0.0.0.0".to_string(),
                    port,
                    message_post_format: "array".to_string(),
                    report_self_message: false,
                    token: String::new(),
                    enable_force_push_event: true,
                    debug: false,
                    heart_interval: 30000,
                }],
                websocket_clients: vec![],
            },
            music_sign_url: String::new(),
            enable_local_file2_url: false,
            parse_mult_msg: false,
        }
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}
