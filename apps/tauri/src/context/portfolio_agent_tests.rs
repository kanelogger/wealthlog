//! End-to-end portfolio bookkeeping through the desktop Agent catalog, with
//! real services and an isolated SQLite ledger. No windows or real credentials.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
use super::*;
use chrono::Duration;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use wealthfolio_agent_tools::{AgentScopeSet, AgentToolCatalog, McpHost, ToolCallContext};
use wealthfolio_core::portfolio::snapshot::{AccountStateSnapshot, Position, SnapshotSource};
use wealthfolio_core::utils::time_utils::{parse_user_timezone_or_default, user_today};

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get_secret(&self, _: &str) -> wealthfolio_core::errors::Result<Option<String>> {
        Ok(None)
    }
    fn set_secret(&self, _: &str, _: &str) -> wealthfolio_core::errors::Result<()> {
        Ok(())
    }
    fn delete_secret(&self, _: &str) -> wealthfolio_core::errors::Result<()> {
        Ok(())
    }
}
struct Fixture {
    init: ContextInitResult,
    access: db::DbAccess,
    _dir: tempfile::TempDir,
    day: String,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let access = db::DbAccess::plaintext(dir.path().join("ledger.db").to_str().unwrap());
        access.run_migrations().unwrap();
        access.connect_rusqlite().unwrap().execute_batch("\
            INSERT INTO accounts (id,name,account_type,currency,tracking_mode) VALUES
            ('broker','Broker','SECURITIES','CNY','HOLDINGS'),('bank','Bank','CASH','CNY','TRANSACTIONS');
            INSERT INTO assets (id,kind,quote_ccy,instrument_type,quote_mode) VALUES
            ('etf','INVESTMENT','CNY','EQUITY','MARKET'),('other','INVESTMENT','CNY','EQUITY','MARKET');
        ").unwrap();
        let init = initialize_with_pool(
            access.create_pool().unwrap(),
            uuid::Uuid::new_v4(),
            Arc::new(NoSecrets),
        )
        .await
        .unwrap();
        *init.context.base_currency.write().unwrap() = "CNY".into();
        *init.context.timezone.write().unwrap() = "Asia/Shanghai".into();
        let today = user_today(parse_user_timezone_or_default("Asia/Shanghai"));
        let position = |id: &str, quantity: &str, cost: &str| Position {
            id: format!("p-{id}"),
            account_id: "broker".into(),
            asset_id: id.into(),
            currency: "CNY".into(),
            quantity: quantity.parse().unwrap(),
            average_cost: cost.parse().unwrap(),
            total_cost_basis: quantity.parse::<Decimal>().unwrap()
                * cost.parse::<Decimal>().unwrap(),
            ..Position::default()
        };
        let snapshot = AccountStateSnapshot {
            account_id: "broker".into(),
            currency: "CNY".into(),
            snapshot_date: today - Duration::days(1),
            source: SnapshotSource::ManualEntry,
            positions: [
                ("etf".into(), position("etf", "38500", "1.6849")),
                ("other".into(), position("other", "7", "20")),
            ]
            .into(),
            cash_balances: [
                ("CNY".into(), "62002.17".parse().unwrap()),
                ("USD".into(), Decimal::ZERO),
            ]
            .into(),
            ..Default::default()
        };
        init.context
            .snapshot_service
            .save_manual_snapshot("broker", snapshot)
            .await
            .unwrap();
        Self {
            init,
            access,
            _dir: dir,
            day: today.to_string(),
        }
    }
    async fn call(
        &self,
        name: &str,
        args: Value,
    ) -> std::result::Result<Value, wealthfolio_agent_tools::AgentToolError> {
        let scopes = AgentScopeSet::from_strs([
            "accounts:read",
            "holdings:read",
            "activities:read",
            "transfers:write",
            "holdings:write",
        ]);
        let context = ToolCallContext::pat("test-pat", Some("test-pat".into()), scopes);
        AgentToolCatalog::mcp_catalog_for(McpHost::Desktop)
            .execute_with_context(
                self.init.context.agent_environment.clone(),
                &context,
                name,
                args,
            )
            .await
            .map(|r| r.content)
    }
    async fn state(&self) -> Value {
        self.call(
            "get_investment_account_state",
            json!({"accountId":"broker"}),
        )
        .await
        .unwrap()
    }
    fn transfer(&self, key: &str) -> Value {
        json!({"fromAccountId":"bank","toAccountId":"broker","activityDate":self.day,"sourceCurrency":"CNY","destinationCurrency":"CNY","sourceAmount":"5000","requestKey":key})
    }
    fn count(&self, table: &str) -> i64 {
        self.access
            .connect_rusqlite()
            .unwrap()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    async fn stop(self) {
        self.init.writer.shutdown().await;
        self.init.writer_task.join().await;
    }
}

#[tokio::test]
async fn script_and_mcp_operations_cover_the_cash_transfer_manual_buy_and_reconciliation() {
    let f = Fixture::new().await;
    let t = f
        .call("create_internal_cash_transfer", f.transfer("transfer-1"))
        .await
        .unwrap();
    assert_eq!(t["result"]["activityIds"].as_array().unwrap().len(), 2);
    let back = json!({"fromAccountId":"broker","toAccountId":"bank","activityDate":f.day,"sourceCurrency":"CNY","destinationCurrency":"CNY","sourceAmount":"104.18","requestKey":"back-1"});
    f.call("create_internal_cash_transfer", back).await.unwrap();
    let state = f.state().await;
    assert_eq!(state["cashBalances"]["CNY"], "66897.99");
    let buy = json!({"accountId":"broker","activityDate":f.day,"expectedStateVersion":state["stateVersion"],"assetId":"etf","side":"BUY","quantity":"3000","price":"1.298","currency":"CNY","observedCashBalance":"63007.83","requestKey":"buy-1"});
    let result = f.call("record_manual_trade", buy.clone()).await.unwrap();
    assert_eq!(result["result"]["positionQuantity"], "41500");
    assert_eq!(result["result"]["totalCostBasis"], "68762.65");
    assert_eq!(result["result"]["cashAdjustment"], "3.84");
    assert_eq!(result["result"]["feesProvided"], false);
    let replay = f.call("record_manual_trade", buy.clone()).await.unwrap();
    assert_eq!(replay["result"]["requestId"], result["result"]["requestId"]);
    assert_eq!(replay["result"]["replayed"], true);
    let mut drift = buy;
    drift["quantity"] = json!("1");
    assert_eq!(
        f.call("record_manual_trade", drift)
            .await
            .unwrap_err()
            .structured_value()["code"],
        "request_conflict"
    );
    let state = f.state().await;
    assert_eq!(state["cashBalances"]["CNY"], "63007.83");
    assert_eq!(state["cashBalances"]["USD"], "0");
    assert_eq!(state["positions"].as_array().unwrap().len(), 2);
    assert_eq!(f.count("activities"), 4); // manual trades do not pretend to be transaction-mode activities
    assert_eq!(f.count("business_requests"), 3);
    assert_eq!(f.count("business_audit"), 3);
    let c = f.access.connect_rusqlite().unwrap();
    let position_count:i64=c.query_row("SELECT count(*) FROM snapshot_positions WHERE snapshot_id=(SELECT id FROM holdings_snapshots ORDER BY snapshot_date DESC LIMIT 1)",[],|r|r.get(0)).unwrap();
    assert_eq!(position_count, 2);
    drop(c);
    f.stop().await;
}
#[tokio::test]
async fn concurrent_retries_create_one_transfer_pair_and_a_new_key_does_not_duplicate_it() {
    let f = Fixture::new().await;
    let args = f.transfer("concurrent");
    let (a, b) = tokio::join!(
        f.call("create_internal_cash_transfer", args.clone()),
        f.call("create_internal_cash_transfer", args.clone())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a["result"]["requestId"], b["result"]["requestId"]);
    assert_eq!(f.count("activities"), 2);
    assert_eq!(f.count("business_audit"), 1);
    let mut duplicate = args;
    duplicate["requestKey"] = json!("different-request");
    assert_eq!(
        f.call("create_internal_cash_transfer", duplicate)
            .await
            .unwrap_err()
            .structured_value()["code"],
        "duplicate_activity"
    );
    assert_eq!(f.count("activities"), 2);
    f.stop().await;
}
#[tokio::test]
async fn stale_snapshot_patch_never_overwrites_another_writer_and_preserves_unspecified_fields() {
    let f = Fixture::new().await;
    let state = f.state().await;
    let patch = json!({"accountId":"broker","snapshotDate":f.day,"expectedStateVersion":state["stateVersion"],"cashBalances":{"CNY":"63007.83"},"notes":"observed balance","requestKey":"patch-1"});
    let result = f
        .call("patch_holdings_snapshot", patch.clone())
        .await
        .unwrap();
    assert_eq!(result["result"]["cashAdjustments"]["CNY"], "1005.66");
    let mut stale = patch;
    stale["requestKey"] = json!("stale");
    stale["cashBalances"]["CNY"] = json!("1");
    assert_eq!(
        f.call("patch_holdings_snapshot", stale)
            .await
            .unwrap_err()
            .structured_value()["code"],
        "state_conflict"
    );
    let state = f.state().await;
    assert_eq!(state["cashBalances"]["CNY"], "63007.83");
    assert_eq!(state["positions"].as_array().unwrap().len(), 2);
    assert_eq!(state["cashBalances"]["USD"], "0");
    f.stop().await;
}
#[tokio::test]
async fn audit_failure_rolls_back_both_transfer_legs_and_replay_record() {
    let f = Fixture::new().await;
    f.access.connect_rusqlite().unwrap().execute_batch("CREATE TRIGGER reject_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'test audit failure'); END;").unwrap();
    assert!(f
        .call("create_internal_cash_transfer", f.transfer("fail"))
        .await
        .is_err());
    assert_eq!(f.count("activities"), 0);
    assert_eq!(f.count("business_requests"), 0);
    assert_eq!(f.state().await["cashBalances"]["CNY"], "62002.17");
    f.stop().await;
}
#[tokio::test]
async fn trade_validation_rejects_overselling_and_partial_or_cross_currency_transfers() {
    let f = Fixture::new().await;
    let state = f.state().await;
    let sell = json!({"accountId":"broker","activityDate":f.day,"expectedStateVersion":state["stateVersion"],"assetId":"etf","side":"SELL","quantity":"40000","price":"1.298","currency":"CNY","requestKey":"sell"});
    assert_eq!(
        f.call("record_manual_trade", sell)
            .await
            .unwrap_err()
            .structured_value()["code"],
        "invalid_input"
    );
    let mut transfer = f.transfer("cross");
    transfer["destinationCurrency"] = json!("USD");
    assert_eq!(
        f.call("create_internal_cash_transfer", transfer)
            .await
            .unwrap_err()
            .structured_value()["code"],
        "invalid_input"
    );
    assert_eq!(f.count("activities"), 0);
    assert_eq!(f.count("business_requests"), 0);
    f.stop().await;
}

#[tokio::test]
async fn authenticated_http_mcp_lists_and_executes_portfolio_tools_with_explicit_ledger() {
    use tokio_util::sync::CancellationToken;
    use wealthfolio_storage_sqlite::agent::NewPersonalAccessToken;
    let f = Fixture::new().await;
    let token = wealthfolio_mcp::pat::generate_token();
    f.init
        .context
        .pat_repository
        .create(NewPersonalAccessToken {
            name: "isolated-portfolio-test".into(),
            token_prefix: wealthfolio_mcp::pat::token_prefix(&token).unwrap().into(),
            token_hash: wealthfolio_mcp::pat::hash_token(&token),
            scopes_json: json!([
                "accounts:read",
                "activities:read",
                "holdings:read",
                "transfers:write",
                "holdings:write"
            ])
            .to_string(),
            expires_at: None,
        })
        .await
        .unwrap();
    let ledger = f.init.context.profile_id.to_string();
    let cancel = CancellationToken::new();
    let router = crate::mcp::server::build_static_router(
        f.init.context.agent_environment.clone(),
        None,
        f.init.context.pat_repository.clone(),
        cancel.clone(),
        ledger.clone(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let shutdown = cancel.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap()
    });
    let client = reqwest::Client::new();
    let init=client.post(&url).header("Content-Type","application/json").header("Accept","application/json, text/event-stream")
        .bearer_auth(&token).json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"portfolio-test","version":"1"}}}))
        .send().await.unwrap();
    let session = init
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let parse = |text: String| {
        text.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
            .unwrap()
    };
    let _ = init.text().await.unwrap();
    let post = |body: Value, bind: bool| {
        let mut request = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .bearer_auth(&token)
            .json(&body);
        if bind {
            request = request.header("x-wf-profile-id", &ledger);
        }
        request.send()
    };
    let list = parse(
        post(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), true)
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
    );
    let names: Vec<_> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"create_internal_cash_transfer"));
    assert!(names.contains(&"record_manual_trade"));
    let call = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_internal_cash_transfer","arguments":f.transfer("http-transfer")}});
    let unbound = parse(
        post(call.clone(), false)
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
    );
    assert_eq!(unbound["error"]["data"]["code"], "profile_required");
    assert_eq!(f.count("activities"), 0);
    let saved = parse(post(call, true).await.unwrap().text().await.unwrap());
    assert_eq!(saved["result"]["isError"], false, "{saved}");
    assert_eq!(f.count("activities"), 2);
    let read=parse(post(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_investment_account_state","arguments":{"accountId":"broker"}}}),true).await.unwrap().text().await.unwrap());
    assert_eq!(
        read["result"]["structuredContent"]["cashBalances"]["CNY"], "67002.17",
        "{read}"
    );
    cancel.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    f.stop().await;
}

#[tokio::test]
async fn an_unobserved_manual_account_requires_initialization_before_a_trade() {
    let f = Fixture::new().await;
    let date = chrono::NaiveDate::parse_from_str(&f.day, "%Y-%m-%d").unwrap() - Duration::days(1);
    f.init
        .context
        .snapshot_service
        .delete_snapshot_for_account("broker", &[date])
        .await
        .unwrap();
    let state = f.state().await;
    assert_eq!(state["hasSnapshot"], false);
    assert_eq!(state["snapshotDate"], Value::Null);
    let buy = json!({"accountId":"broker","activityDate":f.day,"expectedStateVersion":state["stateVersion"],"assetId":"etf","side":"BUY","quantity":"1","price":"1.298","currency":"CNY","requestKey":"unobserved-buy"});
    assert_eq!(
        f.call("record_manual_trade", buy)
            .await
            .unwrap_err()
            .structured_value()["code"],
        "invalid_input"
    );
    assert_eq!(f.count("business_requests"), 0);
    f.stop().await;
}

#[tokio::test]
async fn cross_currency_transfer_records_actual_amounts_without_fabricating_fx() {
    let f = Fixture::new().await;
    let mut args = f.transfer("cross-currency");
    args["destinationCurrency"] = json!("USD");
    args["destinationAmount"] = json!("700");
    let response = f.call("create_internal_cash_transfer", args).await.unwrap();
    assert_eq!(response["result"]["sourceAmount"], "5000");
    assert_eq!(response["result"]["destinationAmount"], "700");
    let state = f.state().await;
    assert_eq!(state["cashBalances"]["USD"], "700");
    assert_eq!(state["cashBalances"]["CNY"], "62002.17");
    f.stop().await;
}

#[tokio::test]
async fn a_transfer_covered_by_an_observed_cash_snapshot_is_flagged_for_reconciliation() {
    let f = Fixture::new().await;
    let state = f.state().await;
    f.call("patch_holdings_snapshot",json!({"accountId":"broker","snapshotDate":f.day,"expectedStateVersion":state["stateVersion"],"cashBalances":{"CNY":"63007.83"},"notes":"observed final cash","requestKey":"observe"})).await.unwrap();
    let transfer = f
        .call(
            "create_internal_cash_transfer",
            f.transfer("covered-transfer"),
        )
        .await
        .unwrap();
    assert_eq!(
        transfer["result"]["cashReconciliationAccounts"],
        json!(["broker"])
    );
    assert_eq!(f.state().await["cashBalances"]["CNY"], "63007.83");
    f.stop().await;
}
