use std::path::Path;

use anyhow::{Context, Result};
use brain::control::{ControlRequest, ControlResponse};
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
    {
        let path = socket_path(data_dir);
        if !path.exists() {
            anyhow::bail!("control socket not found; is qqbot-core running?");
        }

        let mut stream = UnixStream::connect(&path)
            .await
            .with_context(|| format!("failed to connect to {}", path.display()))?;

        let request = ControlRequest::ListTools;
        let bytes = serde_json::to_vec(&request).context("failed to serialize control request")?;

        timeout(CONTROL_TIMEOUT, stream.write_all(&bytes))
            .await
            .context("control request timed out")?
            .context("failed to write control request")?;

        let mut buf = [0u8; 4096];
        let n = timeout(CONTROL_TIMEOUT, stream.read(&mut buf))
            .await
            .context("control response timed out")?
            .context("failed to read control response")?;

        match serde_json::from_slice::<ControlResponse>(&buf[..n]) {
            Ok(ControlResponse::Tools { names }) => Ok(names),
            Ok(ControlResponse::Error { message }) => {
                anyhow::bail!("core reported error: {message}")
            }
            Ok(ControlResponse::Pong) => anyhow::bail!("unexpected pong response"),
            Err(e) => anyhow::bail!("invalid control response: {e}"),
        }
    }
}
