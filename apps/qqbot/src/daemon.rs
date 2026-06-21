use crate::service::{logs_dir, run_dir};
use anyhow::{Context, Result};
use daemonize::Daemonize;
use std::fs::File;
use std::path::Path;
use std::time::Duration;

/// Daemonize the current process and, in the child process, continue execution.
/// In the parent process this exits the process.
pub fn start(data_dir: &Path) -> Result<()> {
    let run = run_dir(data_dir);
    let logs = logs_dir(data_dir);
    std::fs::create_dir_all(&run)?;
    std::fs::create_dir_all(&logs)?;

    let stdout = File::create(logs.join("supervisor.log"))
        .context("failed to create supervisor stdout log")?;
    let stderr = stdout
        .try_clone()
        .context("failed to clone supervisor stderr log")?;
    let pid_file = run.join("qqbot.pid");
    let working_dir = std::env::current_dir().context("failed to get current directory")?;

    let daemonize = Daemonize::new()
        .pid_file(&pid_file)
        .working_directory(working_dir)
        .stdout(stdout)
        .stderr(stderr);

    // daemonize::Daemonize::start exits the parent process and returns in the child.
    daemonize
        .start()
        .context("failed to daemonize qqbot supervisor")?;

    Ok(())
}

/// Read the daemon PID from the pid file, if present.
pub fn pid(data_dir: &Path) -> Option<libc::pid_t> {
    let pid_file = run_dir(data_dir).join("qqbot.pid");
    let contents = std::fs::read_to_string(pid_file).ok()?;
    contents.trim().parse().ok()
}

/// Send SIGTERM to the daemon and wait for the pid file to disappear.
pub async fn stop(data_dir: &Path) -> Result<()> {
    let pid_file = run_dir(data_dir).join("qqbot.pid");
    let pid = match pid(data_dir) {
        Some(p) => p,
        None => {
            anyhow::bail!("daemon pid file not found; is qqbot running?");
        }
    };

    // Send SIGTERM.
    unsafe {
        if libc::kill(pid, libc::SIGTERM) != 0 {
            anyhow::bail!("failed to send SIGTERM to daemon pid {pid}");
        }
    }

    // Wait for pid file to disappear (with timeout).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while pid_file.exists() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    if pid_file.exists() {
        anyhow::bail!("daemon did not shut down within timeout");
    }

    Ok(())
}

/// Check whether the pid in the pid file refers to a live process.
pub fn is_alive(data_dir: &Path) -> bool {
    match pid(data_dir) {
        Some(pid) => unsafe { libc::kill(pid, 0) == 0 },
        None => false,
    }
}
