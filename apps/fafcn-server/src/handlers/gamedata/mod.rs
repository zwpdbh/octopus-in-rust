//! Gamedata mirror routes: manifest/status reads and token-gated uploads,
//! parameterized by sync channel (gamedata, map-generator).
//!
//! File downloads themselves are served by `ServeDir`s mounted in
//! `crate::routes`; this module implements the JSON API plus the patched
//! client-binary download.

mod store;

pub use store::GamedataStore;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap},
    response::IntoResponse,
    Json,
};
use fafcn_gamedata::{
    ChannelStatus, EmbeddedConfig, Manifest, StatusResponse, UploadCheckRequest,
    UploadCheckResponse, UploadCommitRequest, CHANNELS,
};

use crate::{
    error::{Error, Result},
    state::AppState,
};

/// Validate a channel path parameter against the known channels.
fn known_channel(channel: &str) -> Result<String> {
    if CHANNELS.contains(&channel) {
        Ok(channel.to_string())
    } else {
        Err(Error::NotFound)
    }
}

/// `GET /api/gamedata/channels/:channel/manifest.json`.
pub async fn get_manifest(
    State(state): State<AppState>,
    Path(channel): Path<String>,
) -> Result<Json<Manifest>> {
    let channel = known_channel(&channel)?;
    let store = state.gamedata.clone();
    let manifest = tokio::task::spawn_blocking(move || store.read_manifest(&channel))
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    manifest.map(Json).ok_or(Error::NotFound)
}

/// `GET /api/gamedata/status` — per-channel mirror state for the web page.
pub async fn get_status(State(state): State<AppState>) -> Result<Json<StatusResponse>> {
    let store = state.gamedata.clone();
    let client_dir = state.gamedata_client_dir.clone();
    let (channels, client_tag) = tokio::task::spawn_blocking(move || {
        let mut channels = Vec::new();
        for channel in CHANNELS {
            channels.push(ChannelStatus {
                name: channel.to_string(),
                manifest: store.read_manifest(channel)?.map(|m| m.summary()),
            });
        }
        let tag = std::fs::read_to_string(client_dir.join("VERSION"))
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        Ok::<_, Error>((channels, tag))
    })
    .await
    .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    Ok(Json(StatusResponse {
        channels,
        client_tag,
    }))
}

/// `POST /api/gamedata/channels/:channel/upload/check` — which of the listed
/// files the server still needs. Requires the upload token.
pub async fn upload_check(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    headers: HeaderMap,
    Json(req): Json<UploadCheckRequest>,
) -> Result<Json<UploadCheckResponse>> {
    state.gamedata.authorize(&headers)?;
    let channel = known_channel(&channel)?;
    let store = state.gamedata.clone();
    let needed = tokio::task::spawn_blocking(move || store.check_needed(&channel, &req.files))
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    Ok(Json(UploadCheckResponse { needed }))
}

/// Header carrying the relative path of an uploaded file.
pub const PATH_HEADER: &str = "x-gamedata-path";
/// Header carrying the expected sha256 of an uploaded file.
pub const SHA256_HEADER: &str = "x-gamedata-sha256";

/// `POST /api/gamedata/channels/:channel/upload/file` — store one file.
/// Requires the upload token plus `x-gamedata-path` / `x-gamedata-sha256`
/// headers; body is the raw file content.
pub async fn upload_file(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<()> {
    state.gamedata.authorize(&headers)?;
    let channel = known_channel(&channel)?;
    let rel_path = required_header(&headers, PATH_HEADER)?.to_string();
    let sha256 = required_header(&headers, SHA256_HEADER)?.to_string();
    let size = body.len();
    let log_path = rel_path.clone();
    let store = state.gamedata.clone();
    tokio::task::spawn_blocking(move || store.store_upload(&channel, &rel_path, &sha256, &body))
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    tracing::info!(path = %log_path, size, "gamedata file uploaded");
    Ok(())
}

/// `POST /api/gamedata/channels/:channel/upload/commit` — verify all files
/// and publish a new manifest. Requires the upload token.
pub async fn upload_commit(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    headers: HeaderMap,
    Json(req): Json<UploadCommitRequest>,
) -> Result<Json<Manifest>> {
    state.gamedata.authorize(&headers)?;
    let channel = known_channel(&channel)?;
    let log_channel = channel.clone();
    let store = state.gamedata.clone();
    let manifest = tokio::task::spawn_blocking(move || store.commit(&channel, &req))
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    tracing::info!(
        channel = %log_channel,
        patch_version = %manifest.patch_version,
        uploader = %manifest.uploader,
        files = manifest.files.len(),
        "gamedata manifest committed"
    );
    Ok(Json(manifest))
}

/// `GET /api/gamedata/client/:filename` — serve a sync client binary with
/// the mirror's own address embedded (see `fafcn_gamedata::overlay`), so the
/// player never has to type the server URL.
pub async fn download_client(
    State(state): State<AppState>,
    Path(filename): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    // Allow simple file names only — no path traversal.
    if filename.is_empty()
        || !filename
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(Error::BadRequest("invalid file name".to_string()));
    }
    let path = state.gamedata_client_dir.join(&filename);
    let bytes = tokio::fs::read(&path).await.map_err(|_| Error::NotFound)?;

    let config = EmbeddedConfig {
        server: Some(request_origin(&headers)),
    };
    let patched = fafcn_gamedata::append_config(&bytes, &config)
        .map_err(|e| Error::Internal(format!("failed to embed config: {e}")))?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
            // The file on disk is replaced by `xtask fafcn file-sync` at any
            // time; never let a browser serve a stale cached build.
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        patched,
    ))
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::BadRequest(format!("missing or invalid header: {name}")))
}

/// Best-effort public origin of this server for one request:
/// `X-Forwarded-Proto` (reverse proxy) + `Host`.
fn request_origin(headers: &HeaderMap) -> String {
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{proto}://{host}")
}
