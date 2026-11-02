//! Tool registry with scope-gated execution.

use std::sync::Arc;

use crate::context::ToolCallContext;
use crate::env::AgentEnvironment;
use crate::scope::AgentScopeSet;
use crate::tool::{AgentTool, AgentToolAccess, AgentToolError, AgentToolResult};

/// Tools hidden from MCP by name even though their access level would allow
/// exposure.
///
/// `prepare_activity_updates` previews a correction whose apply step is a
/// disabled formal write; the persistent `create_activity` and
/// `create_pending_activity` tools replaced the old activity previews, which
/// were removed (plan task two).
pub const MCP_HIDDEN_TOOLS: &[&str] = &["prepare_activity_updates"];

/// The only formal writes a desktop-hosted MCP/CLI surface may execute.
pub const DESKTOP_FORMAL_WRITE_TOOLS: &[&str] = &[
    "create_ordinary_activity",
    "update_ordinary_activity",
    "create_internal_cash_transfer",
    "patch_holdings_snapshot",
    "record_manual_trade",
];

/// Which host is serving the catalog. The host policy is code, not a token
/// scope: a token cannot widen it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpHost {
    /// Tauri desktop app (HTTP `/mcp` and the local CLI socket).
    Desktop,
    /// Axum server. Formal writes stay unavailable in this release.
    Server,
}

impl McpHost {
    pub fn permits_formal_write(self, tool: &str) -> bool {
        matches!(self, McpHost::Desktop) && DESKTOP_FORMAL_WRITE_TOOLS.contains(&tool)
    }
}

/// Whether `tool` is blocked from MCP exposure and execution.
///
/// Every `Write`-access tool and the by-name hidden previews stay blocked. The
/// persistent surface for agents is the pending-entry tools, which are
/// `Draft`/`Read`; formal activity writes remain GUI/REST-only this round.
/// Applied when the default and custom MCP catalogs are built and enforced
/// again at the MCP execution entry (`wealthfolio-mcp` handler).
pub fn mcp_write_blocked(tool: &dyn AgentTool) -> bool {
    mcp_write_blocked_for(tool, McpHost::Server)
}

/// Host-aware policy: desktop exposes explicitly listed bookkeeping writes.
/// Other writes and unsupported server-only reads remain unavailable.
pub fn mcp_write_blocked_for(tool: &dyn AgentTool, host: McpHost) -> bool {
    let blocked_write = matches!(tool.access_level(), AgentToolAccess::Write)
        && !host.permits_formal_write(tool.name());
    blocked_write
        || MCP_HIDDEN_TOOLS.contains(&tool.name())
        || (host == McpHost::Server && tool.name() == "get_investment_account_state")
}

/// An ordered collection of agent tools.
///
/// `execute` enforces scopes before the tool runs. This is the authorization
/// boundary for external MCP callers.
pub struct AgentToolCatalog {
    tools: Vec<Arc<dyn AgentTool>>,
}

impl AgentToolCatalog {
    pub fn new(tools: Vec<Arc<dyn AgentTool>>) -> Self {
        Self { tools }
    }

    /// The v1 read-only catalog.
    pub fn v1_read_tools() -> Self {
        Self::new(crate::tools::v1_read_tools())
    }

    /// The MCP catalog: reads, drafts/suggestions and transfer matching.
    ///
    /// Formal-write (`Write` access) tools and [`MCP_HIDDEN_TOOLS`] are
    /// filtered out — MCP does not expose formal writes this release. Scope
    /// filtering additionally hides tools that a token cannot access.
    pub fn mcp_catalog() -> Self {
        Self::mcp_catalog_for(McpHost::Server)
    }

    /// The MCP catalog for a specific host policy.
    pub fn mcp_catalog_for(host: McpHost) -> Self {
        let mut tools = crate::tools::v1_read_tools();
        tools.extend(crate::tools::draft_suggest_tools());
        tools.extend(crate::tools::commit_tools());
        tools.extend(crate::tools::transfer_link_tools());
        tools.extend(crate::tools::activity_update_tools());
        tools.extend(crate::tools::ordinary_tools());
        tools.extend(crate::tools::investments::investment_tools());
        tools.retain(|tool| !mcp_write_blocked_for(tool.as_ref(), host));
        Self::new(tools)
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn AgentTool>> {
        self.tools.iter().find(|tool| tool.name() == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn AgentTool>> {
        self.tools.iter()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Execute `name` with `args`, enforcing `granted` scopes first.
    /// Denials and unknown tools return errors without touching services.
    ///
    /// Convenience for callers without a transport context: the tool sees an
    /// anonymous context carrying the same scopes.
    pub async fn execute(
        &self,
        env: Arc<dyn AgentEnvironment>,
        granted: &AgentScopeSet,
        name: &str,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        let context = ToolCallContext::anonymous(granted.clone());
        self.execute_with_context(env, &context, name, args).await
    }

    /// Execute `name` with the explicit trusted call context.
    pub async fn execute_with_context(
        &self,
        env: Arc<dyn AgentEnvironment>,
        context: &ToolCallContext,
        name: &str,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        let granted = &context.scopes;
        let tool = self
            .get(name)
            .ok_or_else(|| AgentToolError::NotFound(name.to_string()))?;
        let required = tool.required_scopes();
        if !granted.grants_all(required) {
            let missing = required
                .iter()
                .filter(|scope| !granted.contains(**scope))
                .map(|scope| scope.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AgentToolError::ScopeDenied {
                tool: name.to_string(),
                missing,
            });
        }
        tool.call_with_context(env, context, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use wealthfolio_spending::categorization_rules::{CategorizationRule, NewCategorizationRule};

    /// Stub environment for authorization tests: service access panics unless
    /// a test explicitly supplies the rule service.
    #[derive(Default)]
    struct PanicEnv {
        rules: Option<Arc<TestRuleService>>,
    }

    #[derive(Default)]
    struct TestRuleService {
        created: Mutex<Vec<NewCategorizationRule>>,
    }

    #[async_trait::async_trait]
    impl wealthfolio_spending::categorization_rules::CategorizationRulesServiceTrait
        for TestRuleService
    {
        async fn list(&self) -> anyhow::Result<Vec<CategorizationRule>> {
            unreachable!("not used by the commit test")
        }

        async fn create(&self, rule: NewCategorizationRule) -> anyhow::Result<CategorizationRule> {
            let now = chrono::Utc::now().naive_utc();
            let created = CategorizationRule {
                id: rule.id.clone().unwrap(),
                name: rule.name.clone(),
                pattern: rule.pattern.clone(),
                match_type: rule.match_type,
                taxonomy_id: rule.taxonomy_id.clone(),
                category_id: rule.category_id.clone(),
                activity_type: rule.activity_type.clone(),
                amount_op: rule.amount_op,
                amount_value: rule.amount_value,
                amount_value2: rule.amount_value2,
                priority: rule.priority,
                is_global: rule.is_global,
                account_id: rule.account_id.clone(),
                preset_id: None,
                preset_rule_key: None,
                preset_version: None,
                preset_modified: false,
                created_at: now,
                updated_at: now,
            };
            self.created.lock().unwrap().push(rule);
            Ok(created)
        }
    }

    impl AgentEnvironment for PanicEnv {
        fn base_currency(&self) -> String {
            "USD".to_string()
        }
        fn account_service(&self) -> Arc<dyn wealthfolio_core::accounts::AccountServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn activity_service(&self) -> Arc<dyn wealthfolio_core::activities::ActivityServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn holdings_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::holdings::HoldingsServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn valuation_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::valuation::ValuationServiceTrait> {
            unimplemented!("PanicEnv")
        }

        fn settings_service(&self) -> Arc<dyn wealthfolio_core::settings::SettingsServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn quote_service(&self) -> Arc<dyn wealthfolio_core::quotes::QuoteServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn asset_service(&self) -> Arc<dyn wealthfolio_core::assets::AssetServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn allocation_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::allocation::AllocationServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn performance_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::performance::PerformanceServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn income_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::income::IncomeServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn health_service(&self) -> Arc<dyn wealthfolio_core::health::HealthServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn taxonomy_service(&self) -> Arc<dyn wealthfolio_core::taxonomies::TaxonomyServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn portfolio_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolios::PortfolioServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn net_worth_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::net_worth::NetWorthServiceTrait> {
            unimplemented!("PanicEnv")
        }

        fn cash_activity_service(
            &self,
        ) -> Arc<dyn wealthfolio_spending::cash_activities::CashActivityServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn categorization_rules_service(
            &self,
        ) -> Arc<dyn wealthfolio_spending::categorization_rules::CategorizationRulesServiceTrait>
        {
            self.rules.clone().expect("PanicEnv")
        }
    }

    #[tokio::test]
    async fn execute_denies_every_v1_tool_with_empty_scopes() {
        let catalog = AgentToolCatalog::v1_read_tools();
        let granted = AgentScopeSet::new();
        for name in catalog.iter().map(|tool| tool.name()).collect::<Vec<_>>() {
            let err = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &granted,
                    name,
                    serde_json::json!({}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, AgentToolError::ScopeDenied { .. }),
                "tool {name} should be scope-denied, got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn mcp_catalog_exposes_only_read_draft_and_match_tools() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let names: Vec<&str> = catalog.iter().map(|tool| tool.name()).collect();
        assert_eq!(
            names,
            vec![
                "get_holdings",
                "get_accounts",
                "get_cash_balances",
                "search_activities",
                "get_valuation_history",
                "get_income",
                "get_asset_allocation",
                "get_performance",
                "get_health_status",
                "list_categorization_context",
                "list_asset_taxonomies",
                "get_asset_taxonomy_assignments",
                "get_portfolios",
                "get_net_worth",
                "create_pending_activity",
                "list_pending_activities",
                "propose_transaction_categories",
                "create_categorization_rule",
                "prepare_asset_classification",
                "find_transfer_matches",
                "query_ordinary_activities",
                "get_ordinary_activity",
                "get_bookkeeping_context",
            ]
        );
        for tool in catalog.iter() {
            assert!(
                !mcp_write_blocked(tool.as_ref()),
                "blocked tool {} exposed",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn bookkeeping_context_checks_all_read_scopes_before_services() {
        let catalog = AgentToolCatalog::mcp_catalog();
        for scopes in [
            vec!["accounts:read", "activities:read"],
            vec!["accounts:read", "classification:read"],
            vec!["activities:read", "classification:read"],
        ] {
            let error = catalog.execute(
                Arc::new(PanicEnv::default()), &AgentScopeSet::from_strs(scopes),
                "get_bookkeeping_context", serde_json::json!({}),
            ).await.unwrap_err();
            assert!(matches!(error, AgentToolError::ScopeDenied { .. }));
        }
    }

    #[test]
    fn desktop_catalog_exposes_explicit_portfolio_writes_and_keeps_server_blocked() {
        let desktop = AgentToolCatalog::mcp_catalog_for(McpHost::Desktop);
        let names: Vec<&str> = desktop.iter().map(|tool| tool.name()).collect();
        assert!(names.contains(&"create_ordinary_activity"));
        assert!(names.contains(&"update_ordinary_activity"));
        for name in [
            "get_investment_account_state",
            "create_internal_cash_transfer",
            "record_manual_trade",
            "patch_holdings_snapshot",
        ] {
            assert!(names.contains(&name));
        }
        assert!(!names.contains(&"commit_activity_updates"));
        assert!(!names.contains(&"link_transfer_activities"));
        assert!(!names.contains(&"prepare_activity_updates"));

        let server = AgentToolCatalog::mcp_catalog_for(McpHost::Server);
        let server_names: Vec<&str> = server.iter().map(|tool| tool.name()).collect();
        assert!(!server_names.contains(&"create_ordinary_activity"));
        assert!(!server_names.contains(&"update_ordinary_activity"));
        assert!(server_names.contains(&"query_ordinary_activities"));
        for name in [
            "get_investment_account_state",
            "create_internal_cash_transfer",
            "record_manual_trade",
            "patch_holdings_snapshot",
        ] {
            assert!(!server_names.contains(&name));
        }
    }

    #[test]
    fn tool_errors_carry_the_structured_contract() {
        let duplicate = AgentToolError::DuplicateActivity {
            message: "duplicate activity".to_string(),
            existing_activity_id: Some("act-1".to_string()),
        };
        let value = duplicate.structured_value();
        assert_eq!(value["code"], "duplicate_activity");
        assert_eq!(value["retryable"], false);
        assert_eq!(value["requestId"], serde_json::Value::Null);
        assert_eq!(value["data"]["existingActivityId"], "act-1");
        assert!(duplicate.to_string().contains("duplicate activity"));
    }

    #[tokio::test]
    async fn mcp_catalog_execution_rejects_formal_writes_and_hidden_tools() {
        let catalog = AgentToolCatalog::mcp_catalog();
        // Full scopes: the rejection must come from the catalog filter, not
        // from a missing scope. PanicEnv proves no service is reached.
        let granted = AgentScopeSet::from_strs([
            "activities:read",
            "activities:draft",
            "activities:write",
            "classification:read",
            "classification:suggest",
            "classification:write",
        ]);
        for name in [
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
            let err = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &granted,
                    name,
                    serde_json::json!({}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, AgentToolError::NotFound(_)),
                "tool {name} should be filtered out, got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn category_commit_requires_both_classification_scopes() {
        // The tool is filtered from MCP this release; this keeps its scope
        // contract covered directly through a generic catalog.
        let catalog =
            AgentToolCatalog::new(vec![Arc::new(crate::tools::CommitTransactionCategories)]);
        for scopes in [
            vec!["classification:read"],
            vec!["classification:suggest"],
            vec!["classification:write"],
        ] {
            let error = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &AgentScopeSet::from_strs(scopes),
                    "commit_transaction_categories",
                    serde_json::json!({"assignments": []}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, AgentToolError::ScopeDenied { .. }),
                "{error}"
            );
        }
    }

    /// Generated duplicate pairs must be rejected before any service access,
    /// regardless of category, batch position, Unicode or delimiter-containing IDs.
    #[tokio::test]
    async fn category_commit_rejects_generated_duplicate_pairs_before_service_access() {
        let catalog =
            AgentToolCatalog::new(vec![Arc::new(crate::tools::CommitTransactionCategories)]);
        let scopes = AgentScopeSet::from_strs(["classification:suggest", "classification:write"]);
        let mut seed = 0x6a09_e667_f3bc_c909_u64;
        for _ in 0..256 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let count = 2 + (seed as usize % 99);
            let mut assignments: Vec<_> = (0..count)
                .map(|index| {
                    let ch = char::from_u32((seed.wrapping_add(index as u64) % 0x110000) as u32)
                        .unwrap_or('\0');
                    serde_json::json!({
                        "activityId": format!("{index}:{ch}:{seed}"),
                        "taxonomyId": format!("{ch}:taxonomy:{seed}"),
                        "categoryId": format!("category:{index}")
                    })
                })
                .collect();
            let original = seed as usize % count;
            let duplicate = (original + 1 + (seed >> 32) as usize % (count - 1)) % count;
            assignments[duplicate]["activityId"] = assignments[original]["activityId"].clone();
            assignments[duplicate]["taxonomyId"] = assignments[original]["taxonomyId"].clone();
            let error = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &scopes,
                    "commit_transaction_categories",
                    serde_json::json!({"assignments": assignments}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, AgentToolError::InvalidInput(_)),
                "seed {seed}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn categorization_rule_commit_requires_write_scope_and_saves_reviewed_rule() {
        let catalog = AgentToolCatalog::new(vec![Arc::new(crate::tools::CommitCategorizationRule)]);
        let rules = Arc::new(TestRuleService::default());
        let env = Arc::new(PanicEnv {
            rules: Some(rules.clone()),
        });
        let input = serde_json::json!({
            "rule": {
                "id": "draft-1", "name": "Coffee", "pattern": "CAFE",
                "matchType": "contains", "taxonomyId": "spending_categories",
                "categoryId": "food", "activityType": null, "amountOp": null,
                "amountValue": null, "amountValue2": null, "priority": 0,
                "isGlobal": true, "accountId": null, "presetId": null,
                "presetRuleKey": null, "presetVersion": null
            }
        });

        let suggest_only = AgentScopeSet::from_strs(["classification:suggest"]);
        let denied = catalog
            .execute(
                env.clone(),
                &suggest_only,
                "commit_categorization_rule",
                input.clone(),
            )
            .await
            .unwrap_err();
        assert!(matches!(denied, AgentToolError::ScopeDenied { .. }));
        assert!(rules.created.lock().unwrap().is_empty());

        let writable = AgentScopeSet::from_strs(["classification:suggest", "classification:write"]);
        let result = catalog
            .execute(env, &writable, "commit_categorization_rule", input)
            .await
            .unwrap();
        assert_eq!(result.content["created"]["id"], "draft-1");
        let saved = rules.created.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].pattern, "CAFE");
    }

    #[tokio::test]
    async fn activity_update_tools_need_their_scopes() {
        // Both update tools are filtered from MCP this release; their scope
        // contract stays covered through a generic catalog.
        let catalog = AgentToolCatalog::new(vec![
            Arc::new(crate::tools::PrepareActivityUpdates),
            Arc::new(crate::tools::CommitActivityUpdates),
        ]);
        // Rejected while parsing, so no service is reached (PanicEnv).
        let args = serde_json::json!({ "updates": [{ "activityId": "a", "symbol": "BTC" }] });
        let cases = [
            ("prepare_activity_updates", vec!["activities:read"], false),
            ("prepare_activity_updates", vec!["activities:draft"], false),
            (
                "prepare_activity_updates",
                vec!["activities:read", "activities:draft"],
                true,
            ),
            (
                "commit_activity_updates",
                vec!["activities:read", "activities:draft"],
                false,
            ),
            (
                "commit_activity_updates",
                vec!["activities:draft", "activities:write"],
                false,
            ),
            (
                "commit_activity_updates",
                vec!["activities:read", "activities:draft", "activities:write"],
                true,
            ),
        ];
        for (name, scopes, allowed) in cases {
            let result = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &AgentScopeSet::from_strs(scopes.clone()),
                    name,
                    args.clone(),
                )
                .await;
            assert_eq!(
                !matches!(result, Err(AgentToolError::ScopeDenied { .. })),
                allowed,
                "{name} with {scopes:?}"
            );
        }
    }

    #[tokio::test]
    async fn activity_updates_refuse_symbols_and_bad_batches_before_touching_env() {
        let catalog = AgentToolCatalog::new(vec![
            Arc::new(crate::tools::PrepareActivityUpdates),
            Arc::new(crate::tools::CommitActivityUpdates),
        ]);
        let granted =
            AgentScopeSet::from_strs(["activities:read", "activities:draft", "activities:write"]);
        let rows = serde_json::json!({ "updates": [
            { "activityId": "a", "symbol": "BTC" },
            { "activityId": "b", "assetId": "x", "ticker": "BTC" },
            { "activityId": "c", "unitprice": 1 }
        ]});
        let preview = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &granted,
                "prepare_activity_updates",
                rows.clone(),
            )
            .await
            .unwrap();
        assert_eq!(
            preview.content["summary"]["invalid"], 3,
            "{}",
            preview.content
        );
        let committed = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &granted,
                "commit_activity_updates",
                rows,
            )
            .await
            .unwrap();
        assert_eq!(committed.content["updated"], serde_json::json!([]));
        assert_eq!(committed.content["errors"].as_array().unwrap().len(), 3);

        let oversized: Vec<_> = (0..101)
            .map(|index| serde_json::json!({ "activityId": format!("a{index}"), "fee": 1 }))
            .collect();
        let err = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &granted,
                "commit_activity_updates",
                serde_json::json!({ "updates": oversized }),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentToolError::InvalidInput(_)), "{err}");
    }

    #[tokio::test]
    async fn execute_returns_not_found_for_unknown_tool() {
        let catalog = AgentToolCatalog::v1_read_tools();
        let err = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &AgentScopeSet::read_only(),
                "no_such_tool",
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentToolError::NotFound(_)));
    }
}
