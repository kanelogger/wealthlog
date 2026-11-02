//! Embedded local MCP HTTP + Unix-domain-socket server.
//!
//! Binds `127.0.0.1` only: the fixed default port first, falling back to
//! a random high port when taken (the lock file is the source of truth for
//! discovery). The same tools are also served over a private Unix socket for
//! the CLI; that transport is recorded as the `cli` channel.
//!
//! Neither listener owns a ledger runtime. Every request resolves one through
//! the host's [`McpRuntimeProvider`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::routing::get;
use axum::{middleware, Json, Router};
use chrono::Utc;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use tokio_util::sync::CancellationToken;
use wealthfolio_business::audit::BusinessChannel;
use wealthfolio_mcp::runtime::McpRuntimeProvider;
#[cfg(test)]
use wealthfolio_mcp::AuditSink;
use wealthfolio_mcp::McpServerBuilder;

use super::middleware::{require_runtime_pat, validate_origin, McpRouterState};
use super::RunningServer;

/// Fixed default port; falls back to a random port when already in use.
pub const DEFAULT_PORT: u16 = 8639;

/// macOS `sun_path` includes the trailing NUL; Linux uses 108.
#[cfg(target_os = "macos")]
const SUN_PATH_CAPACITY: usize = 104;
#[cfg(not(target_os = "macos"))]
const SUN_PATH_CAPACITY: usize = 108;

const INSTRUCTIONS: &str = "Read access to the user's Wealthlog portfolio: accounts, \
holdings, valuations, performance, activities, income, health, and classifications, plus \
non-persisting draft and suggestion tools. Cash transfer recording requires transfers:write; manual securities trades and snapshot patches require holdings:write. They record local ledger facts, never bank payments or brokerage orders. Read get_investment_account_state first for stateVersion. Ordinary income/expense create and update are \
available when the token holds ordinary:write; they write posted activities directly under \
request replay. Other formal writes are unavailable. Tool visibility depends on the scopes \
granted to the access token in use. The service stays available while the main window is \
hidden; locking the profile, stopping the service, quitting the app, revoking the token or \
token expiry blocks further calls. Formal writes require X-WF-Profile-Id with the active ledger.";

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "server": "wealthfolio-mcp",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Builds the HTTP router: public `GET /health` plus `/mcp` behind Origin and
/// the ledger-gate/bearer middlewares.
pub fn build_router(
    provider: Arc<dyn McpRuntimeProvider>,
    channel: BusinessChannel,
    cancel: CancellationToken,
) -> Router {
    let mcp_service = McpServerBuilder::new_dynamic()
        .instructions(INSTRUCTIONS)
        .host(wealthfolio_agent_tools::McpHost::Desktop)
        .channel(channel)
        .dynamic()
        .build_http_service(
            StreamableHttpServerConfig::default().with_cancellation_token(cancel.child_token()),
        );

    let protected = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(
            McpRouterState {
                provider,
                sessions: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            },
            require_runtime_pat,
        ))
        .layer(middleware::from_fn(validate_origin));

    Router::new().route("/health", get(health)).merge(protected)
}

/// Convenience router over one fixed runtime, used by tests and single-ledger
/// embeddings.
#[cfg(test)]
pub fn build_static_router(
    env: Arc<dyn wealthfolio_agent_tools::AgentEnvironment>,
    audit_sink: Option<Arc<dyn AuditSink>>,
    pat_repository: Arc<wealthfolio_storage_sqlite::agent::PatRepository>,
    cancel: CancellationToken,
    ledger_id: String,
) -> Router {
    let provider = Arc::new(wealthfolio_mcp::StaticRuntimeProvider::from_parts(
        ledger_id,
        env,
        pat_repository,
        audit_sink,
    ));
    build_router(provider, BusinessChannel::Mcp, cancel)
}

/// A private temporary directory plus its socket. The app publishes this exact
/// path in `mcp.lock`; the CLI never derives it.
#[derive(Debug)]
pub struct SocketLocation {
    pub path: PathBuf,
    directory: PathBuf,
}

impl SocketLocation {
    fn create() -> Result<Self, String> {
        Self::create_in(&std::env::temp_dir())
    }

    fn create_in(root: &Path) -> Result<Self, String> {
        let root = std::fs::canonicalize(root)
            .map_err(|error| format!("Failed to resolve the temporary directory: {error}"))?;
        verify_private_root(&root)?;

        for _ in 0..8 {
            let name = format!("wl-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
            let directory = root.join(name);
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => {
                    verify_private_directory(&directory)?;
                    let path = directory.join("mcp.sock");
                    let bytes = path.as_os_str().as_encoded_bytes().len();
                    if bytes + 1 > SUN_PATH_CAPACITY {
                        let _ = std::fs::remove_dir(&directory);
                        return Err(format!(
                            "Unix socket path is {bytes} bytes; the platform limit is {} bytes including the trailing NUL",
                            SUN_PATH_CAPACITY
                        ));
                    }
                    return Ok(Self { path, directory });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("Failed to create the socket directory: {error}"))
                }
            }
        }
        Err("Failed to allocate a private socket directory".to_string())
    }

    pub(crate) fn cleanup(&self) {
        // Only remove what we created: the directory is random and private.
        #[cfg(unix)]
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path) {
            #[cfg(unix)]
            use std::os::unix::fs::FileTypeExt;
            if metadata.file_type().is_socket() {
                let _ = std::fs::remove_file(&self.path);
            }
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[cfg(unix)]
fn verify_private_root(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(root)
        .map_err(|error| format!("Failed to inspect the temporary directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("The temporary directory is not a real directory".to_string());
    }
    if metadata.uid() != effective_uid() {
        return Err("The temporary directory is not owned by the current user".to_string());
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(format!(
            "The temporary directory is not private (mode {:o})",
            metadata.mode() & 0o777
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_private_root(_root: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn verify_private_directory(directory: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(directory)
        .map_err(|error| format!("Failed to inspect the socket directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("The socket directory is not a real directory".to_string());
    }
    if metadata.uid() != effective_uid() {
        return Err("The socket directory is not owned by the current user".to_string());
    }
    if metadata.mode() & 0o777 != 0o700 {
        return Err(format!(
            "The socket directory is not private (mode {:o})",
            metadata.mode() & 0o777
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_private_directory(_directory: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    libc_geteuid()
}

#[cfg(unix)]
#[allow(unsafe_code, reason = "geteuid is a parameterless libc read")]
fn libc_geteuid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

/// Binds loopback TCP and a private Unix socket, then serves the same gateway
/// on both. `configured_port` falls back to [`DEFAULT_PORT`], then a random
/// loopback port when that is in use.
pub async fn start(
    provider: Arc<dyn McpRuntimeProvider>,
    configured_port: Option<u16>,
) -> Result<RunningServer, String> {
    let preferred = configured_port.unwrap_or(DEFAULT_PORT);
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", preferred)).await {
        Ok(listener) => listener,
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
            log::warn!(
                "MCP server port {} is in use; falling back to a random port",
                preferred
            );
            tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|e| format!("Failed to bind MCP server fallback port: {e}"))?
        }
        Err(err) => return Err(format!("Failed to bind MCP server port {preferred}: {err}")),
    };
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to read MCP server address: {e}"))?
        .port();

    let socket = SocketLocation::create()?;
    let unix_listener = match tokio::net::UnixListener::bind(&socket.path) {
        Ok(listener) => listener,
        Err(error) => {
            socket.cleanup();
            return Err(format!("Failed to bind the Unix socket: {error}"));
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) =
            std::fs::set_permissions(&socket.path, std::fs::Permissions::from_mode(0o600))
        {
            socket.cleanup();
            return Err(format!("Failed to restrict the Unix socket: {error}"));
        }
        let mode = std::fs::metadata(&socket.path)
            .map_err(|error| format!("Failed to inspect the Unix socket: {error}"))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o600 {
            socket.cleanup();
            return Err(format!("Unix socket mode is {mode:o}, expected 600"));
        }
    }

    let cancel = CancellationToken::new();
    let tcp_router = build_router(provider.clone(), BusinessChannel::Mcp, cancel.clone());
    let uds_router = build_router(provider, BusinessChannel::Cli, cancel.clone());

    let tcp_cancel = cancel.clone();
    let tcp_join = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, tcp_router)
            .with_graceful_shutdown(tcp_cancel.cancelled_owned())
            .await
        {
            log::error!("MCP TCP server terminated with error: {err}");
        }
    });
    let uds_cancel = cancel.clone();
    let uds_join = tokio::spawn(async move {
        if let Err(err) = axum::serve(unix_listener, uds_router)
            .with_graceful_shutdown(uds_cancel.cancelled_owned())
            .await
        {
            log::error!("MCP Unix socket server terminated with error: {err}");
        }
    });

    log::info!(
        "MCP server listening on 127.0.0.1:{port} and {}",
        socket.path.display()
    );
    Ok(RunningServer {
        port,
        started_at: Utc::now(),
        instance_id: uuid::Uuid::new_v4().to_string(),
        socket,
        cancel,
        joins: vec![tcp_join, uds_join],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_socket_lives_in_a_private_short_directory() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let first = SocketLocation::create_in(root.path()).unwrap();
        let second = SocketLocation::create_in(root.path()).unwrap();
        assert_ne!(first.path, second.path, "each listener gets a new path");
        let bytes = first.path.as_os_str().as_encoded_bytes().len();
        assert!(
            bytes < SUN_PATH_CAPACITY,
            "socket path is {bytes} bytes, limit including NUL {}",
            SUN_PATH_CAPACITY
        );
        let directory = first.path.parent().unwrap();
        assert_eq!(
            std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        first.cleanup();
        second.cleanup();
        assert!(!directory.exists());
    }

    #[test]
    fn a_public_temporary_root_is_rejected_without_fallback() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = SocketLocation::create_in(root.path()).unwrap_err();
        assert!(error.contains("not private"), "{error}");
    }

    #[test]
    fn a_file_as_temporary_root_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        assert!(SocketLocation::create_in(&file).is_err());
    }
}
