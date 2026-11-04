//! Desktop-only adapters for the typed GUI/CLI/MCP investment command boundary.
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
    ToolCallContext,
};
use serde_json::{json, Value};
use std::sync::Arc;
use wealthfolio_business::investment_journal::{
    InvestmentJournalCommand, JournalCommandAccess, JournalError,
};

struct JournalTool(InvestmentJournalCommand);

pub fn journal_tools() -> Vec<Arc<dyn AgentTool>> {
    InvestmentJournalCommand::ALL
        .iter()
        .copied()
        .filter(|command| command.external_available())
        .map(|command| Arc::new(JournalTool(command)) as Arc<dyn AgentTool>)
        .collect()
}
pub fn journal_command(name: &str) -> Option<InvestmentJournalCommand> {
    InvestmentJournalCommand::ALL
        .iter()
        .copied()
        .find(|command| command.name() == name)
}
pub fn journal_formal_write(name: &str) -> bool {
    journal_command(name).is_some_and(|command| {
        command.external_available() && command.access() != JournalCommandAccess::Read
    })
}

#[async_trait::async_trait]
impl AgentTool for JournalTool {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn description(&self) -> &'static str {
        self.0.description()
    }
    fn input_schema(&self) -> Value {
        self.0.input_schema()
    }
    fn access_level(&self) -> AgentToolAccess {
        if self.0.access() == JournalCommandAccess::Read {
            AgentToolAccess::Read
        } else {
            AgentToolAccess::Write
        }
    }
    fn required_scopes(&self) -> &'static [AgentScope] {
        use AgentScope::*;
        match self.0.access() {
            JournalCommandAccess::Read => {
                &[InvestmentsRead, AccountsRead, HoldingsRead, ActivitiesRead]
            }
            JournalCommandAccess::Write | JournalCommandAccess::GuiPreferenceWrite => &[
                InvestmentsRead,
                InvestmentsWrite,
                AccountsRead,
                HoldingsRead,
                ActivitiesRead,
            ],
            JournalCommandAccess::Confirm => &[
                InvestmentsRead,
                InvestmentsWrite,
                InvestmentsConfirm,
                AccountsRead,
                HoldingsRead,
                ActivitiesRead,
            ],
            JournalCommandAccess::ManualTradeWrite => &[
                InvestmentsRead,
                InvestmentsWrite,
                HoldingsWrite,
                AccountsRead,
                HoldingsRead,
                ActivitiesRead,
            ],
            JournalCommandAccess::TransferWrite => &[
                InvestmentsRead,
                InvestmentsWrite,
                TransfersWrite,
                AccountsRead,
                HoldingsRead,
                ActivitiesRead,
            ],
            JournalCommandAccess::GuiTradeWrite => &[
                InvestmentsRead,
                InvestmentsWrite,
                ActivitiesWrite,
                AccountsRead,
                HoldingsRead,
                ActivitiesRead,
            ],
        }
    }
    fn sanitize_args_for_audit(&self, args: &Value) -> Value {
        // The native business transaction retains exact authorized contents.
        // Transport audits do not copy financial text, evidence or credentials.
        json!({"fields":args.as_object().map(|map|map.keys().collect::<Vec<_>>()).unwrap_or_default()})
    }
    async fn call(
        &self,
        _env: Arc<dyn AgentEnvironment>,
        _args: Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        Err(JournalError::PermissionDenied.into())
    }
    async fn call_with_context(
        &self,
        env: Arc<dyn AgentEnvironment>,
        context: &ToolCallContext,
        args: Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        let caller = context.journal_caller();
        caller.require_read()?;
        let service = env.investment_journal_service().ok_or_else(|| {
            AgentToolError::InvalidInput("investment journal unavailable on this host".into())
        })?;
        Ok(AgentToolResult {
            content: self.0.execute(&service, args, &caller).await?,
        })
    }
}
