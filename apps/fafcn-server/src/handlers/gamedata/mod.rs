//! Gamedata mirror routes: manifest/status reads and token-gated uploads.
//!
//! File downloads themselves are served by a `ServeDir` mounted in
//! `crate::routes`; this module implements the JSON API.

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
    EmbeddedConfig, Manifest, StatusResponse, UploadCheckRequest, UploadCheckResponse,
    UploadCommitRequest,
};

use crate::{
    error::{Error, Result},
    state::AppState,
};

/// `GET /api/gamedata/manifest.json` — the manifest clients diff against.
pub async fn get_manifest(State(state): State<AppState>) -> Result<Json<Manifest>> {
    let store = state.gamedata.clone();
    let manifest = tokio::task::spawn_blocking(move || store.read_manifest())
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    manifest.map(Json).ok_or(Error::NotFound)
}

/// `GET /api/gamedata/status` — abridged mirror state for the web page.
pub async fn get_status(State(state): State<AppState>) -> Result<Json<StatusResponse>> {
    let store = state.gamedata.clone();
    let client_dir = state.gamedata_client_dir.clone();
    let (manifest, client_tag) = tokio::task::spawn_blocking(move || {
        let manifest = store.read_manifest()?;
        let tag = std::fs::read_to_string(client_dir.join("VERSION"))
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        Ok::<_, Error>((manifest, tag))
    })
    .await
    .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    Ok(Json(StatusResponse {
        manifest: manifest.map(|m| m.summary()),
        client_tag,
    }))
}

/// `POST /api/gamedata/upload/check` — which of the listed files the server
/// still needs. Requires the upload token.
pub async fn upload_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UploadCheckRequest>,
) -> Result<Json<UploadCheckResponse>> {
    state.gamedata.authorize(&headers)?;
    let store = state.gamedata.clone();
    let needed = tokio::task::spawn_blocking(move || store.check_needed(&req.files))
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    Ok(Json(UploadCheckResponse { needed }))
}

/// Header carrying the relative path of an uploaded file.
pub const PATH_HEADER: &str = "x-gamedata-path";
/// Header carrying the expected sha256 of an uploaded file.
pub const SHA256_HEADER: &str = "x-gamedata-sha256";

/// `POST /api/gamedata/upload/file` — store one file. Requires the upload
/// token plus `x-gamedata-path` / `x-gamedata-sha256` headers; body is the
/// raw file content.
pub async fn upload_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<()> {
    state.gamedata.authorize(&headers)?;
    let rel_path = required_header(&headers, PATH_HEADER)?.to_string();
    let sha256 = required_header(&headers, SHA256_HEADER)?.to_string();
    let size = body.len();
    let log_path = rel_path.clone();
    let store = state.gamedata.clone();
    tokio::task::spawn_blocking(move || store.store_upload(&rel_path, &sha256, &body))
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    tracing::info!(path = %log_path, size, "gamedata file uploaded");
    Ok(())
}

/// `POST /api/gamedata/upload/commit` — verify all files and publish a new
/// manifest. Requires the upload token.
pub async fn upload_commit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UploadCommitRequest>,
) -> Result<Json<Manifest>> {
    state.gamedata.authorize(&headers)?;
    let store = state.gamedata.clone();
    let manifest = tokio::task::spawn_blocking(move || store.commit(&req))
        .await
        .map_err(|e| Error::Internal(format!("task join error: {e}")))??;
    tracing::info!(
        patch_version = %manifest.patch_version,
        uploader = %manifest.uploader,
        files = manifest.files.len(),
        "gamedata manifest committed"
    );
    Ok(Json(manifest))
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::BadRequest(format!("missing or invalid header: {name}")))
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
