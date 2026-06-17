use std::path::Path;

use anyhow::{Context, Result};
use brain::control::{ControlRequest, ControlResponse, GroupRuntimeStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{timeout, Duration};

use crate::service::run_dir;

const SOCKET_NAME: &str = "qqbot-core.sock";
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

/// Resolve the control socket path used by the running `qqbot-core` process.
#[cfg(unix)]
fn socket_path(data_dir: &Path) -> std::path::PathBuf {
    run_dir(data_dir).join(SOCKET_NAME)
}

/// Send a control request and read the response.
#[cfg(unix)]
async fn send_request(data_dir: &Path, request: ControlRequest) -> Result<ControlResponse> {
    let path = socket_path(data_dir);
    if !path.exists() {
        anyhow::bail!("control socket not found; is qqbot-core running?");
    }

    let mut stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("failed to connect to {}", path.display()))?;

    let bytes = serde_json::to_vec(&request).context("failed to serialize control request")?;

    timeout(CONTROL_TIMEOUT, stream.write_all(&bytes))
        .await
        .context("control request timed out")?
        .context("failed to write control request")?;

    // Read the full response. The server writes one compact JSON object and
    // then drops the connection, so reading until EOF is reliable.
    let mut buf = Vec::new();
    timeout(CONTROL_TIMEOUT, stream.read_to_end(&mut buf))
        .await
        .context("control response timed out")?
        .context("failed to read control response")?;

    serde_json::from_slice::<ControlResponse>(&buf)
        .with_context(|| format!("invalid control response: {}", String::from_utf8_lossy(&buf)))
}

/// Ask the running core for the names of the tools it has loaded.
///
/// On non-Unix platforms or if the core is not running, this returns an error
/// so callers can fall back to inspecting the plugin directory.
pub async fn list_runtime_tools(data_dir: &Path) -> Result<Vec<String>> {
    #[cfg(not(unix))]
    {
        let _ = data_dir;
        anyhow::bail!("runtime tool listing is only supported on Unix");
    }

    #[cfg(unix)]
    match send_request(data_dir, ControlRequest::ListTools).await? {
        ControlResponse::Tools { names } => Ok(names),
        ControlResponse::Error { message } => anyhow::bail!("core reported error: {message}"),
        _ => anyhow::bail!("unexpected control response"),
    }
}

/// Ask the running core for per-group runtime status.
///
/// On non-Unix platforms or if the core is not running, this returns an error.
pub async fn group_status(data_dir: &Path) -> Result<Vec<GroupRuntimeStatus>> {
    #[cfg(not(unix))]
    {
        let _ = data_dir;
        anyhow::bail!("group status is only supported on Unix");
    }

    #[cfg(unix)]
    match send_request(data_dir, ControlRequest::GroupStatus).await? {
        ControlResponse::Groups { groups } => Ok(groups),
        ControlResponse::Error { message } => anyhow::bail!("core reported error: {message}"),
        _ => anyhow::bail!("unexpected control response"),
    }
}
