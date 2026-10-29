//! Integration tests for the `/mcp` endpoint (PAT auth) and the
//! agent-access management API.
//!
//! These boot the real router on a loopback port (the MCP transport
//! answers over SSE, so `oneshot` is not enough) with auth enabled.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use std::net::SocketAddr;

use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::{rngs::OsRng, RngCore};
use reqwest::header;
use tempfile::TempDir;
use wealthfolio_server::{api::app_router_from_config, config::Config};

const PASSWORD: &str = "super-secret";

/// The canonical read-only scope set, used when minting test tokens.
const READ_ONLY_SCOPES: &[&str] = &[
    "accounts:read",
    "holdings:read",
    "performance:read",
    "activities:read",
    "health:read",
    "classification:read",
];

/// Env vars are process-global; serialize config/state construction
/// (build_state itself mutates DATABASE_URL / WF_SECRET_FILE).
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct TestServer {
    base: String,
    client: reqwest::Client,
    /// Keeps the database directory alive for the server's lifetime.
    _tmp: TempDir,
    shared_runtime: Option<std::sync::Arc<wealthfolio_server::AppState>>,
}

async fn spawn_server(mcp_enabled: bool, audit_enabled: bool) -> TestServer {
    spawn_server_with_runtime(mcp_enabled, audit_enabled, false).await
}

async fn spawn_server_with_runtime(
    mcp_enabled: bool,
    audit_enabled: bool,
    retain_runtime: bool,
) -> TestServer {
    let tmp = tempfile::tempdir().unwrap();
    let (router, shared_runtime) = {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("WF_DATA_DIR", "");
        std::env::set_var("WF_DB_PATH", tmp.path().join("test.db"));
        std::env::set_var("WF_SECRET_FILE", tmp.path().join("secrets.json"));
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::default()
            .hash_password(PASSWORD.as_bytes(), &salt)
            .unwrap()
            .to_string();
        std::env::set_var("WF_AUTH_PASSWORD_HASH", password_hash);
        let mut secret_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut secret_bytes);
        std::env::set_var("WF_SECRET_KEY", BASE64.encode(secret_bytes));
        std::env::set_var("WF_CORS_ALLOW_ORIGINS", "http://localhost:3000");
        // The router is served on an ephemeral loopback port; declare the same
        // listen address so /auth/status reports `loopback`.
        std::env::set_var("WF_LISTEN_ADDR", "127.0.0.1:8088");
        if mcp_enabled {
            std::env::set_var("WF_MCP_ENABLED", "true");
        } else {
            std::env::remove_var("WF_MCP_ENABLED");
        }
        if audit_enabled {
            std::env::remove_var("WF_MCP_AUDIT_ENABLED");
        } else {
            std::env::set_var("WF_MCP_AUDIT_ENABLED", "false");
        }

        let config = Config::from_env().unwrap();
        if retain_runtime {
            let state = wealthfolio_server::build_state(&config).await.unwrap();
            let router = wealthfolio_server::api::app_router(state.clone(), &config).unwrap();
            (router, Some(state))
        } else {
            (app_router_from_config(&config).await.unwrap(), None)
        }
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    TestServer {
        base: format!("http://{addr}"),
        client: reqwest::Client::new(),
        _tmp: tmp,
        shared_runtime,
    }
}

/// Boots the router without any configured authentication, loopback only.
async fn spawn_server_unauthenticated() -> TestServer {
    let tmp = tempfile::tempdir().unwrap();
    let router = {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("WF_DATA_DIR", "");
        std::env::set_var("WF_DB_PATH", tmp.path().join("test.db"));
        std::env::set_var("WF_SECRET_FILE", tmp.path().join("secrets.json"));
        std::env::remove_var("WF_AUTH_PASSWORD_HASH");
        let mut secret_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut secret_bytes);
        std::env::set_var("WF_SECRET_KEY", BASE64.encode(secret_bytes));
        std::env::set_var("WF_CORS_ALLOW_ORIGINS", "http://localhost:3000");
        std::env::set_var("WF_LISTEN_ADDR", "127.0.0.1:8088");
        std::env::remove_var("WF_MCP_ENABLED");
        std::env::remove_var("WF_MCP_AUDIT_ENABLED");

        let config = Config::from_env().unwrap();
        app_router_from_config(&config).await.unwrap()
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    TestServer {
        base: format!("http://{addr}"),
        client: reqwest::Client::new(),
        _tmp: tmp,
        shared_runtime: None,
    }
}

/// Logs in and returns the `wf_session` cookie value (the JWT).
async fn login(server: &TestServer) -> String {
    let response = server
        .client
        .post(format!("{}/api/v1/auth/login", server.base))
        .json(&serde_json::json!({ "password": PASSWORD }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("login should set a cookie")
        .to_str()
        .unwrap();
    set_cookie
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("wf_session=")
        .to_string()
}

fn init_body() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1.0" }
        }
    })
}

/// POSTs a JSON-RPC message to `/mcp` with optional bearer + session id.
async fn mcp_post(
    server: &TestServer,
    bearer: Option<&str>,
    session: Option<&str>,
    body: serde_json::Value,
) -> reqwest::Response {
    let mut request = server
        .client
        .post(format!("{}/mcp", server.base))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(body.to_string());
    if let Some(token) = bearer {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    request.send().await.unwrap()
}

/// Stateful mode answers over SSE — extract the first `data:` line that
/// carries a JSON payload (priming events have empty data).
fn parse_sse_data(body: &str) -> serde_json::Value {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .find_map(|data| serde_json::from_str(data.trim()).ok())
        .unwrap_or_else(|| panic!("no SSE JSON data line in response: {body}"))
}

/// Creates a PAT via the JWT-protected REST API; returns the response JSON.
async fn create_pat(
    server: &TestServer,
    cookie: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = server
        .client
        .post(format!("{}/api/v1/agent-access/tokens", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let json = response.json().await.unwrap();
    (status, json)
}

/// Runs the MCP initialize handshake; returns the session id.
async fn mcp_initialize(server: &TestServer, pat: &str) -> String {
    let response = mcp_post(server, Some(pat), None, init_body()).await;
    assert_eq!(response.status(), 200);
    let session = response
        .headers()
        .get("mcp-session-id")
        .expect("stateful mode should assign a session id")
        .to_str()
        .unwrap()
        .to_string();
    let init = parse_sse_data(&response.text().await.unwrap());
    assert_eq!(init["result"]["serverInfo"]["name"], "wealthlog");

    // The spec-mandated initialized notification (202 Accepted).
    let notify = mcp_post(
        server,
        Some(pat),
        Some(&session),
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(notify.status(), 202);
    session
}

/// Lists the tools a token can discover.
async fn list_tools(server: &TestServer, pat: &str, session: &str) -> Vec<String> {
    let response = mcp_post(
        server,
        Some(pat),
        Some(session),
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let list = parse_sse_data(&response.text().await.unwrap());
    list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn mcp_pat_lifecycle() {
    let server = spawn_server(true, true).await;

    // (a) No PAT -> 401 with a JSON-RPC-shaped body.
    let response = mcp_post(&server, None, None, init_body()).await;
    assert_eq!(response.status(), 401);
    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["error"]["message"].is_string());

    // (b) Garbage PAT -> 401.
    let response = mcp_post(
        &server,
        Some("wfp_notavalidtoken_notavalidtoken_notavalid"),
        None,
        init_body(),
    )
    .await;
    assert_eq!(response.status(), 401);

    // (c) A JWT does not work on /mcp.
    let cookie = login(&server).await;
    let response = mcp_post(&server, Some(&cookie), None, init_body()).await;
    assert_eq!(response.status(), 401);

    // (d) Create a PAT via REST, then run the MCP handshake with it.
    let (status, created) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "  ci token  ", "scopes": READ_ONLY_SCOPES }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = created["token"].as_str().unwrap().to_string();
    assert!(pat.starts_with("wfp_"));
    assert_eq!(pat.len(), 47, "wfp_ + 43 base64url chars");
    assert_eq!(created["name"], "ci token");
    assert_eq!(created["tokenPrefix"].as_str().unwrap().len(), 12);
    assert!(pat[4..].starts_with(created["tokenPrefix"].as_str().unwrap()));
    let token_id = created["id"].as_str().unwrap().to_string();

    // Empty name is rejected.
    let (status, _) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "   ", "scopes": READ_ONLY_SCOPES }),
    )
    .await;
    assert_eq!(status, 400);

    // Empty scopes are rejected.
    let (status, _) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "no scopes", "scopes": [] }),
    )
    .await;
    assert_eq!(status, 400);

    // Unknown scopes are rejected.
    let (status, _) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "bad scope", "scopes": ["accounts:read", "bogus:scope"] }),
    )
    .await;
    assert_eq!(status, 400);

    // Retired planning scopes are rejected, including alongside a retained scope.
    for scopes in [
        vec!["financial-planning:read"],
        vec!["holdings:read", "financial-planning:read"],
    ] {
        let (status, _) = create_pat(
            &server,
            &cookie,
            serde_json::json!({ "name": "retired planning scope", "scopes": scopes }),
        )
        .await;
        assert_eq!(status, 400);
    }

    // activities:write without activities:draft is rejected (dependency).
    let (status, _) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "write only", "scopes": ["activities:write"] }),
    )
    .await;
    assert_eq!(status, 400);

    // classification:write without classification:suggest is rejected (dependency).
    let (status, _) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "classification write only", "scopes": ["classification:write"] }),
    )
    .await;
    assert_eq!(status, 400);

    // This host never grants the desktop-only ordinary write scope.
    for scopes in [
        vec!["ordinary:write"],
        vec!["activities:read", "accounts:read", "ordinary:write"],
    ] {
        let (status, body) = create_pat(
            &server,
            &cookie,
            serde_json::json!({ "name": "ordinary writer", "scopes": scopes }),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert!(
            body.to_string().contains("not available on this host"),
            "{body}"
        );
    }

    // A removed scope cannot grant access to retired quote import tools.
    let (status, _) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "quote write only", "scopes": ["market-data:write"] }),
    )
    .await;
    assert_eq!(status, 400);

    let session = mcp_initialize(&server, &pat).await;

    // A read-only token cannot discover write or commit tools.
    let names = list_tools(&server, &pat, &session).await;
    assert!(!names.iter().any(|name| matches!(
        name.as_str(),
        "commit_activity_draft" | "commit_activity_updates" | "commit_transaction_categories"
    )));
    assert!(names.iter().any(|name| name == "get_net_worth"));

    // (h) tools/call succeeds and writes an audit row (awaited before the
    // response returns; the poll just tolerates sink latency).
    let response = mcp_post(
        &server,
        Some(&pat),
        Some(&session),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "get_accounts", "arguments": {} }
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let call = parse_sse_data(&response.text().await.unwrap());
    assert_ne!(call["result"]["isError"], serde_json::json!(true));

    let mut audit: Option<serde_json::Value> = None;
    for _ in 0..100 {
        let page: serde_json::Value = server
            .client
            .get(format!("{}/api/v1/agent-access/audit", server.base))
            .header(header::COOKIE, format!("wf_session={cookie}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if page["totalCount"].as_i64().unwrap_or(0) >= 1 {
            audit = Some(page);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let audit = audit.expect("audit row for tools/call never arrived");
    let item = &audit["items"][0];
    assert_eq!(item["tool"], "get_accounts");
    assert_eq!(item["actorKind"], "pat");
    assert_eq!(item["outcome"], "success");
    assert!(item["actorFingerprint"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));

    // The retained holdings read permission still authorizes net-worth queries.
    let response = mcp_post(
        &server,
        Some(&pat),
        Some(&session),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "get_net_worth", "arguments": {} }
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let call = parse_sse_data(&response.text().await.unwrap());
    assert!(call.get("error").is_none(), "{call}");
    assert_ne!(call["result"]["isError"], serde_json::json!(true));

    // (e) A PAT is not a JWT: protected /api/v1 routes reject it.
    let response = server
        .client
        .get(format!("{}/api/v1/accounts", server.base))
        .header("Authorization", format!("Bearer {pat}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);

    // Token listing exposes metadata but never the hash or the token.
    let tokens: serde_json::Value = server
        .client
        .get(format!("{}/api/v1/agent-access/tokens", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let listed = tokens
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == token_id.as_str())
        .expect("created token should be listed");
    assert!(listed.get("tokenHash").is_none());
    assert!(listed.get("token").is_none());
    assert_eq!(listed["tokenPrefix"], created["tokenPrefix"]);

    // Status endpoint reflects the enabled MCP endpoint.
    let status_json: serde_json::Value = server
        .client
        .get(format!("{}/api/v1/agent-access/status", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_json["mcpEnabled"], true);
    assert_eq!(status_json["auditEnabled"], true);
    assert_eq!(status_json["endpoint"], "/mcp");
    assert_eq!(status_json["serverAuthEnabled"], true);
    assert_eq!(status_json["listenScope"], "loopback");

    // (f) Remove -> subsequent /mcp calls fail; unknown id -> 404.
    let response = server
        .client
        .delete(format!(
            "{}/api/v1/agent-access/tokens/{token_id}",
            server.base
        ))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    let response = mcp_post(&server, Some(&pat), None, init_body()).await;
    assert_eq!(response.status(), 401);
    let response = server
        .client
        .delete(format!(
            "{}/api/v1/agent-access/tokens/no-such-token",
            server.base
        ))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);

    // (g) Expired PAT -> 401.
    let (status, expired) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "expired", "expiresAt": "2000-01-01T00:00:00Z", "scopes": READ_ONLY_SCOPES }),
    )
    .await;
    assert_eq!(status, 201);
    let expired_pat = expired["token"].as_str().unwrap();
    let response = mcp_post(&server, Some(expired_pat), None, init_body()).await;
    assert_eq!(response.status(), 401);

    // Audit purge clears the log.
    let purge: serde_json::Value = server
        .client
        .post(format!("{}/api/v1/agent-access/audit/purge", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(purge["purged"].as_u64().unwrap() >= 1);
}

/// Net-worth reads need only the retained holdings scope.
#[tokio::test]
async fn mcp_holdings_only_token_can_query_net_worth() {
    let server = spawn_server(true, false).await;
    let cookie = login(&server).await;
    let (status, created) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "net worth reader", "scopes": ["holdings:read"] }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = created["token"].as_str().unwrap();
    let session = mcp_initialize(&server, pat).await;

    let names = list_tools(&server, pat, &session).await;
    assert!(names.iter().any(|name| name == "get_net_worth"));
    assert!(!names.iter().any(|name| name == "get_accounts"));

    let response = mcp_post(
        &server,
        Some(pat),
        Some(&session),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "get_net_worth", "arguments": {} }
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let call = parse_sse_data(&response.text().await.unwrap());
    assert!(call.get("error").is_none(), "{call}");
    assert!(call["result"].is_object(), "{call}");
    assert_ne!(call["result"]["isError"], serde_json::json!(true));
}

/// Formal-write tools stay absent from every token's discovery, the eight
/// write tools and the hidden preview cannot be executed, and the write
/// scopes no longer expose anything beyond the draft/suggest tools.
#[tokio::test]
async fn mcp_write_scoped_token_sees_no_formal_write_tools() {
    let server = spawn_server(true, false).await;
    let cookie = login(&server).await;

    let full_scopes: Vec<&str> = READ_ONLY_SCOPES
        .iter()
        .copied()
        .chain([
            "activities:draft",
            "activities:write",
            "classification:suggest",
            "classification:write",
        ])
        .collect();
    let (status, created) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "writer", "scopes": full_scopes }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = created["token"].as_str().unwrap().to_string();
    let session = mcp_initialize(&server, &pat).await;
    let names = list_tools(&server, &pat, &session).await;

    // Reads, pending entries, suggestions and the new persistent writes are
    // discoverable; the retired previews are not.
    for name in [
        "find_transfer_matches",
        "create_pending_activity",
        "list_pending_activities",
        "propose_transaction_categories",
        "create_categorization_rule",
        "prepare_asset_classification",
    ] {
        assert!(
            names.iter().any(|visible| visible == name),
            "{name} visible"
        );
    }
    // The retired draft-commit surface and hidden previews stay out of discovery.
    for name in [
        "record_activity",
        "record_activities",
        "create_activity",
        "update_activity",
        "commit_activity_draft",
        "commit_activity_drafts",
        "commit_asset_classification_draft",
        "commit_categorization_rule",
        "commit_transaction_categories",
        "link_transfer_activities",
        "unlink_transfer_activities",
        "prepare_activity_updates",
        "commit_activity_updates",
    ] {
        assert!(
            !names.iter().any(|visible| visible == name),
            "write tool {name} must be hidden"
        );
        let response = mcp_post(
            &server,
            Some(&pat),
            Some(&session),
            serde_json::json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": name, "arguments": {} } }),
        )
        .await;
        let rejected = parse_sse_data(&response.text().await.unwrap());
        assert!(
            rejected["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown tool"),
            "{rejected}"
        );
    }
    for name in [
        "get_import_mapping",
        "prepare_activity_import",
        "commit_activity_import",
        "prepare_quote_import",
        "commit_quote_import",
        "get_goals",
        "get_contribution_limits",
    ] {
        assert!(
            !names.contains(&name.to_string()),
            "retired tool {name} must be absent"
        );
    }
}
/// POSTs JSON to the cookie-authenticated REST API and returns the body.
async fn api_post(
    server: &TestServer,
    cookie: &str,
    path: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let response = server
        .client
        .post(format!("{}/api/v1/{path}", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert!(status.is_success(), "{path}: {status} {value}");
    value
}

/// PUTs JSON to the cookie-authenticated REST API and returns the body.
async fn api_put(
    server: &TestServer,
    cookie: &str,
    path: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let response = server
        .client
        .put(format!("{}/api/v1/{path}", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert!(status.is_success(), "{path}: {status} {value}");
    value
}

/// Disables every market-data provider. Symbol search, quote-currency and
/// profile lookups all go through them, so the test makes no outbound requests.
async fn disable_market_data_providers(server: &TestServer, cookie: &str) {
    let providers: Vec<serde_json::Value> = server
        .client
        .get(format!("{}/api/v1/providers/settings", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!providers.is_empty());
    for provider in providers {
        let response = server
            .client
            .put(format!("{}/api/v1/providers/settings", server.base))
            .header(header::COOKIE, format!("wf_session={cookie}"))
            .json(&serde_json::json!({
                "providerId": provider["id"], "priority": provider["priority"], "enabled": false
            }))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "{provider}");
    }
}
#[tokio::test]
async fn mcp_audit_disabled_writes_no_rows() {
    let server = spawn_server(true, false).await;
    let cookie = login(&server).await;

    // Status reflects the disabled audit log.
    let status_json: serde_json::Value = server
        .client
        .get(format!("{}/api/v1/agent-access/status", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_json["mcpEnabled"], true);
    assert_eq!(status_json["auditEnabled"], false);

    // A successful tools/call must not write an audit row.
    let (status, created) = create_pat(
        &server,
        &cookie,
        serde_json::json!({ "name": "ci", "scopes": READ_ONLY_SCOPES }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = created["token"].as_str().unwrap().to_string();
    let session = mcp_initialize(&server, &pat).await;

    let response = mcp_post(
        &server,
        Some(&pat),
        Some(&session),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "get_accounts", "arguments": {} }
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let call = parse_sse_data(&response.text().await.unwrap());
    assert_ne!(call["result"]["isError"], serde_json::json!(true));

    // The sink is disabled, so no row is written; a small delay guards against
    // any async write slipping in before asserting the log stayed empty.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let page: serde_json::Value = server
        .client
        .get(format!("{}/api/v1/agent-access/audit", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        page["totalCount"].as_i64().unwrap(),
        0,
        "no audit row should be written when WF_MCP_AUDIT_ENABLED=false"
    );
}

#[tokio::test]
async fn mcp_disabled_returns_404() {
    let server = spawn_server(false, true).await;

    // (i) /mcp is not mounted at all when WF_MCP_ENABLED is false.
    let response = mcp_post(&server, None, None, init_body()).await;
    assert_eq!(response.status(), 404);

    // The management API still reports the endpoint as disabled.
    let cookie = login(&server).await;
    let status_json: serde_json::Value = server
        .client
        .get(format!("{}/api/v1/agent-access/status", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_json["mcpEnabled"], false);
}

#[tokio::test]
async fn profile_deletion_closes_initialized_mcp_sessions() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    let (status, token) = create_pat(
        &server,
        &cookie,
        serde_json::json!({"name":"deletion test", "scopes":READ_ONLY_SCOPES}),
    )
    .await;
    assert_eq!(status, 201);
    let pat = token["token"].as_str().unwrap();
    let session = mcp_initialize(&server, pat).await;
    let state: serde_json::Value = server
        .client
        .post(format!("{}/api/v1/profiles/get_profile_state", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let profile = &state["profiles"][0];
    let response = server
        .client
        .post(format!("{}/api/v1/profiles/delete_profile", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .header(
            "x-wf-profile-scope",
            state["session"]["scopeId"].as_str().unwrap(),
        )
        .json(&serde_json::json!({"profileId":profile["id"],"confirmation":profile["name"]}))
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(status, 200, "{}", response.text().await.unwrap());
    let response = mcp_post(
        &server,
        Some(pat),
        Some(&session),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    assert!(!response.status().is_success());
}

async fn category_api_get(server: &TestServer, cookie: &str, path: &str) -> serde_json::Value {
    let response = server
        .client
        .get(format!("{}/api/v1/{path}", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value = response.json().await.unwrap();
    assert!(status.is_success(), "{path}: {status} {value}");
    value
}

/// Synthetic spending rows and categories, independent of bundled merchant rules.
async fn category_save_fixture(
    server: &TestServer,
    cookie: &str,
) -> (
    serde_json::Value,
    Vec<serde_json::Value>,
    String,
    String,
    String,
) {
    disable_market_data_providers(server, cookie).await;
    let account = api_post(
        server,
        cookie,
        "accounts",
        serde_json::json!({
            "name": "Synthetic spending", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        server,
        cookie,
        "spending/settings",
        serde_json::json!({
            "enabled": true, "accountIds": [account["id"]]
        }),
    )
    .await;
    let mut activities = Vec::new();
    for day in [1, 2] {
        activities.push(
            api_post(
                server,
                cookie,
                "activities",
                serde_json::json!({
                    "accountId": account["id"], "activityType": "WITHDRAWAL",
                    "activityDate": format!("2026-04-0{day}T12:00:00Z"), "currency": "EUR",
                    "amount": "12.50", "notes": format!("Synthetic merchant {day}")
                }),
            )
            .await,
        );
    }
    let mut categories = Vec::new();
    for (taxonomy, key) in [
        ("spending_categories", "synthetic_first"),
        ("spending_categories", "synthetic_second"),
        ("income_sources", "synthetic_income"),
    ] {
        let category = api_post(
            server,
            cookie,
            "taxonomies/categories",
            serde_json::json!({
                "taxonomyId": taxonomy, "name": key, "key": key, "color": "#123456",
                "sortOrder": 0
            }),
        )
        .await;
        categories.push(category["id"].as_str().unwrap().to_string());
    }
    (
        account,
        activities,
        categories[0].clone(),
        categories[1].clone(),
        categories[2].clone(),
    )
}

async fn category_writer_session(server: &TestServer, cookie: &str) -> (String, String) {
    let (status, token) = create_pat(
        server,
        cookie,
        serde_json::json!({
            "name": "category writer",
            "scopes": ["activities:read", "classification:read", "classification:suggest", "classification:write"]
        }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = token["token"].as_str().unwrap().to_string();
    let session = mcp_initialize(server, &pat).await;
    (pat, session)
}

fn category_assignment(activity: &serde_json::Value, category: &str) -> serde_json::Value {
    serde_json::json!({
        "activityId": activity["id"], "taxonomyId": "spending_categories", "categoryId": category
    })
}

async fn category_rows(
    server: &TestServer,
    cookie: &str,
    activity: &serde_json::Value,
) -> serde_json::Value {
    category_api_get(
        server,
        cookie,
        &format!(
            "spending/activities/{}/assignments",
            activity["id"].as_str().unwrap()
        ),
    )
    .await
}

/// `commit_transaction_categories` is disabled over MCP this release, so an
/// authorized token discovers the draft tool but cannot persist proposals.
/// The REST bulk-assignment route still works for the GUI.
#[tokio::test]
async fn mcp_category_commit_is_blocked_while_rest_bulk_assign_works() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    let (_, activities, first, _, _) = category_save_fixture(&server, &cookie).await;
    let (pat, session) = category_writer_session(&server, &cookie).await;

    // Discovery: the suggest tool is visible; the commit tool is not.
    let names = list_tools(&server, &pat, &session).await;
    assert!(names
        .iter()
        .any(|name| name == "propose_transaction_categories"));
    assert!(!names
        .iter()
        .any(|name| name == "commit_transaction_categories"));

    // Execution: refused as an unknown tool even with every write scope.
    let response = mcp_post(
        &server,
        Some(&pat),
        Some(&session),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "commit_transaction_categories",
                "arguments": { "assignments": [category_assignment(&activities[0], &first)] } }
        }),
    )
    .await;
    let rejected = parse_sse_data(&response.text().await.unwrap());
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown tool"),
        "{rejected}"
    );
    assert!(category_rows(&server, &cookie, &activities[0])
        .await
        .as_array()
        .unwrap()
        .is_empty());

    // Rejected calls leave an audit trace without leaking arguments.
    let audit = category_api_get(&server, &cookie, "agent-access/audit").await;
    let row = audit["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["tool"] == "commit_transaction_categories")
        .expect("blocked call must be audited");
    assert_eq!(row["outcome"], "error");
    let text = row.to_string();
    assert!(
        !text.contains(activities[0]["id"].as_str().unwrap()),
        "{row}"
    );
    assert!(!text.contains(&first), "{row}");

    // The GUI/REST bulk path remains available for the same assignment.
    let applied = api_post(
        &server,
        &cookie,
        "spending/assignments/bulk",
        serde_json::json!([{
            "activityId": activities[0]["id"],
            "taxonomyId": "spending_categories",
            "categoryId": first
        }]),
    )
    .await;
    assert_eq!(applied.as_array().unwrap().len(), 1, "{applied}");
    let rows = category_rows(&server, &cookie, &activities[0]).await;
    assert_eq!(rows[0]["categoryId"], first);
    assert_eq!(rows[0]["weight"], 10000);
}

/// Calls an MCP tool and returns its structured result, failing on tool errors.
async fn mcp_call_tool(
    server: &TestServer,
    pat: &str,
    session: &str,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let response = mcp_post(
        server,
        Some(pat),
        Some(session),
        serde_json::json!({
            "jsonrpc": "2.0", "id": name, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let result = parse_sse_data(&response.text().await.unwrap());
    assert!(result.get("error").is_none(), "{result}");
    assert_ne!(result["result"]["isError"], true, "{name}: {result}");
    result["result"]["structuredContent"].clone()
}

async fn create_eur_account(server: &TestServer, cookie: &str, name: &str) -> serde_json::Value {
    api_post(
        server,
        cookie,
        "accounts",
        serde_json::json!({
            "name": name, "accountType": "SECURITIES", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await
}

/// The transfer matcher stays available over MCP after the link/unlink writes
/// were disabled: it finds an unlinked pair, and the GUI REST route can link it
/// while the MCP write route cannot be reached at all.
#[tokio::test]
async fn mcp_find_transfer_matches_remains_readable_while_linking_moves_to_rest() {
    let server = spawn_server(true, false).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let (status, token) = create_pat(
        &server,
        &cookie,
        serde_json::json!({
            "name": "transfer reader",
            "scopes": ["activities:read", "activities:draft", "activities:write"]
        }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = token["token"].as_str().unwrap();
    let session = mcp_initialize(&server, pat).await;

    let chequing = create_eur_account(&server, &cookie, "Chequing").await;
    let savings = create_eur_account(&server, &cookie, "Savings").await;
    let transfer = |account: &serde_json::Value, activity_type: &str| {
        api_post(
            &server,
            &cookie,
            "activities",
            serde_json::json!({
                "accountId": account["id"], "activityType": activity_type,
                "activityDate": "2026-04-01T12:00:00Z", "currency": "EUR", "amount": "100"
            }),
        )
    };
    let out_id = transfer(&chequing, "TRANSFER_OUT").await["id"].clone();
    let in_id = transfer(&savings, "TRANSFER_IN").await["id"].clone();

    let found = mcp_call_tool(
        &server,
        pat,
        &session,
        "find_transfer_matches",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(found["total"], 2, "{found}");
    let outgoing = found["transfers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|transfer| transfer["activity"]["id"] == out_id)
        .unwrap_or_else(|| panic!("{found}"));
    assert_eq!(outgoing["linkState"], "needs_counterpart", "{outgoing}");
    assert_eq!(
        outgoing["candidates"][0]["activity"]["id"], in_id,
        "{outgoing}"
    );

    // MCP cannot link the pair.
    for name in ["link_transfer_activities", "unlink_transfer_activities"] {
        let response = mcp_post(
            &server,
            Some(pat),
            Some(&session),
            serde_json::json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": name, "arguments": { "pairs": [] } } }),
        )
        .await;
        let rejected = parse_sse_data(&response.text().await.unwrap());
        assert!(
            rejected["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown tool"),
            "{name}: {rejected}"
        );
    }

    // The GUI path still links, and the lookup sees the pair.
    api_post(
        &server,
        &cookie,
        "activities/link",
        serde_json::json!({ "activityAId": out_id, "activityBId": in_id }),
    )
    .await;
    let lookup = mcp_call_tool(
        &server,
        pat,
        &session,
        "find_transfer_matches",
        serde_json::json!({ "activityId": out_id }),
    )
    .await;
    assert_eq!(lookup["total"], 0, "{lookup}");
    assert_eq!(lookup["linkedTo"], in_id, "{lookup}");
}

/// Reads the profile state for an authenticated session cookie.
async fn session_state(server: &TestServer, cookie: &str) -> serde_json::Value {
    server
        .client
        .post(format!("{}/api/v1/profiles/get_profile_state", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn session_scope(server: &TestServer, cookie: &str) -> String {
    session_state(server, cookie).await["session"]["scopeId"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn business_post(
    server: &TestServer,
    cookie: &str,
    scope: &str,
    path: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let response = server
        .client
        .post(format!("{}/api/v1{path}", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .header("x-wf-profile-scope", scope)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert!(status.is_success(), "{path}: {status} {value}");
    value
}

async fn business_put(
    server: &TestServer,
    cookie: &str,
    scope: &str,
    path: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = server
        .client
        .put(format!("{}/api/v1{path}", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .header("x-wf-profile-scope", scope)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    (status, value)
}

async fn business_post_status(
    server: &TestServer,
    cookie: &str,
    scope: &str,
    path: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = server
        .client
        .post(format!("{}/api/v1{path}", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .header("x-wf-profile-scope", scope)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    (status, value)
}

async fn business_get(
    server: &TestServer,
    cookie: &str,
    scope: &str,
    path: &str,
) -> serde_json::Value {
    let response = server
        .client
        .get(format!("{}/api/v1{path}", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .header("x-wf-profile-scope", scope)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert!(status.is_success(), "{path}: {status} {value}");
    value
}

/// The unified ordinary write is versioned, replays retries and records the
/// authenticated session in the business audit.
#[tokio::test]
async fn ordinary_activity_write_is_versioned_and_replays_retries() {
    let server = spawn_server_with_runtime(true, true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;

    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Everyday", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;

    let create_body = serde_json::json!({
        "accountId": account["id"],
        "activityType": "WITHDRAWAL",
        "activityDate": "2026-04-01",
        "currency": "EUR",
        "amount": "12.50",
        "notes": "Lunch",
        "requestKey": "form-1"
    });
    let first = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        create_body.clone(),
    )
    .await;
    assert_eq!(first["version"], 1, "{first}");
    assert_eq!(first["replayed"], false, "{first}");
    let activity_id = first["activity"]["id"].as_str().unwrap().to_string();
    assert_eq!(first["activity"]["status"], "POSTED", "{first}");

    // Same key and content: no second row.
    let retried = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        create_body,
    )
    .await;
    assert_eq!(retried["replayed"], true, "{retried}");
    assert_eq!(retried["activity"]["id"], activity_id);
    assert_eq!(retried["version"], 1);
    assert_eq!(retried["requestId"], first["requestId"]);
    let stored = api_post(
        &server,
        &cookie,
        "activities/search",
        serde_json::json!({ "page": 0, "pageSize": 10, "accountIdFilter": account["id"] }),
    )
    .await;
    assert_eq!(stored["data"].as_array().unwrap().len(), 1, "{stored}");

    // A stale version conflicts instead of overwriting.
    let stale_body = serde_json::json!({
        "activityId": activity_id,
        "expectedVersion": 7,
        "accountId": account["id"],
        "activityType": "WITHDRAWAL",
        "activityDate": "2026-04-02",
        "currency": "EUR",
        "amount": "13.00",
        "requestKey": "form-2"
    });
    let stale = business_put(&server, &cookie, &scope, "/ordinary-activities", stale_body).await;
    assert_eq!(stale.0, 400, "{:?}", stale.1);
    assert!(
        stale.1.to_string().contains("version_conflict"),
        "{:?}",
        stale.1
    );

    let updated = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": 1,
            "accountId": account["id"],
            "activityType": "DEPOSIT",
            "activityDate": "2026-04-02",
            "currency": "EUR",
            "amount": "13.00",
            "requestKey": "form-3"
        }),
    )
    .await;
    assert_eq!(updated.1["version"], 2, "{:?}", updated.1);
    assert_eq!(
        updated.1["activity"]["activityType"], "DEPOSIT",
        "{:?}",
        updated.1
    );

    // Business audit: authenticated session, never a person's name.
    let profile = &session_state(&server, &cookie).await["profiles"][0];
    let db = server
        .shared_runtime
        .as_ref()
        .unwrap()
        .db_access
        .connect_rusqlite()
        .unwrap();
    let (authenticated, actor): (i64, String) = db
        .query_row(
            "SELECT authenticated, actor FROM business_audit WHERE operation = 'ordinary_activity_create' ORDER BY created_at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(authenticated, 1);
    assert!(!actor.is_empty());
    for result in [&first, &updated.1] {
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM business_requests r JOIN business_audit a ON a.request_id = r.id JOIN activities e ON e.id = a.entity_id WHERE r.id = ?1 AND e.id = ?2 AND json_extract(r.result_json, '$.activity.id') = e.id",
            [result["requestId"].as_str().unwrap(), activity_id.as_str()],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(
            count, 1,
            "one audit per actual create/update, including retries: {result}"
        );
    }
    let audit_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM business_audit WHERE entity_id = ?1",
            [&activity_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 2,
        "replay and stale update must not add business audits"
    );

    // Exercise the real MCP handler with desktop policy and the same SQLite
    // business services: the web host intentionally blocks ordinary writes.
    let runtime = server.shared_runtime.as_ref().unwrap();
    let service = wealthfolio_mcp::McpServerBuilder::new(runtime.agent_environment.clone())
        .host(wealthfolio_agent_tools::McpHost::Desktop)
        .ledger(profile["id"].as_str().unwrap())
        .audit(std::sync::Arc::new(
            wealthfolio_server::mcp::audit_sink::RepoAuditSink(
                runtime.mcp_audit_repository.clone(),
            ),
        ))
        .build_http_service(
            rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
                .with_stateful_mode(false)
                .with_json_response(true),
        );
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::Extension(wealthfolio_mcp::McpAuthContext {
            actor_kind: wealthfolio_mcp::ActorKind::Pat,
            actor_fingerprint: "sha256:ordinary-regression".to_string(),
            credential_id: Some("pat-ordinary-regression".to_string()),
            granted_scopes: wealthfolio_agent_tools::AgentScopeSet::from_strs([
                "ordinary:write",
                "activities:read",
                "accounts:read",
            ]),
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let transport = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mcp_create = serde_json::json!({
        "accountId": account["id"], "activityType": "WITHDRAWAL",
        "activityDate": "2026-04-03", "currency": "EUR", "amount": "7.25",
        "requestKey": "mcp-create-regression",
    });
    let mut results: Vec<serde_json::Value> = Vec::new();
    for (index, tool) in [
        "create_ordinary_activity",
        "create_ordinary_activity",
        "update_ordinary_activity",
        "update_ordinary_activity",
    ]
    .into_iter()
    .enumerate()
    {
        let args = if index < 2 {
            mcp_create.clone()
        } else {
            serde_json::json!({
                "activityId": results[0]["activity"]["id"], "expectedVersion": 1,
                "accountId": account["id"], "activityType": "WITHDRAWAL",
                "activityDate": "2026-04-03", "currency": "EUR", "amount": "8.25",
                "requestKey": "mcp-update-regression",
            })
        };
        let response: serde_json::Value = server
            .client
            .post(format!("http://{address}/mcp"))
            .header("Accept", "application/json, text/event-stream")
            .header("x-wf-profile-id", profile["id"].as_str().unwrap())
            .json(&serde_json::json!({ "jsonrpc": "2.0", "id": index + 100,
                "method": "tools/call", "params": { "name": tool, "arguments": args } }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_ne!(response["result"]["isError"], true, "{response}");
        let result = response["result"]["structuredContent"]["result"].clone();
        let request_id = result["requestId"].as_str().unwrap();
        let entity_id = result["activity"]["id"].as_str().unwrap();
        let linked: i64 = db.query_row(
            "SELECT COUNT(*) FROM mcp_audit_log c JOIN business_requests r ON r.id = c.business_request_id JOIN business_audit a ON a.request_id = r.id JOIN activities e ON e.id = a.entity_id WHERE r.id = ?1 AND e.id = ?2 AND c.tool = ?3 AND c.outcome = 'success' AND json_extract(r.result_json, '$.activity.id') = e.id",
            [request_id, entity_id, tool], |row| row.get(0),
        ).unwrap();
        assert_eq!(linked, if index % 2 == 0 { 1 } else { 2 }, "{response}");
        assert_eq!(result["replayed"], index % 2 == 1, "{response}");
        if index % 2 == 1 {
            assert_eq!(result["requestId"], results[index - 1]["requestId"]);
        }
        results.push(result);
    }
    let business_audits: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM business_audit WHERE entity_id = ?1",
            [results[0]["activity"]["id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        business_audits, 2,
        "MCP replays must not duplicate business audits"
    );
    transport.abort();
}

/// Pending entries created over MCP keep the creating PAT id, and confirming
/// them over REST creates exactly one activity. A second confirm replays.
#[tokio::test]
async fn pending_entries_are_attributed_and_confirmed_once() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Everyday", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;

    let (status, created) = create_pat(
        &server,
        &cookie,
        serde_json::json!({
            "name": "extractor",
            "scopes": ["activities:read", "activities:draft"]
        }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = created["token"].as_str().unwrap().to_string();
    let token_id = created["id"].as_str().unwrap().to_string();
    let session = mcp_initialize(&server, &pat).await;

    let pending = mcp_call_tool(
        &server,
        &pat,
        &session,
        "create_pending_activity",
        serde_json::json!({
            "activityType": "WITHDRAWAL",
            "accountId": account["id"],
            "activityDate": "2026-04-05",
            "currency": "EUR",
            "amount": "31.40",
            "notes": "Groceries",
            "source": {
                "channel": "telegram",
                "messageId": "msg-9",
                "entryId": "line-1",
                "sourceAccount": "chat-account",
                "excerpt": "paid 31.40"
            },
            "requestKey": "pending-req-1"
        }),
    )
    .await;
    let pending_id = pending["pending"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        pending["pending"]["createdBy"],
        token_id.as_str(),
        "{pending}"
    );
    assert_eq!(
        pending["pending"]["status"],
        "pending".to_string(),
        "{pending}"
    );

    // Full source identity dedupe: another token replaying the same upstream
    // message gets the existing entry, not a duplicate, and does not inherit it.
    let (status, second) = create_pat(
        &server,
        &cookie,
        serde_json::json!({
            "name": "extractor-2",
            "scopes": ["activities:read", "activities:draft"]
        }),
    )
    .await;
    assert_eq!(status, 201);
    let second_pat = second["token"].as_str().unwrap().to_string();
    let second_session = mcp_initialize(&server, &second_pat).await;
    let duplicate = mcp_call_tool(
        &server,
        &second_pat,
        &second_session,
        "create_pending_activity",
        serde_json::json!({
            "activityType": "WITHDRAWAL",
            "accountId": account["id"],
            "activityDate": "2026-04-05",
            "currency": "EUR",
            "amount": "31.40",
            "notes": "Groceries",
            "source": {
                "channel": "telegram",
                "messageId": "msg-9",
                "entryId": "line-1",
                "sourceAccount": "chat-account",
                "excerpt": "paid 31.40"
            },
            "requestKey": "pending-req-2"
        }),
    )
    .await;
    assert_eq!(
        duplicate["pending"]["id"],
        pending_id.as_str(),
        "{duplicate}"
    );
    assert_eq!(
        duplicate["pending"]["createdBy"],
        token_id.as_str(),
        "{duplicate}"
    );

    // Same request key and content replays; different content conflicts.
    let replayed = mcp_call_tool(
        &server,
        &pat,
        &session,
        "create_pending_activity",
        serde_json::json!({
            "activityType": "WITHDRAWAL",
            "accountId": account["id"],
            "activityDate": "2026-04-05",
            "currency": "EUR",
            "amount": "31.40",
            "notes": "Groceries",
            "source": {
                "channel": "telegram",
                "messageId": "msg-9",
                "entryId": "line-1",
                "sourceAccount": "chat-account",
                "excerpt": "paid 31.40"
            },
            "requestKey": "pending-req-1"
        }),
    )
    .await;
    assert_eq!(replayed["pending"]["id"], pending_id.as_str(), "{replayed}");
    assert_eq!(
        replayed["pending"]["requestId"],
        pending["pending"]["requestId"]
    );
    let conflict = mcp_post(
        &server,
        Some(&pat),
        Some(&session),
        serde_json::json!({
            "jsonrpc": "2.0", "id": "pending-conflict", "method": "tools/call",
            "params": { "name": "create_pending_activity", "arguments": {
                "activityType": "WITHDRAWAL",
                "accountId": account["id"],
                "activityDate": "2026-04-05",
                "currency": "EUR",
                "amount": "99.00",
                "source": {
                    "channel": "telegram",
                    "messageId": "msg-9",
                    "entryId": "line-1",
                    "sourceAccount": "chat-account"
                },
                "requestKey": "pending-req-1"
            }}
        }),
    )
    .await;
    let conflict = parse_sse_data(&conflict.text().await.unwrap());
    assert!(
        conflict["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("request_conflict"),
        "{conflict}"
    );

    // A different source account is a different entry even with the same
    // message and entry id; so is a message without an entry id.
    for (key, source_account) in [
        ("pending-req-3", Some("other-account")),
        ("pending-req-4", None),
    ] {
        let mut source = serde_json::json!({
            "channel": "telegram",
            "messageId": "msg-9",
            "entryId": "line-1"
        });
        if let Some(value) = source_account {
            source["sourceAccount"] = serde_json::json!(value);
        } else {
            source.as_object_mut().unwrap().remove("entryId");
        }
        let created = mcp_call_tool(
            &server,
            &pat,
            &session,
            "create_pending_activity",
            serde_json::json!({
                "activityType": "WITHDRAWAL",
                "accountId": account["id"],
                "activityDate": "2026-04-06",
                "currency": "EUR",
                "amount": "7.00",
                "source": source,
                "requestKey": key
            }),
        )
        .await;
        assert_ne!(created["pending"]["id"], pending_id.as_str(), "{created}");
    }

    // The agent can see its entry but cannot confirm it.
    let listed = mcp_call_tool(
        &server,
        &pat,
        &session,
        "list_pending_activities",
        serde_json::json!({}),
    )
    .await;
    // The original entry plus the two distinct-identity entries.
    assert_eq!(listed["pending"].as_array().unwrap().len(), 3, "{listed}");

    // Confirm through the GUI/REST surface.
    let confirmed = api_post(
        &server,
        &cookie,
        &format!("pending-activities/{pending_id}/confirm"),
        serde_json::json!({ "expectedVersion": 1 }),
    )
    .await;
    assert_eq!(confirmed["pending"]["status"], "committed", "{confirmed}");
    // The confirmer is the authenticated session from middleware, recorded
    // separately from the creating token.
    let confirmer = confirmed["pending"]["confirmedBy"].as_str().unwrap();
    assert!(!confirmer.is_empty(), "{confirmed}");
    assert_ne!(confirmer, token_id.as_str(), "{confirmed}");
    assert_eq!(
        confirmed["pending"]["createdBy"],
        token_id.as_str(),
        "{confirmed}"
    );
    let activity_id = confirmed["activity"]["id"].as_str().unwrap().to_string();

    let replay = api_post(
        &server,
        &cookie,
        &format!("pending-activities/{pending_id}/confirm"),
        serde_json::json!({ "expectedVersion": 1 }),
    )
    .await;
    assert_eq!(replay["replayed"], true, "{replay}");
    assert_eq!(replay["activity"]["id"], activity_id, "{replay}");

    let stored = api_post(
        &server,
        &cookie,
        "activities/search",
        serde_json::json!({ "page": 0, "pageSize": 10, "accountIdFilter": account["id"] }),
    )
    .await;
    assert_eq!(stored["data"].as_array().unwrap().len(), 1, "{stored}");
    let profile = &session_state(&server, &cookie).await["profiles"][0];
    let db_path = server
        ._tmp
        .path()
        .join("profiles")
        .join(profile["id"].as_str().unwrap())
        .join("app.db");
    let db = wealthfolio_storage_sqlite::db::DbAccess::plaintext(db_path.to_str().unwrap())
        .connect_rusqlite()
        .unwrap();
    let request_id = pending["pending"]["requestId"].as_str().unwrap();
    let linked_calls: i64 = db.query_row(
        "SELECT COUNT(*) FROM mcp_audit_log c JOIN business_requests r ON r.id = c.business_request_id JOIN business_audit a ON a.request_id = r.id JOIN pending_activities p ON p.id = a.entity_id WHERE r.id = ?1 AND p.id = ?2 AND c.outcome = 'success'",
        [request_id, pending_id.as_str()], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        linked_calls, 2,
        "create and replay link to the same actual request"
    );
    let create_audits: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM business_audit WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        create_audits, 1,
        "replay must not duplicate business audits"
    );
    let duplicate_request = duplicate["pending"]["requestId"].as_str().unwrap();
    assert_ne!(duplicate_request, request_id);
    let duplicate_entity: String = db
        .query_row(
            "SELECT entity_id FROM business_audit WHERE request_id = ?1",
            [duplicate_request],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        duplicate_entity, pending_id,
        "source dedupe audit identifies the existing entity"
    );
    let confirm_audits: i64 = db.query_row(
        "SELECT COUNT(*) FROM business_audit a JOIN business_requests r ON r.id = a.request_id WHERE a.operation = 'pending_activity_confirm' AND a.entity_id = ?1 AND json_extract(r.result_json, '$.activity.id') = ?2",
        [pending_id.as_str(), activity_id.as_str()], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        confirm_audits, 1,
        "confirmation replay keeps one request-backed audit"
    );
    let unlinked_reads: i64 = db.query_row(
        "SELECT COUNT(*) FROM mcp_audit_log WHERE tool = 'list_pending_activities' AND business_request_id IS NULL",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        unlinked_reads, 1,
        "read calls must not use JSON-RPC ids as request links"
    );
}

/// A bad category/event rolls the whole ordinary write back: no activity, no
/// audit, no successful request.
#[tokio::test]
async fn ordinary_write_rolls_back_when_a_follow_up_fails() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Everyday", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;

    let response = server
        .client
        .post(format!("{}/api/v1/ordinary-activities", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .json(&serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-06",
            "currency": "EUR",
            "amount": "19.90",
            "requestKey": "form-rollback",
            "taxonomyId": "spending_categories",
            "categoryId": "missing-category"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400, "{}", response.text().await.unwrap());

    let stored = api_post(
        &server,
        &cookie,
        "activities/search",
        serde_json::json!({ "page": 0, "pageSize": 10, "accountIdFilter": account["id"] }),
    )
    .await;
    assert_eq!(stored["data"].as_array().unwrap().len(), 0, "{stored}");

    let audit = category_api_get(&server, &cookie, "agent-access/audit").await;
    assert!(
        audit["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["tool"] != "create_activity"),
        "failed write must not leave a success audit: {audit}"
    );

    // The request record shares the same transaction: a failed write must not
    // reserve the key or store a result.
    let state = session_state(&server, &cookie).await;
    let db_path = server
        ._tmp
        .path()
        .join("profiles")
        .join(state["profiles"][0]["id"].as_str().unwrap())
        .join("app.db");
    let db = wealthfolio_storage_sqlite::db::DbAccess::plaintext(db_path.to_str().unwrap())
        .connect_rusqlite()
        .unwrap();
    let requests: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM business_requests WHERE request_key = 'form-rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(requests, 0);
}

/// A valid category is committed with the activity in the same transaction.
#[tokio::test]
async fn ordinary_write_commits_category_and_event_follow_ups() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Everyday", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let category = api_post(
        &server,
        &cookie,
        "taxonomies/categories",
        serde_json::json!({
            "taxonomyId": "spending_categories", "name": "Rollback food",
            "key": "rollback_food", "color": "#123456", "sortOrder": 0
        }),
    )
    .await;
    let event_type = api_post(
        &server,
        &cookie,
        "spending/event-types",
        serde_json::json!({ "name": "Trip", "color": "#654321" }),
    )
    .await;
    let event = api_post(
        &server,
        &cookie,
        "spending/events",
        serde_json::json!({
            "name": "Trip 2026", "eventTypeId": event_type["id"],
            "startDate": "2026-04-01", "endDate": "2026-04-30"
        }),
    )
    .await;

    let result = api_post(
        &server,
        &cookie,
        "ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-07",
            "currency": "EUR",
            "amount": "21.00",
            "requestKey": "form-followups",
            "taxonomyId": "spending_categories",
            "categoryId": category["id"],
            "eventId": event["id"]
        }),
    )
    .await;
    let activity_id = result["activity"]["id"].as_str().unwrap();
    let assignments = category_api_get(
        &server,
        &cookie,
        &format!("spending/activities/{activity_id}/assignments"),
    )
    .await;
    assert_eq!(
        assignments[0]["categoryId"], category["id"],
        "{assignments}"
    );

    let rows = api_post(
        &server,
        &cookie,
        "spending/cash-activities/search",
        serde_json::json!({ "accountIds": [account["id"]], "limit": 10 }),
    )
    .await;
    let row = rows["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == activity_id)
        .unwrap_or_else(|| panic!("{rows}"));
    assert_eq!(row["eventId"], event["id"], "{row}");
}

/// Authenticated mode closes the new business endpoints to PATs, missing
/// sessions and invalid session cookies, exactly like the old write routes.
#[tokio::test]
async fn authenticated_mode_rejects_non_session_credentials_on_business_endpoints() {
    let server = spawn_server(true, false).await;
    let cookie = login(&server).await;

    let ordinary_body = serde_json::json!({
        "accountId": "acc",
        "activityType": "WITHDRAWAL",
        "activityDate": "2026-04-01",
        "currency": "USD",
        "amount": "1.00",
        "requestKey": "auth-check"
    });
    for (method, path, body) in [
        ("GET", "/api/v1/pending-activities".to_string(), None),
        (
            "POST",
            "/api/v1/ordinary-activities".to_string(),
            Some(ordinary_body.clone()),
        ),
    ] {
        let build = || match method {
            "GET" => server.client.get(format!("{}{}", server.base, path)),
            _ => server
                .client
                .post(format!("{}{}", server.base, path))
                .json(&body),
        };
        assert_eq!(
            build().send().await.unwrap().status(),
            401,
            "missing session must be rejected for {path}"
        );
        assert_eq!(
            build()
                .header(header::COOKIE, "wf_session=not-a-jwt")
                .send()
                .await
                .unwrap()
                .status(),
            401,
            "invalid session must be rejected for {path}"
        );
    }

    // A PAT never substitutes for a user session, even with write scopes.
    let (status, created) = create_pat(
        &server,
        &cookie,
        serde_json::json!({
            "name": "writer",
            "scopes": ["activities:read", "activities:draft", "activities:write"]
        }),
    )
    .await;
    assert_eq!(status, 201);
    let pat = created["token"].as_str().unwrap();
    let response = server
        .client
        .post(format!("{}/api/v1/ordinary-activities", server.base))
        .header(header::AUTHORIZATION, format!("Bearer {pat}"))
        .json(&ordinary_body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let response = server
        .client
        .get(format!("{}/api/v1/pending-activities", server.base))
        .header(header::AUTHORIZATION, format!("Bearer {pat}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}

/// Without configured auth, a loopback client with the ledger scope can write,
/// but the business audit records the call as unauthenticated — never as a
/// verified person.
#[tokio::test]
async fn unauthenticated_loopback_writes_are_audited_as_anonymous() {
    let server = spawn_server_unauthenticated().await;

    // Establish the anonymous session cookie and ledger scope.
    let state_res = server
        .client
        .post(format!("{}/api/v1/profiles/get_profile_state", server.base))
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(state_res.status(), 200);
    let browser_cookie = state_res
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .unwrap()
        .to_string();
    let state: serde_json::Value = state_res.json().await.unwrap();
    let scope = state["session"]["scopeId"].as_str().unwrap().to_string();

    let account: serde_json::Value = server
        .client
        .post(format!("{}/api/v1/accounts", server.base))
        .header(header::COOKIE, &browser_cookie)
        .header("x-wf-profile-scope", &scope)
        .json(&serde_json::json!({
            "name": "Anonymous cash", "accountType": "CASH", "currency": "USD",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let account_id = account["id"].as_str().unwrap().to_string();
    let response = server
        .client
        .put(format!("{}/api/v1/spending/settings", server.base))
        .header(header::COOKIE, &browser_cookie)
        .header("x-wf-profile-scope", &scope)
        .json(&serde_json::json!({ "enabled": true, "accountIds": [account_id] }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());

    // A reachable anonymous client with the ledger scope can mint a PAT.
    let token_response = server
        .client
        .post(format!("{}/api/v1/agent-access/tokens", server.base))
        .header(header::COOKIE, &browser_cookie)
        .header("x-wf-profile-scope", &scope)
        .json(&serde_json::json!({
            "name": "anonymous-minted",
            "scopes": ["activities:read"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(token_response.status(), 201);
    let token: serde_json::Value = token_response.json().await.unwrap();
    assert!(token["token"].as_str().unwrap().starts_with("wfp_"));

    // The write succeeds without any user session.
    let response = server
        .client
        .post(format!("{}/api/v1/ordinary-activities", server.base))
        .header(header::COOKIE, &browser_cookie)
        .header("x-wf-profile-scope", &scope)
        .json(&serde_json::json!({
            "accountId": account_id,
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-08",
            "currency": "USD",
            "amount": "5.25",
            "requestKey": "anonymous-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());

    // Business audit marks it unauthenticated with the anonymous actor id.
    let profile = &state["profiles"][0];
    let db_path = server
        ._tmp
        .path()
        .join("profiles")
        .join(profile["id"].as_str().unwrap())
        .join("app.db");
    let db = wealthfolio_storage_sqlite::db::DbAccess::plaintext(db_path.to_str().unwrap())
        .connect_rusqlite()
        .unwrap();
    let (authenticated, actor): (i64, String) = db
        .query_row(
            "SELECT authenticated, actor FROM business_audit WHERE operation = 'ordinary_activity_create' ORDER BY created_at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(authenticated, 0);
    assert!(!actor.is_empty());
    assert_ne!(actor, "gui-session");

    // Pending confirmation follows the same rule: anonymous stays anonymous.
    let pending: serde_json::Value = server
        .client
        .post(format!("{}/api/v1/pending-activities", server.base))
        .header(header::COOKIE, &browser_cookie)
        .header("x-wf-profile-scope", &scope)
        .json(&serde_json::json!({
            "activityType": "WITHDRAWAL",
            "accountId": account_id,
            "activityDate": "2026-04-09",
            "currency": "USD",
            "amount": "3.00",
            "source": { "channel": "sms", "messageId": "anon-msg", "entryId": "line-1" },
            "requestKey": "anon-pending-1"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let response = server
        .client
        .post(format!(
            "{}/api/v1/pending-activities/{}/confirm",
            server.base,
            pending["id"].as_str().unwrap()
        ))
        .header(header::COOKIE, &browser_cookie)
        .header("x-wf-profile-scope", &scope)
        .json(&serde_json::json!({ "expectedVersion": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let (confirm_authenticated, confirm_actor): (i64, String) = db
        .query_row(
            "SELECT authenticated, actor FROM business_audit WHERE operation = 'pending_activity_confirm' ORDER BY created_at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(confirm_authenticated, 0);
    assert_ne!(confirm_actor, "gui-session");
}

/// A broker-origin row with an effective override stays on the original cash
/// edit path; the unified update refuses it instead of bypassing protection.
#[tokio::test]
async fn override_rows_keep_the_original_cash_edit_path() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Broker cash", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let created = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "12.00",
            "requestKey": "override-create"
        }),
    )
    .await;
    let activity_id = created["activity"]["id"].as_str().unwrap().to_string();

    // Simulate the broker record whose direction was corrected: base type BUY,
    // effective override WITHDRAWAL.
    let state = session_state(&server, &cookie).await;
    let db_path = server
        ._tmp
        .path()
        .join("profiles")
        .join(state["profiles"][0]["id"].as_str().unwrap())
        .join("app.db");
    let db = wealthfolio_storage_sqlite::db::DbAccess::plaintext(db_path.to_str().unwrap())
        .connect_rusqlite()
        .unwrap();
    db.execute(
        "UPDATE activities SET source_system = 'SNAPTRADE', source_record_id = 'broker-1', \
         activity_type = 'BUY', activity_type_override = 'WITHDRAWAL' WHERE id = ?1",
        [&activity_id],
    )
    .unwrap();
    drop(db);

    let version = api_post(
        &server,
        &cookie,
        "activities/search",
        serde_json::json!({ "page": 0, "pageSize": 5, "accountIdFilter": account["id"] }),
    )
    .await["data"][0]["version"]
        .as_i64()
        .unwrap() as i32;
    let refused = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": version,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "13.00",
            "requestKey": "override-unified"
        }),
    )
    .await;
    assert_eq!(refused.0, 400, "{:?}", refused.1);
    assert!(
        refused.1.to_string().contains("activity_out_of_scope"),
        "{:?}",
        refused.1
    );

    // The original cash edit path still updates the amount.
    let response = server
        .client
        .put(format!("{}/api/v1/activities", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .header("x-wf-profile-scope", &scope)
        .json(&serde_json::json!({
            "id": activity_id,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "13.00"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let stored = api_post(
        &server,
        &cookie,
        "activities/search",
        serde_json::json!({ "page": 0, "pageSize": 5, "accountIdFilter": account["id"] }),
    )
    .await;
    assert_eq!(stored["data"][0]["amount"], "13.00", "{stored}");
}

/// The scoped ordinary query filters scope and status before counting,
/// clamps the page size, and orders by date then id.
#[tokio::test]
async fn ordinary_query_filters_before_counting_and_clamps_page_size() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Query cash", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    for (index, day) in ["01", "02", "03"].iter().enumerate() {
        business_post(
            &server,
            &cookie,
            &scope,
            "/ordinary-activities",
            serde_json::json!({
                "accountId": account["id"],
                "activityType": "WITHDRAWAL",
                "activityDate": format!("2026-04-{day}"),
                "currency": "EUR",
                "amount": "10.00",
                "requestKey": format!("query-{index}")
            }),
        )
        .await;
    }

    let page = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities/query",
        serde_json::json!({ "page": 0, "pageSize": 1 }),
    )
    .await;
    assert_eq!(page["totalCount"], 3, "{page}");
    assert_eq!(page["rows"].as_array().unwrap().len(), 1, "{page}");
    assert_eq!(
        page["rows"][0]["activityDate"].as_str().unwrap(),
        "2026-04-03T00:00:00+00:00"
    );

    let clamped = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities/query",
        serde_json::json!({ "pageSize": 1000 }),
    )
    .await;
    assert_eq!(clamped["pageSize"], 200, "{clamped}");

    // An account with no entries has no ordinary results, even when eligible.
    let other = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Not opted in", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    let empty = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities/query",
        serde_json::json!({ "accountIds": [other["id"]] }),
    )
    .await;
    assert_eq!(empty["totalCount"], 0, "{empty}");
}

/// A broker-origin direction change keeps the base type and records the new
/// direction in the override; subsequent edits are therefore refused by the
/// unified path and stay on the old cash edit path.
#[tokio::test]
async fn broker_direction_change_keeps_base_type_and_writes_override() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Broker direction", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let created = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "10.00",
            "requestKey": "broker-base"
        }),
    )
    .await;
    let activity_id = created["activity"]["id"].as_str().unwrap().to_string();
    let state = session_state(&server, &cookie).await;
    let db_path = server
        ._tmp
        .path()
        .join("profiles")
        .join(state["profiles"][0]["id"].as_str().unwrap())
        .join("app.db");
    let db = wealthfolio_storage_sqlite::db::DbAccess::plaintext(db_path.to_str().unwrap())
        .connect_rusqlite()
        .unwrap();
    db.execute(
        "UPDATE activities SET source_system = 'SNAPTRADE', source_record_id = 'broker-2', \
         activity_type = 'WITHDRAWAL', activity_type_override = NULL WHERE id = ?1",
        [&activity_id],
    )
    .unwrap();
    drop(db);

    let version = api_post(
        &server,
        &cookie,
        "activities/search",
        serde_json::json!({ "page": 0, "pageSize": 5, "accountIdFilter": account["id"] }),
    )
    .await["data"][0]["version"]
        .as_i64()
        .unwrap() as i32;
    let updated = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": version,
            "accountId": account["id"],
            "activityType": "DEPOSIT",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "10.00",
            "requestKey": "broker-direction"
        }),
    )
    .await;
    assert_eq!(updated.0, 200, "{:?}", updated.1);

    // The commit response exposes the stored base type and the override.
    assert_eq!(
        updated.1["activity"]["activityType"], "WITHDRAWAL",
        "{:?}",
        updated.1
    );
    assert_eq!(
        updated.1["activity"]["activityTypeOverride"], "DEPOSIT",
        "{:?}",
        updated.1
    );
    let stored = api_post(
        &server,
        &cookie,
        "activities/search",
        serde_json::json!({ "page": 0, "pageSize": 5, "accountIdFilter": account["id"] }),
    )
    .await;
    assert_eq!(
        stored["data"][0]["activityTypeOverride"], "DEPOSIT",
        "{stored}"
    );

    // The same request is a replay even though the record now carries an
    // override: retries must return the stored result, not a scope error.
    let retried = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": version,
            "accountId": account["id"],
            "activityType": "DEPOSIT",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "10.00",
            "requestKey": "broker-direction"
        }),
    )
    .await;
    assert_eq!(retried.0, 200, "{:?}", retried.1);
    assert_eq!(retried.1["replayed"], true, "{:?}", retried.1);
}

/// Changing the amount invalidates existing split lines, matching the old
/// cash edit rules; they must not survive with the old total.
#[tokio::test]
async fn changing_amount_clears_stale_splits() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Split cash", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let category = api_post(
        &server,
        &cookie,
        "taxonomies/categories",
        serde_json::json!({
            "taxonomyId": "spending_categories", "name": "Split food",
            "key": "split_food", "color": "#123456", "sortOrder": 0
        }),
    )
    .await;
    let created = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "12.50",
            "requestKey": "split-create"
        }),
    )
    .await;
    let activity_id = created["activity"]["id"].as_str().unwrap().to_string();
    let splits_path = format!("spending/activities/{activity_id}/splits");
    api_put(
        &server,
        &cookie,
        &splits_path,
        serde_json::json!([
            { "taxonomyId": "spending_categories", "categoryId": category["id"], "amount": "5.00" },
            { "taxonomyId": "spending_categories", "categoryId": category["id"], "amount": "7.50" }
        ]),
    )
    .await;
    let splits = category_api_get(&server, &cookie, &splits_path).await;
    assert_eq!(splits.as_array().unwrap().len(), 2, "{splits}");

    let updated = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": 1,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "40.00",
            "requestKey": "split-update"
        }),
    )
    .await;
    assert_eq!(updated.0, 200, "{:?}", updated.1);
    let splits = category_api_get(&server, &cookie, &splits_path).await;
    assert!(splits.as_array().unwrap().is_empty(), "{splits}");
}

/// Update FX-rate patch semantics: omitted preserves, explicit null clears.
#[tokio::test]
async fn ordinary_update_preserves_or_clears_fx_rate() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "FX cash", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let created = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "USD",
            "amount": "10.00",
            "fee": "1.20",
            "fxRate": "1.5",
            "requestKey": "fx-create"
        }),
    )
    .await;
    let activity_id = created["activity"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["activity"]["fxRate"], "1.5", "{created}");

    let preserved = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": 1,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "USD",
            "amount": "11.00",
            "requestKey": "fx-omit"
        }),
    )
    .await;
    assert_eq!(preserved.0, 200, "{:?}", preserved.1);
    assert_eq!(
        preserved.1["activity"]["fxRate"], "1.5",
        "{:?}",
        preserved.1
    );

    let cleared = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": 2,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-01",
            "currency": "EUR",
            "amount": "11.00",
            "fee": null,
            "fxRate": null,
            "requestKey": "fx-clear"
        }),
    )
    .await;
    assert_eq!(cleared.0, 200, "{:?}", cleared.1);
    assert!(cleared.1["activity"]["fxRate"].is_null(), "{:?}", cleared.1);
    assert!(cleared.1["activity"]["fee"].is_null(), "{:?}", cleared.1);
}

/// Zero is a legitimate fee/tax value; only the activity amount must be nonzero.
#[tokio::test]
async fn ordinary_create_accepts_zero_fee_and_tax() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Zero fee", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let created = api_post(
        &server,
        &cookie,
        "ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-08",
            "currency": "EUR",
            "amount": "12.00",
            "fee": "0",
            "tax": "0.0000",
            "requestKey": "zero-fee"
        }),
    )
    .await;
    assert_eq!(created["activity"]["fee"], "0", "{created}");
    assert_eq!(created["activity"]["tax"], "0.0000", "{created}");
}

/// Clearing the single-select category removes the stored assignment.
#[tokio::test]
async fn ordinary_update_clears_selected_category() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Clear category", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let category = api_post(
        &server,
        &cookie,
        "taxonomies/categories",
        serde_json::json!({
            "taxonomyId": "spending_categories", "name": "Clearable",
            "key": "clearable", "color": "#123456", "sortOrder": 0
        }),
    )
    .await;
    let created = api_post(
        &server,
        &cookie,
        "ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-09",
            "currency": "EUR",
            "amount": "8.00",
            "requestKey": "clear-category-create",
            "taxonomyId": "spending_categories",
            "categoryId": category["id"]
        }),
    )
    .await;
    let activity_id = created["activity"]["id"].as_str().unwrap();
    let assigned = category_api_get(
        &server,
        &cookie,
        &format!("spending/activities/{activity_id}/assignments"),
    )
    .await;
    assert_eq!(assigned.as_array().unwrap().len(), 1, "{assigned}");

    let updated = api_put(
        &server,
        &cookie,
        "ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": created["version"],
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-09",
            "currency": "EUR",
            "amount": "8.00",
            "requestKey": "clear-category-update",
            "taxonomyId": null,
            "categoryId": null
        }),
    )
    .await;
    assert_eq!(updated["version"], 2, "{updated}");
    let cleared = category_api_get(
        &server,
        &cookie,
        &format!("spending/activities/{activity_id}/assignments"),
    )
    .await;
    assert_eq!(cleared.as_array().unwrap().len(), 0, "{cleared}");
}

/// Archiving the account after entry creation blocks confirmation.
#[tokio::test]
async fn archived_account_cannot_be_confirmed() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Archived cash", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let pending = api_post(
        &server,
        &cookie,
        "pending-activities",
        serde_json::json!({
            "activityType": "WITHDRAWAL",
            "accountId": account["id"],
            "activityDate": "2026-04-10",
            "currency": "EUR",
            "amount": "6.00",
            "requestKey": "archive-confirm"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        &format!("accounts/{}", account["id"].as_str().unwrap()),
        serde_json::json!({
            "id": account["id"],
            "name": "Archived cash", "accountType": "CASH", "isDefault": false,
            "isActive": true, "isArchived": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    let response = server
        .client
        .post(format!(
            "{}/api/v1/pending-activities/{}/confirm",
            server.base,
            pending["id"].as_str().unwrap()
        ))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .json(&serde_json::json!({ "expectedVersion": 1 }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("archived"), "{body}");
}

/// Omitted patch fields preserve; explicit null clears. The request snapshot
/// must keep that distinction, otherwise the same key would replay a different
/// operation as if it were the original.
#[tokio::test]
async fn same_request_key_distinguishes_omitted_from_null_patch() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Hash patch", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;
    let created = api_post(
        &server,
        &cookie,
        "ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-11",
            "currency": "EUR",
            "amount": "9.00",
            "requestKey": "hash-base"
        }),
    )
    .await;
    let activity_id = created["activity"]["id"].as_str().unwrap();
    let version = created["version"].as_i64().unwrap();

    // First request omits the classification and the fee patch fields.
    let first = api_put(
        &server,
        &cookie,
        "ordinary-activities",
        serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": version,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-11",
            "currency": "EUR",
            "amount": "9.00",
            "requestKey": "hash-patch"
        }),
    )
    .await;
    assert_eq!(first["version"], version + 1, "{first}");

    // Same key, same values, but now explicit nulls: different operations.
    let conflict = server
        .client
        .put(format!("{}/api/v1/ordinary-activities", server.base))
        .header(header::COOKIE, format!("wf_session={cookie}"))
        .json(&serde_json::json!({
            "activityId": activity_id,
            "expectedVersion": version,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-04-11",
            "currency": "EUR",
            "amount": "9.00",
            "requestKey": "hash-patch",
            "taxonomyId": null,
            "categoryId": null,
            "fee": null,
            "tax": null,
            "fxRate": null
        }))
        .send()
        .await
        .unwrap();
    let status = conflict.status();
    let body = conflict.text().await.unwrap();
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("request_conflict"), "{body}");
}

/// Dedupe key semantics: an explicit key allows intentional independent rows,
/// blanks are rejected, and moving an existing row's amount does not rewrite
/// its dedupe key.
#[tokio::test]
async fn ordinary_idempotency_key_semantics_and_duplicate_errors() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Dedupe", "accountType": "CASH", "currency": "EUR",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({ "enabled": true, "accountIds": [account["id"]] }),
    )
    .await;

    let original = serde_json::json!({
        "accountId": account["id"],
        "activityType": "WITHDRAWAL",
        "activityDate": "2026-05-01",
        "currency": "EUR",
        "amount": "30.00",
        "notes": "Coffee",
        "requestKey": "dedupe-1"
    });
    let first = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        original.clone(),
    )
    .await;
    let first_id = first["activity"]["id"].as_str().unwrap().to_string();

    let (status, changed) = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": first_id,
            "expectedVersion": first["version"],
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-05-01",
            "currency": "EUR",
            "amount": "35.00",
            "notes": "Coffee",
            "requestKey": "dedupe-2"
        }),
    )
    .await;
    assert_eq!(status, 200, "{changed}");
    assert_eq!(changed["activity"]["amount"], "35.00", "{changed}");
    assert_eq!(changed["version"], 2, "{changed}");

    // The update kept the old content key: recreating the original 30-unit row
    // now hits it even though the stored row says 35.
    let (status, duplicate) = business_post_status(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-05-01",
            "currency": "EUR",
            "amount": "30.00",
            "notes": "Coffee",
            "requestKey": "dedupe-3"
        }),
    )
    .await;
    assert_eq!(status, 400, "{duplicate}");
    assert!(
        duplicate.to_string().contains("storage_error: storage error: Activity error: Invalid data: Duplicate activity detected"),
        "{duplicate}"
    );
    assert!(
        !duplicate.to_string().contains("duplicate_activity"),
        "legacy REST body must not adopt the new code: {duplicate}"
    );

    // A new request key for the original frozen request still replays the
    // first stored result instead of reporting a duplicate.
    let replay = business_post(&server, &cookie, &scope, "/ordinary-activities", original).await;
    assert_eq!(replay["replayed"], true, "{replay}");
    assert_eq!(replay["activity"]["id"], first_id, "{replay}");

    // Different content under the same request key is a request conflict, not
    // a duplicate.
    let (status, conflict) = business_post_status(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-05-01",
            "currency": "EUR",
            "amount": "31.00",
            "notes": "Coffee",
            "requestKey": "dedupe-1"
        }),
    )
    .await;
    assert_eq!(status, 400, "{conflict}");
    assert!(
        conflict.to_string().contains("request_conflict"),
        "{conflict}"
    );

    // An explicit key is an intentional independent row with identical
    // economics; it must not be mistaken for the fingerprint.
    let independent = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-05-01",
            "currency": "EUR",
            "amount": "30.00",
            "notes": "Coffee",
            "requestKey": "dedupe-4",
            "idempotencyKey": "  manual-independent-1  "
        }),
    )
    .await;
    assert_ne!(independent["activity"]["id"], first_id, "{independent}");
    assert_eq!(independent["replayed"], false, "{independent}");

    // Blank explicit keys are invalid rather than silently falling back.
    for blank in ["", "   "] {
        let (status, invalid) = business_post_status(
            &server,
            &cookie,
            &scope,
            "/ordinary-activities",
            serde_json::json!({
                "accountId": account["id"],
                "activityType": "WITHDRAWAL",
                "activityDate": "2026-05-02",
                "currency": "EUR",
                "amount": "9.00",
                "requestKey": format!("dedupe-blank-{blank}"),
                "idempotencyKey": blank
            }),
        )
        .await;
        assert_eq!(status, 400, "{invalid}");
        assert!(invalid.to_string().contains("invalid_input"), "{invalid}");
    }

    // Updates never accept an idempotency key.
    let (status, rejected) = business_put(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        serde_json::json!({
            "activityId": first_id,
            "expectedVersion": 2,
            "accountId": account["id"],
            "activityType": "WITHDRAWAL",
            "activityDate": "2026-05-01",
            "currency": "EUR",
            "amount": "35.00",
            "requestKey": "dedupe-5",
            "idempotencyKey": "nope"
        }),
    )
    .await;
    assert_eq!(status, 400, "{rejected}");
    assert!(rejected.to_string().contains("invalid_input"), "{rejected}");

    // Single read returns the complete editable field set and current version.
    let detail = business_get(
        &server,
        &cookie,
        &scope,
        &format!("/ordinary-activities/{first_id}"),
    )
    .await;
    assert_eq!(detail["id"], first_id);
    assert_eq!(detail["version"], 2, "{detail}");
    assert_eq!(detail["amount"], "35.00", "{detail}");
    assert_eq!(
        detail["activityDate"], "2026-05-01T00:00:00+00:00",
        "{detail}"
    );
    assert_eq!(detail["status"], "POSTED", "{detail}");
}

/// Message bookkeeping works before report enrollment. Context exposes only
/// activity categories and supplies the IDs needed for one atomic classified write.
#[tokio::test]
async fn message_bookkeeping_context_and_classified_write_without_report_enrollment() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    let account = api_post(
        &server,
        &cookie,
        "accounts",
        serde_json::json!({
            "name": "Bank (0407)", "accountType": "CASH", "currency": "CNY",
            "isDefault": false, "isActive": true, "trackingMode": "TRANSACTIONS"
        }),
    )
    .await;
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({
            "enabled": false, "accountIds": []
        }),
    )
    .await;
    let category = api_post(
        &server,
        &cookie,
        "taxonomies/categories",
        serde_json::json!({
            "taxonomyId": "spending_categories", "name": "Digital services",
            "key": "message_digital", "color": "#123456", "sortOrder": 0
        }),
    )
    .await;
    let (_, token) = create_pat(&server, &cookie, serde_json::json!({
        "name": "message-reader", "scopes": ["accounts:read", "activities:read", "classification:read"]
    })).await;
    let pat = token["token"].as_str().unwrap();
    let session = mcp_initialize(&server, pat).await;
    let context = mcp_call_tool(
        &server,
        pat,
        &session,
        "get_bookkeeping_context",
        serde_json::json!({}),
    )
    .await;
    assert!(
        context["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == account["id"]),
        "{context}"
    );
    assert!(
        context["taxonomies"].as_array().unwrap().iter().any(|t| {
            t["taxonomyId"] == "spending_categories"
                && t["categories"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["categoryId"] == category["id"])
        }),
        "{context}"
    );
    assert!(
        context["recentCategorizedActivities"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{context}"
    );
    let input = serde_json::json!({
        "accountId": account["id"], "activityType": "WITHDRAWAL", "activityDate": "2026-10-28",
        "currency": "USD", "amount": "29.19", "notes": "GOOGLE digital service",
        "taxonomyId": "spending_categories", "categoryId": category["id"], "requestKey": "message-google"
    });
    let created = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities",
        input.clone(),
    )
    .await;
    assert_eq!(created["version"], 1, "{created}");
    let replay = business_post(&server, &cookie, &scope, "/ordinary-activities", input).await;
    assert_eq!(replay["replayed"], true, "{replay}");
    let id = created["activity"]["id"].as_str().unwrap();
    let detail = business_get(
        &server,
        &cookie,
        &scope,
        &format!("/ordinary-activities/{id}"),
    )
    .await;
    assert_eq!(detail["currency"], "USD", "{detail}");
    assert_eq!(detail["amount"], "29.19", "{detail}");
    assert_eq!(detail["categoryId"], category["id"], "{detail}");
    assert!(detail["fxRate"].is_null(), "{detail}");
    let context = mcp_call_tool(
        &server,
        pat,
        &session,
        "get_bookkeeping_context",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(
        context["recentCategorizedActivities"][0]["id"], id,
        "{context}"
    );
    let query = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities/query",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(query["totalCount"], 1, "{query}");
    // Enabled reporting with the account explicitly excluded also allows bookkeeping.
    api_put(
        &server,
        &cookie,
        "spending/settings",
        serde_json::json!({"enabled": true, "accountIds": []}),
    )
    .await;
    let updated = business_put(&server, &cookie, &scope, "/ordinary-activities", serde_json::json!({
        "activityId": id, "expectedVersion": 1, "accountId": account["id"],
        "activityType": "WITHDRAWAL", "activityDate": "2026-10-28", "currency": "USD", "amount": "29.19",
        "notes": "GOOGLE corrected note", "requestKey": "message-google-note"
    })).await;
    assert_eq!(updated.0, 200, "{updated:?}");
    let settings = category_api_get(&server, &cookie, "spending/settings").await;
    assert_eq!(settings["accountIds"], serde_json::json!([]), "{settings}");
}

#[tokio::test]
async fn ordinary_bookkeeping_accepts_credit_cards_and_rejects_ineligible_accounts() {
    let server = spawn_server(true, true).await;
    let cookie = login(&server).await;
    disable_market_data_providers(&server, &cookie).await;
    let scope = session_scope(&server, &cookie).await;
    for (name, account_type, active, expected) in [
        ("Card", "CREDIT_CARD", true, 200),
        ("Inactive cash", "CASH", false, 400),
        ("Broker", "SECURITIES", true, 400),
    ] {
        let account = api_post(
            &server,
            &cookie,
            "accounts",
            serde_json::json!({
                "name": name, "accountType": account_type, "currency": "USD",
                "isDefault": false, "isActive": active, "trackingMode": "TRANSACTIONS"
            }),
        )
        .await;
        let (status, result) = business_post_status(&server, &cookie, &scope, "/ordinary-activities", serde_json::json!({
            "accountId": account["id"], "activityType": "WITHDRAWAL", "activityDate": "2026-10-28",
            "currency": "USD", "amount": "10", "requestKey": format!("bookkeeping-{account_type}-{active}")
        })).await;
        assert_eq!(status, expected, "{result}");
    }
    let query = business_post(
        &server,
        &cookie,
        &scope,
        "/ordinary-activities/query",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(query["totalCount"], 1, "{query}");
}
