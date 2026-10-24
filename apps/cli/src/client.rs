//! Local MCP session over the published Unix socket.

use std::collections::HashMap;
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, UnixSocketHttpClient};
use rmcp::ServiceExt;
use serde_json::Value;

use crate::discovery::Discovery;
use crate::error::{exit_for_code, CliError, EXIT_AUTH, EXIT_INTERNAL, EXIT_USAGE};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Session {
    client: rmcp::service::RunningService<rmcp::RoleClient, ()>,
}

impl Session {
    /// Connect, authenticate and initialize. Locked and unavailable states are
    /// classified before the token is used where the discovery file already
    /// says the ledger is locked.
    pub async fn connect(
        discovery: &Discovery,
        token: &str,
        ledger_id: String,
    ) -> Result<Self, CliError> {
        if discovery.lock.status == "locked" {
            return Err(CliError::locked(
                "the profile is locked; unlock it in the Wealthlog app",
            ));
        }
        verify_peer(discovery).await?;

        let socket = discovery
            .socket
            .to_str()
            .ok_or_else(|| CliError::unavailable("the socket path is not valid UTF-8"))?
            .to_string();
        let uri = "http://localhost/mcp";
        let mut headers = HashMap::new();
        headers.insert(
            http::HeaderName::from_static("x-wf-profile-id"),
            http::HeaderValue::from_str(&ledger_id)
                .map_err(|_| CliError::usage("--ledger is not a valid header value"))?,
        );
        let config = StreamableHttpClientTransportConfig::with_uri(uri)
            .auth_header(token)
            .custom_headers(headers);
        let client = UnixSocketHttpClient::new(&socket, uri);
        let transport = StreamableHttpClientTransport::with_client(client, config);
        let client = match tokio::time::timeout(CONNECT_TIMEOUT, ().serve(transport)).await {
            Ok(Ok(client)) => client,
            Ok(Err(error)) => return Err(classify_connect_error(&error.to_string())),
            Err(_) => {
                return Err(CliError::unavailable(
                    "timed out connecting to the local service",
                ))
            }
        };
        let _ = ledger_id;
        Ok(Self { client })
    }

    /// Call one tool. A structured tool error is converted to [`CliError`]; a
    /// transport failure after the request was sent is an unknown result.
    pub async fn call(
        &self,
        tool: &str,
        arguments: Value,
    ) -> Result<(CallToolResult, Value), CliError> {
        let arguments = arguments
            .as_object()
            .cloned()
            .ok_or_else(|| CliError::usage("tool arguments must be a JSON object"))?;
        let request = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let result = self
            .client
            .call_tool(request)
            .await
            .map_err(|error| classify_service_error(&error))?;

        if result.is_error == Some(true) {
            if let Some(structured) = result.structured_content.clone() {
                return Err(from_structured_error(structured));
            }
            return Err(CliError::new(
                "execution_failed",
                text_of(&result),
                false,
                EXIT_INTERNAL,
            ));
        }
        let data = result.structured_content.clone().unwrap_or(Value::Null);
        Ok((result, data))
    }

    pub async fn close(self) {
        let _ = self.client.cancel().await;
    }
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn classify_rpc_error(detail: &str) -> CliError {
    if detail.contains("profile_mismatch") {
        return CliError::new(
            "profile_mismatch",
            "X-WF-Profile-Id does not match the active ledger",
            false,
            EXIT_USAGE,
        );
    }
    if detail.contains("profile_required") {
        return CliError::new(
            "profile_required",
            "X-WF-Profile-Id is required for formal writes",
            false,
            EXIT_USAGE,
        );
    }
    if detail.contains("session_revoked") || detail.contains("profile_locked") {
        return CliError::locked("the profile locked during this call; initialize a new session");
    }
    if detail.contains("scope_denied") || detail.contains("403") {
        return CliError::new(
            "scope_denied",
            "the access token does not grant this operation",
            false,
            EXIT_AUTH,
        );
    }
    if detail.contains("Unauthorized") || detail.contains("401") {
        return CliError::auth("the access token is missing, invalid, expired or revoked");
    }
    CliError::unknown(format!(
        "the request was sent but its result is unknown ({detail}); retry with the same request key"
    ))
}

fn classify_service_error(error: &rmcp::service::ServiceError) -> CliError {
    if let rmcp::service::ServiceError::McpError(rpc) = error {
        if let Some(data) = &rpc.data {
            if matches!(
                data.get("code").and_then(Value::as_str),
                Some("profile_mismatch" | "profile_required")
            ) {
                let code = data["code"].as_str().expect("matched string code");
                let mut error = CliError::new(code, rpc.message.to_string(), false, EXIT_USAGE);
                error.data = data.clone();
                return error;
            }
        }
    }
    classify_rpc_error(&format!("{error:?}"))
}

fn classify_connect_error(message: &str) -> CliError {
    if message.contains("401") || message.contains("Unauthorized") {
        CliError::auth("the access token is missing, invalid, expired or revoked")
    } else if message.contains("403") || message.contains("scope") {
        CliError::new(
            "scope_denied",
            "the access token does not grant this operation",
            false,
            EXIT_AUTH,
        )
    } else if message.contains("423") || message.contains("profile_locked") {
        CliError::locked("the profile is locked; unlock it in the Wealthlog app")
    } else {
        CliError::unavailable(format!(
            "the local service did not complete the MCP handshake ({message})"
        ))
    }
}

fn from_structured_error(value: Value) -> CliError {
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("execution_failed")
        .to_string();
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("the tool reported an error")
        .to_string();
    let retryable = value
        .get("retryable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let request_id = value
        .get("requestId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let data = value.get("data").cloned().unwrap_or(Value::Null);
    let exit = exit_for_code(&code);
    CliError {
        code,
        message,
        retryable,
        request_id,
        request_key: None,
        ledger_id: None,
        data,
        exit,
    }
}

pub(crate) async fn probe(discovery: &Discovery) -> Result<(), CliError> {
    verify_peer(discovery).await
}

#[cfg(unix)]
async fn verify_peer(discovery: &Discovery) -> Result<(), CliError> {
    let stream = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::UnixStream::connect(&discovery.socket),
    )
    .await
    .map_err(|_| CliError::unavailable("timed out connecting to the local service socket"))?
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot connect to the local service socket: {error}"
        ))
    })?;
    use std::os::fd::AsRawFd;
    let (uid, pid) = peer_credentials(stream.as_raw_fd()).map_err(|error| {
        CliError::unavailable(format!("cannot verify the service peer: {error}"))
    })?;
    if uid != effective_uid() {
        return Err(CliError::unavailable(
            "the service socket belongs to another user",
        ));
    }
    if pid != discovery.lock.pid {
        return Err(CliError::unavailable(
            "the service socket does not belong to the published instance",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
async fn verify_peer(_discovery: &Discovery) -> Result<(), CliError> {
    Err(CliError::unavailable(
        "the CLI Unix-socket transport is only supported on Unix hosts",
    ))
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "peer credentials are a libc socket query")]
fn peer_credentials(fd: std::os::fd::RawFd) -> std::io::Result<(u32, u32)> {
    use std::mem::{size_of, zeroed};
    let mut credential: libc::xucred = unsafe { zeroed() };
    let mut length = size_of::<libc::xucred>() as libc::socklen_t;
    // SAFETY: the descriptor is open, and both pointers point at writable
    // values of the size passed to the kernel.
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERCRED,
            &mut credential as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut pid: libc::pid_t = 0;
    let mut pid_length = size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: same descriptor and a correctly sized writable pid.
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            &mut pid as *mut _ as *mut libc::c_void,
            &mut pid_length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((credential.cr_uid, pid.max(0) as u32))
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code, reason = "peer credentials are a libc socket query")]
fn peer_credentials(fd: std::os::fd::RawFd) -> std::io::Result<(u32, u32)> {
    use std::mem::{size_of, zeroed};
    let mut credential: libc::ucred = unsafe { zeroed() };
    let mut length = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the descriptor is open and the buffer is sized correctly.
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credential as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((credential.uid, credential.pid.max(0) as u32))
}

#[cfg(unix)]
#[allow(unsafe_code, reason = "geteuid is a parameterless libc read")]
fn effective_uid() -> u32 {
    // SAFETY: parameterless libc read.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{EXIT_AUTH, EXIT_LOCKED, EXIT_UNAVAILABLE, EXIT_USAGE};

    #[test]
    fn write_blocked_is_a_non_retryable_authorization_rejection() {
        let error = from_structured_error(serde_json::json!({
            "code": "write_blocked",
            "message": "writes disabled",
            "retryable": false,
        }));
        assert_eq!(error.code, "write_blocked");
        assert_eq!(error.exit, EXIT_AUTH);
        assert!(!error.retryable);
        assert_eq!(error.value()["code"], "write_blocked");
    }

    #[test]
    fn structured_profile_rejections_preserve_binding_data() {
        for code in ["profile_mismatch", "profile_required"] {
            let data = serde_json::json!({
                "code": code,
                "expectedLedgerId": "ledger-a",
                "receivedLedgerId": "ledger-b",
            });
            let rpc = rmcp::service::ServiceError::McpError(rmcp::ErrorData::invalid_request(
                "wrong binding",
                Some(data.clone()),
            ));
            let converted = classify_service_error(&rpc);
            assert_eq!(converted.code, code);
            assert_eq!(converted.exit, EXIT_USAGE);
            assert_eq!(converted.data, data);
            assert!(!converted.retryable);
        }
    }

    #[test]
    fn protocol_rejections_keep_their_meaning() {
        let mismatch = classify_rpc_error("McpError { data: {code: \"profile_mismatch\"} }");
        assert_eq!(
            (mismatch.code.as_str(), mismatch.exit),
            ("profile_mismatch", EXIT_USAGE)
        );

        let required = classify_rpc_error("data: {code: \"profile_required\"}");
        assert_eq!(
            (required.code.as_str(), required.exit),
            ("profile_required", EXIT_USAGE)
        );

        let revoked = classify_rpc_error("data: {code: \"session_revoked\"}");
        assert_eq!(
            (revoked.code.as_str(), revoked.exit),
            ("profile_locked", EXIT_LOCKED)
        );

        let locked = classify_rpc_error("data: {code: \"profile_locked\"}");
        assert_eq!(
            (locked.code.as_str(), locked.exit),
            ("profile_locked", EXIT_LOCKED)
        );

        let scope = classify_rpc_error("data: {code: \"scope_denied\"}");
        assert_eq!(
            (scope.code.as_str(), scope.exit),
            ("scope_denied", EXIT_AUTH)
        );
    }

    #[test]
    fn a_transport_failure_after_send_is_an_unknown_result() {
        let unknown = classify_rpc_error("connection reset by peer");
        assert_eq!(unknown.code, "result_unknown");
        assert_eq!(unknown.exit, EXIT_UNAVAILABLE);
        assert!(unknown.retryable);
        assert!(unknown.message.contains("retry with the same request key"));
    }

    #[test]
    fn a_uuid_containing_423_is_not_mistaken_for_a_lock() {
        let detail = "McpError { message: \"ledger 123e4567-4230-...\", code: -32602 }";
        let error = classify_rpc_error(detail);
        assert_eq!(error.code, "result_unknown");
        assert_eq!(error.exit, EXIT_UNAVAILABLE);
    }
}
