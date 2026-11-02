//! CLI and MCP adapters for the same portfolio bookkeeping use cases.
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
    ToolCallContext,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use wealthfolio_business::investments::{
    CreateCashTransfer, PatchHoldingsSnapshot, RecordManualTrade,
};

#[derive(Clone, Copy)]
enum Kind {
    State,
    Transfer,
    Snapshot,
    Trade,
}
struct InvestmentTool(Kind);
pub fn investment_tools() -> Vec<Arc<dyn AgentTool>> {
    [Kind::State, Kind::Transfer, Kind::Snapshot, Kind::Trade]
        .into_iter()
        .map(|kind| Arc::new(InvestmentTool(kind)) as Arc<dyn AgentTool>)
        .collect()
}
fn string() -> Value {
    json!({"type":"string"})
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateArgs {
    account_id: String,
}
#[async_trait::async_trait]
impl AgentTool for InvestmentTool {
    fn name(&self) -> &'static str {
        match self.0 {
            Kind::State => "get_investment_account_state",
            Kind::Transfer => "create_internal_cash_transfer",
            Kind::Snapshot => "patch_holdings_snapshot",
            Kind::Trade => "record_manual_trade",
        }
    }
    fn description(&self) -> &'static str {
        match self.0{
        Kind::State=>"Read today's per-currency cash, exact manual holdings and stateVersion. Use stateVersion as expectedStateVersion on manual writes. No UI required.",
        Kind::Transfer=>"Record an already-completed internal CASH transfer between two active accounts in the local ledger. Atomically creates both posted legs with replay and audit. No bank payment is executed. Dates must be YYYY-MM-DD, amounts exact strings. Cross-currency transfers require both amounts. Requires transfers:write and explicit ledger binding. Reuse the frozen requestKey after an unknown outcome. cashReconciliationAccounts flags manual accounts where a same-day/later snapshot may already include this cash; read and reconcile the observed balance instead of adding it twice.",
        Kind::Snapshot=>"Patch today's manual securities-account snapshot using expectedStateVersion from get_investment_account_state. Only supplied positions/currencies change; quantity zero removes a position. Notes must explain the observation. Cash changes are reported as adjustments, never invented income or fees. Requires holdings:write and explicit ledger binding. Replay uses the same requestKey.",
        Kind::Trade=>"Record an already-executed BUY/SELL in a HOLDINGS securities account: update quantity, weighted-average book cost and cash atomically as a MANUAL_SNAPSHOT with the trade retained in the request/audit. Does not place a brokerage order or create a transaction-mode BUY/SELL activity. Existing stock/ETF/fund asset, same account settlement currency, today's date only. Omitted fees/tax stay unknown, use zero only for arithmetic. Optional observedCashBalance is authoritative and cashAdjustment exposes the discrepancy. Requires holdings:write, expectedStateVersion and explicit ledger binding. Reuse requestKey on retries.",
    }
    }
    fn input_schema(&self) -> Value {
        let (mut props, required) = match self.0 {
            Kind::State => (json!({"accountId":string()}), vec!["accountId"]),
            Kind::Transfer => (
                json!({"fromAccountId":string(),"toAccountId":string(),"activityDate":string(),"sourceCurrency":string(),"destinationCurrency":string(),"sourceAmount":string(),"destinationAmount":string(),"notes":string(),"idempotencyKey":string(),"requestKey":string()}),
                vec![
                    "fromAccountId",
                    "toAccountId",
                    "activityDate",
                    "sourceCurrency",
                    "destinationCurrency",
                    "sourceAmount",
                    "requestKey",
                ],
            ),
            Kind::Snapshot => (
                json!({"accountId":string(),"snapshotDate":string(),"expectedStateVersion":string(),"positions":{"type":"array","items":{"type":"object","properties":{"assetId":string(),"quantity":string(),"averageCost":string(),"currency":string()},"required":["assetId","quantity","averageCost","currency"],"additionalProperties":false}},"cashBalances":{"type":"object","additionalProperties":string()},"notes":string(),"requestKey":string()}),
                vec![
                    "accountId",
                    "snapshotDate",
                    "expectedStateVersion",
                    "notes",
                    "requestKey",
                ],
            ),
            Kind::Trade => (
                json!({"accountId":string(),"activityDate":string(),"expectedStateVersion":string(),"assetId":string(),"side":{"type":"string","enum":["BUY","SELL"]},"quantity":string(),"price":string(),"currency":string(),"fee":string(),"tax":string(),"observedCashBalance":string(),"notes":string(),"requestKey":string()}),
                vec![
                    "accountId",
                    "activityDate",
                    "expectedStateVersion",
                    "assetId",
                    "side",
                    "quantity",
                    "price",
                    "currency",
                    "requestKey",
                ],
            ),
        };
        if let Some(map) = props.as_object_mut() {
            for key in ["activityDate", "snapshotDate"] {
                if let Some(value) = map.get_mut(key) {
                    value["description"] =
                        json!("Explicit calendar date YYYY-MM-DD. Do not invent a time.");
                }
            }
        }
        json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
    }
    fn required_scopes(&self) -> &'static [AgentScope] {
        match self.0 {
            Kind::State => &[
                AgentScope::AccountsRead,
                AgentScope::HoldingsRead,
                AgentScope::ActivitiesRead,
            ],
            Kind::Transfer => &[
                AgentScope::TransfersWrite,
                AgentScope::AccountsRead,
                AgentScope::ActivitiesRead,
            ],
            _ => &[
                AgentScope::HoldingsWrite,
                AgentScope::AccountsRead,
                AgentScope::HoldingsRead,
                AgentScope::ActivitiesRead,
            ],
        }
    }
    fn access_level(&self) -> AgentToolAccess {
        match self.0 {
            Kind::State => AgentToolAccess::Read,
            _ => AgentToolAccess::Write,
        }
    }
    fn sanitize_args_for_audit(&self, args: &Value) -> Value {
        json!({"fields":args.as_object().map(|v|v.keys().collect::<Vec<_>>()).unwrap_or_default()})
    }
    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        if let Kind::State = self.0 {
            let args: StateArgs = serde_json::from_value(args)?;
            let service = env.investment_service().ok_or_else(|| {
                AgentToolError::InvalidInput(
                    "portfolio bookkeeping unavailable on this host".into(),
                )
            })?;
            return Ok(AgentToolResult {
                content: service.get_state(&args.account_id).await?,
            });
        }
        Err(AgentToolError::InvalidInput(
            "portfolio writes require trusted call context".into(),
        ))
    }
    async fn call_with_context(
        &self,
        env: Arc<dyn AgentEnvironment>,
        context: &ToolCallContext,
        args: Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        if let Kind::State = self.0 {
            return self.call(env, args).await;
        }
        let service = env.investment_service().ok_or_else(|| {
            AgentToolError::InvalidInput("portfolio bookkeeping unavailable on this host".into())
        })?;
        let actor = context.credential_id.as_deref().unwrap_or(&context.actor);
        let result = match self.0 {
            Kind::Transfer => {
                service
                    .create_transfer(
                        serde_json::from_value::<CreateCashTransfer>(args)?,
                        context.channel,
                        actor,
                        context.authenticated,
                    )
                    .await?
            }
            Kind::Snapshot => {
                service
                    .patch_snapshot(
                        serde_json::from_value::<PatchHoldingsSnapshot>(args)?,
                        context.channel,
                        actor,
                        context.authenticated,
                    )
                    .await?
            }
            Kind::Trade => {
                service
                    .record_manual_trade(
                        serde_json::from_value::<RecordManualTrade>(args)?,
                        context.channel,
                        actor,
                        context.authenticated,
                    )
                    .await?
            }
            Kind::State => unreachable!(),
        };
        Ok(AgentToolResult {
            content: json!({"result":result}),
        })
    }
}
