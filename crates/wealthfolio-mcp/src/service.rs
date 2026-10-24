//! Builder producing the tower service hosts mount.

use std::sync::Arc;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use wealthfolio_agent_tools::{AgentEnvironment, AgentTool, AgentToolCatalog, McpHost};
use wealthfolio_business::audit::BusinessChannel;

use crate::audit::AuditSink;
use crate::handler::WealthfolioMcpHandler;

/// Builds the Wealthfolio MCP Streamable HTTP service.
///
/// Defaults: the MCP catalog (read + draft/suggest + transfer matching; all
/// formal-write tools are filtered out), scope-filtered per token at the
/// boundary, with no audit sink and no instructions. Custom tool sets passed
/// to [`McpServerBuilder::tools`] are filtered the same way.
/// Hosts must serve the result behind authentication middleware that
/// injects [`crate::auth::McpAuthContext`] — the handler fails closed
/// otherwise.
pub struct McpServerBuilder {
    env: Option<Arc<dyn AgentEnvironment>>,
    tools: Option<Vec<Arc<dyn AgentTool>>>,
    audit: Option<Arc<dyn AuditSink>>,
    instructions: Option<String>,
    host: McpHost,
    channel: BusinessChannel,
    ledger_id: Option<String>,
    dynamic: bool,
}

impl McpServerBuilder {
    pub fn new(env: Arc<dyn AgentEnvironment>) -> Self {
        Self {
            env: Some(env),
            tools: None,
            audit: None,
            instructions: None,
            host: McpHost::Server,
            channel: BusinessChannel::Mcp,
            ledger_id: None,
            dynamic: false,
        }
    }

    /// Start a builder without a fixed environment. Hosts must attach an
    /// [`crate::runtime::McpCallRuntime`] to every request.
    pub fn new_dynamic() -> Self {
        Self {
            env: None,
            tools: None,
            audit: None,
            instructions: None,
            host: McpHost::Server,
            channel: BusinessChannel::Mcp,
            ledger_id: None,
            dynamic: true,
        }
    }

    /// Build a handler that resolves services per request from the host's
    /// runtime lease. The listener then holds no ledger service or pool.
    pub fn dynamic(mut self) -> Self {
        self.dynamic = true;
        self
    }

    /// Choose the host policy. The default is the web server, where formal
    /// writes stay unavailable.
    pub fn host(mut self, host: McpHost) -> Self {
        self.host = host;
        self
    }

    /// Which surface accepted the call (`mcp` over HTTP, `cli` over UDS).
    pub fn channel(mut self, channel: BusinessChannel) -> Self {
        self.channel = channel;
        self
    }

    /// Pin this listener to one ledger so `X-WF-Profile-Id` can be checked.
    pub fn ledger(mut self, ledger_id: impl Into<String>) -> Self {
        self.ledger_id = Some(ledger_id.into());
        self
    }

    /// Override the tool set (default: the full MCP catalog).
    pub fn tools(mut self, tools: Vec<Arc<dyn AgentTool>>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn audit(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit = Some(sink);
        self
    }

    /// Server instructions surfaced to MCP clients at initialize.
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn build_handler(self) -> WealthfolioMcpHandler {
        let host = self.host;
        let catalog = match self.tools {
            // Custom tool sets get the same formal-write filter as the
            // default MCP catalog.
            Some(tools) => AgentToolCatalog::new(
                tools
                    .into_iter()
                    .filter(|tool| {
                        !wealthfolio_agent_tools::mcp_write_blocked_for(tool.as_ref(), host)
                    })
                    .collect(),
            ),
            None => AgentToolCatalog::mcp_catalog_for(host),
        };
        if self.dynamic {
            WealthfolioMcpHandler::new_dynamic(
                Arc::new(catalog),
                self.audit,
                self.instructions,
                host,
                self.channel,
                self.ledger_id,
            )
        } else {
            WealthfolioMcpHandler::new(
                self.env.expect("static builder has an environment"),
                Arc::new(catalog),
                self.audit,
                self.instructions,
                host,
                self.channel,
                self.ledger_id,
            )
        }
    }

    /// Build the tower service to mount (e.g. `Router::nest_service("/mcp", svc)`).
    pub fn build_http_service(
        self,
        config: StreamableHttpServerConfig,
    ) -> StreamableHttpService<WealthfolioMcpHandler, LocalSessionManager> {
        let handler = self.build_handler();
        StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        )
    }
}
