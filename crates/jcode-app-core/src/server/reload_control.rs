//! Private, owner-only W13 recovery control socket.
//!
//! This is deliberately not a registry or generic control plane. It exists so a
//! CLI can obtain a live, nonce-bound proof that the exact daemon serving one
//! primary socket installed a SIGUSR1 recovery handler before it signals that
//! PID. A stale registry entry or stale on-disk capability file cannot answer
//! the control request, so it cannot authorize a PID-reuse signal.

use crate::transport::{Listener, Stream};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const CONTROL_IO_TIMEOUT: Duration = Duration::from_millis(500);
const CONTROL_MAX_LINE_BYTES: usize = 1024;
const SIGNAL_MARKER_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Serialize, Deserialize)]
struct ReloadControlRequest {
    primary_socket: String,
    nonce: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReloadControlResponse {
    primary_socket: String,
    pid: u32,
    nonce: String,
    sigusr1_supported: bool,
}

pub(super) async fn run(listener: Listener, primary_socket: PathBuf) {
    use tokio::signal::unix::{SignalKind, signal};

    // Do not serve a positive capability unless this process has successfully
    // registered the exact signal handler the CLI will use.
    let Ok(mut sigusr1) = signal(SignalKind::user_defined1()) else {
        crate::logging::error(&format!(
            "W13 reload control disabled because SIGUSR1 registration failed for {}",
            primary_socket.display()
        ));
        return;
    };

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let socket = primary_socket.clone();
                    tokio::spawn(async move {
                        if let Err(error) = answer_capability(stream, socket).await {
                            crate::logging::warn(&format!("W13 reload control request failed: {error}"));
                        }
                    });
                }
                Err(error) => {
                    crate::logging::error(&format!("W13 reload control accept failed: {error}"));
                    return;
                }
            },
            received = sigusr1.recv() => {
                if received.is_none() {
                    crate::logging::error("W13 SIGUSR1 stream ended; disabling stalled recovery control");
                    return;
                }
                match crate::server::reserve_idle_stalled_recovery().await {
                    crate::server::IdleRecoveryReservation::Reserved => {
                        let recovery_id = crate::server::send_current_binary_reload_signal();
                        crate::logging::event_warn(
                            "SERVER_SIGUSR1_IDLE_RECOVERY_RESERVED",
                            vec![
                                ("recovery_id", recovery_id),
                                ("primary_socket", primary_socket.display().to_string()),
                                ("target", "current_executable_only".to_string()),
                            ],
                        );
                    }
                    outcome => crate::logging::event_warn(
                        "SERVER_SIGUSR1_IDLE_RECOVERY_SKIPPED",
                        vec![
                            ("primary_socket", primary_socket.display().to_string()),
                            ("outcome", format!("{outcome:?}")),
                        ],
                    ),
                }
            }
        }
    }
}

async fn answer_capability(stream: Stream, primary_socket: PathBuf) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let bytes = tokio::time::timeout(CONTROL_IO_TIMEOUT, reader.read_line(&mut line))
        .await
        .context("timed out reading reload control request")??;
    if bytes == 0 || bytes > CONTROL_MAX_LINE_BYTES {
        anyhow::bail!("invalid reload control request size {bytes}");
    }
    let request: ReloadControlRequest =
        serde_json::from_str(line.trim_end()).context("invalid reload control request")?;
    if request.primary_socket != primary_socket.display().to_string() || request.nonce.is_empty() {
        anyhow::bail!("reload control primary socket or nonce did not match");
    }

    let response = ReloadControlResponse {
        primary_socket: request.primary_socket,
        pid: std::process::id(),
        nonce: request.nonce,
        sigusr1_supported: true,
    };
    let payload = serde_json::to_vec(&response)?;
    tokio::time::timeout(CONTROL_IO_TIMEOUT, async {
        writer.write_all(&payload).await?;
        writer.write_all(b"\n").await
    })
    .await
    .context("timed out writing reload control response")??;
    Ok(())
}

/// Prove the live daemon owns this primary socket and has a SIGUSR1 handler,
/// then signal only that freshly-attested PID. It never consults the registry.
pub async fn signal_stalled_reload(primary_socket: &Path) -> Result<()> {
    let control_socket =
        super::socket::reload_control_socket_path(primary_socket).ok_or_else(|| {
            anyhow::anyhow!(
                "cannot derive reload control socket from {}",
                primary_socket.display()
            )
        })?;
    let stream = tokio::time::timeout(CONTROL_IO_TIMEOUT, Stream::connect(&control_socket))
        .await
        .context("timed out connecting reload control socket")??;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let nonce = crate::id::new_id("reload-control");
    let request = ReloadControlRequest {
        primary_socket: primary_socket.display().to_string(),
        nonce: nonce.clone(),
    };
    let request = serde_json::to_vec(&request)?;
    tokio::time::timeout(CONTROL_IO_TIMEOUT, async {
        writer.write_all(&request).await?;
        writer.write_all(b"\n").await
    })
    .await
    .context("timed out writing reload control request")??;

    let mut line = String::new();
    let bytes = tokio::time::timeout(CONTROL_IO_TIMEOUT, reader.read_line(&mut line))
        .await
        .context("timed out reading reload control response")??;
    if bytes == 0 || bytes > CONTROL_MAX_LINE_BYTES {
        anyhow::bail!("invalid reload control response size {bytes}");
    }
    let response: ReloadControlResponse =
        serde_json::from_str(line.trim_end()).context("invalid reload control response")?;
    if response.primary_socket != primary_socket.display().to_string()
        || response.nonce != nonce
        || !response.sigusr1_supported
        || response.pid == 0
    {
        anyhow::bail!(
            "reload control capability did not bind the requested socket, nonce, PID, and SIGUSR1 support"
        );
    }

    // The response is from the live owner-only sibling endpoint. Send no signal
    // at all if its in-process reload state already shows an existing handoff.
    if crate::server::reload_marker_active(Duration::from_secs(30)) {
        anyhow::bail!("reload control declined signal because a reload is already active");
    }
    if unsafe { libc::kill(response.pid as i32, libc::SIGUSR1) } != 0 {
        anyhow::bail!(
            "failed to send SIGUSR1 to attested reload-control pid {}: {}",
            response.pid,
            std::io::Error::last_os_error()
        );
    }

    // A signal is not success by itself. It must cause the separately-owned
    // reload listener to write a marker, otherwise the CLI reports a declined
    // fallback rather than implying an unsafe restart occurred.
    let started = std::time::Instant::now();
    while started.elapsed() < SIGNAL_MARKER_TIMEOUT {
        if crate::server::reload_marker_active(Duration::from_secs(30)) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    anyhow::bail!("attested SIGUSR1 fallback was not accepted for idle recovery")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capability_exchange_binds_nonce_primary_socket_and_live_pid() {
        let (server, client) = Stream::pair().expect("socket pair");
        let primary = std::env::temp_dir().join("w13-primary.sock");
        let expected_socket = primary.display().to_string();
        let handler = tokio::spawn(answer_capability(server, primary));

        let (reader, mut writer) = client.into_split();
        let request = ReloadControlRequest {
            primary_socket: expected_socket.clone(),
            nonce: "nonce-w13".to_string(),
        };
        let payload = serde_json::to_vec(&request).expect("serialize request");
        writer.write_all(&payload).await.expect("write request");
        writer.write_all(b"\n").await.expect("terminate request");

        let mut line = String::new();
        BufReader::new(reader)
            .read_line(&mut line)
            .await
            .expect("read capability response");
        let response: ReloadControlResponse =
            serde_json::from_str(line.trim_end()).expect("decode capability response");
        assert_eq!(response.primary_socket, expected_socket);
        assert_eq!(response.nonce, "nonce-w13");
        assert_eq!(response.pid, std::process::id());
        assert!(response.sigusr1_supported);
        handler
            .await
            .expect("control task join")
            .expect("control task");
    }
}
