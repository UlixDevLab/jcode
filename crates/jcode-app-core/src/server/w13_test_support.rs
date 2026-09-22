//! Disposable, feature-gated W13 server child support.
//!
//! This module is not compiled into ordinary builds. It deliberately blocks an
//! existing primary-socket request-path lock after a `Server::run` listener is
//! ready, allowing the root driver to exercise the production watchdog and CLI
//! fallback without a provider, catalog, auth, or production debug command.

use super::{Server, is_server_ready};
use crate::message::{Message, ToolDefinition};
use crate::provider::{EventStream, Provider};
use anyhow::Result;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Resolve, but never change, the target selected by the existing normal reload
/// policy for the feature-only W13 fixture. The offline fixture's disposable
/// request client is non-canary, so this uses the same non-selfdev preference as
/// that production request path.
pub fn normal_reload_exec_target() -> Option<PathBuf> {
    super::reload_exec_target(false).map(|(path, _label)| path)
}

pub fn canonical_w13_regular_file(path: &Path, label: &str) -> Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        anyhow::bail!(
            "W13 {label} is not a regular non-symlink file: {}",
            path.display()
        );
    }
    Ok(path.canonicalize()?)
}

pub fn w13_binary_identity(path: &Path, label: &str) -> Result<(PathBuf, String)> {
    let binary = canonical_w13_regular_file(
        &crate::build::resolve_binary_payload(path),
        &format!("{label} executable"),
    )?;
    let mut file = std::fs::File::open(&binary)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok((binary, format!("{:x}", hasher.finalize())))
}

pub fn write_w13_reexec_identity_receipt(path: &Path, root: &Path, socket: &Path) -> Result<()> {
    let receipt = canonical_w13_regular_file(path, "reexec receipt")?;
    let (source_binary, source_sha256) =
        w13_binary_identity(&std::env::current_exe()?, "observed replacement")?;
    std::fs::write(
        receipt,
        format!(
            "phase=serve\nreplacement_pid={}\nroot={}\nsocket={}\nsource_binary={}\nsource_sha256={}\n",
            std::process::id(),
            root.display(),
            socket.display(),
            source_binary.display(),
            source_sha256,
        ),
    )?;
    Ok(())
}

pub fn validate_w13_reexec_identity_receipt(
    path: &Path,
    root: &Path,
    socket: &Path,
    expected_pid: u32,
    expected_binary: &Path,
    expected_sha256: &str,
) -> Result<()> {
    let receipt = canonical_w13_regular_file(path, "reexec receipt")?;
    let contents = std::fs::read_to_string(&receipt)?;
    let field = |name| {
        contents.lines().find_map(|line| {
            line.strip_prefix(name)
                .and_then(|value| value.strip_prefix('='))
        })
    };
    let observed_binary = field("source_binary")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    if field("phase") != Some("serve")
        || field("replacement_pid").and_then(|value| value.parse().ok()) != Some(expected_pid)
        || field("root") != Some(root.to_string_lossy().as_ref())
        || field("socket") != Some(socket.to_string_lossy().as_ref())
        || observed_binary.as_deref() != Some(expected_binary)
        || field("source_sha256") != Some(expected_sha256)
    {
        anyhow::bail!("W13 reexec receipt did not match the expected replacement identity");
    }
    Ok(())
}

struct OfflineProvider;

#[async_trait]
impl Provider for OfflineProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        Err(anyhow::anyhow!(
            "W13 offline provider must not receive a generation request"
        ))
    }

    fn name(&self) -> &str {
        "w13-offline"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

/// Run a disposable server and, only after its owned primary listener is live,
/// optionally retain the existing connection-map writer lock. The locked mode
/// makes a production `Subscribe` wait in the real request handler without a
/// provider turn. The healthy mode uses the same offline listener and readiness
/// probe without taking that lock.
pub async fn run_offline_request_child(
    socket_path: PathBuf,
    debug_socket_path: PathBuf,
    ready_path: PathBuf,
    nonce: String,
    lock_request: bool,
) -> Result<()> {
    let server = Server::new_with_paths(
        Arc::new(OfflineProvider),
        socket_path.clone(),
        debug_socket_path,
    );
    let connections = Arc::clone(&server.client_connections);
    let ready_socket = socket_path.clone();
    let ready_task = tokio::spawn(async move {
        let ready = tokio::time::timeout(Duration::from_secs(10), async {
            while !is_server_ready(&ready_socket).await {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        if ready.is_err() {
            return;
        }
        if lock_request {
            let _held_connections = connections.write().await;
            let _ = std::fs::write(&ready_path, nonce);
            std::future::pending::<()>().await;
        } else {
            let _ = std::fs::write(&ready_path, nonce);
        }
    });

    let result = server.run().await;
    ready_task.abort();
    result
}

/// Run the feature-only replacement process used by the production same-build
/// `serve --socket` exec. It intentionally starts without the fixture's
/// pre-reload request lock, so the replacement can publish its normal socket
/// ready state.
pub async fn run_offline_reexec_server(
    socket_path: PathBuf,
    debug_socket_path: PathBuf,
) -> Result<()> {
    Server::new_with_paths(Arc::new(OfflineProvider), socket_path, debug_socket_path)
        .run()
        .await
}
