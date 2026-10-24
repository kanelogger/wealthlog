//! Tool implementations.

pub mod accounts;
pub mod activities;

pub mod activity_updates;
pub mod allocation;
pub mod asset_classification;
pub mod asset_taxonomies;
pub mod cash_balances;
pub mod categorization_context;
pub mod commit_asset_classification;
pub mod commit_categorization_rule;
pub mod commit_transaction_categories;
pub mod create_categorization_rule;
pub mod health;
pub mod holdings;
pub mod income;
pub mod net_worth;
pub mod ordinary;
pub mod pending_activities;
pub mod performance;
pub mod portfolios;
pub mod propose_categories;

pub mod transfer_links;
pub mod valuation;

pub use accounts::{AccountDto, GetAccounts, GetAccountsArgs, GetAccountsOutput};
pub use activities::{ActivityDto, SearchActivities, SearchActivitiesArgs, SearchActivitiesOutput};
// `allocation::HoldingDto` is intentionally not re-exported here: it would
// collide with `holdings::HoldingDto`. Reach it as `allocation::HoldingDto`.
pub use allocation::{
    AllocationDto, GetAssetAllocation, GetAssetAllocationArgs, GetAssetAllocationOutput,
};
pub use asset_taxonomies::{
    AssetTaxonomyAssignmentDto, AssetTaxonomyCategoryDto, AssetTaxonomyDto,
    GetAssetTaxonomyAssignments, GetAssetTaxonomyAssignmentsArgs,
    GetAssetTaxonomyAssignmentsOutput, ListAssetTaxonomies, ListAssetTaxonomiesArgs,
    ListAssetTaxonomiesOutput, ResolvedAssetDto,
};
pub use cash_balances::{
    AccountCashSummary, CashBalanceEntry, GetCashBalances, GetCashBalancesArgs,
    GetCashBalancesOutput,
};
pub use categorization_context::{
    CategoryExample, CategoryOption, ContextSummary, ListCategorizationContext,
    ListCategorizationContextArgs, ListCategorizationContextOutput, Proposal, TaxonomySummary,
    UnproposedActivity,
};
pub use health::{GetHealthStatus, GetHealthStatusArgs, GetHealthStatusOutput, HealthIssueDto};
pub use holdings::{GetHoldings, GetHoldingsArgs, GetHoldingsOutput, HoldingDto};
pub use income::{GetIncome, GetIncomeArgs, GetIncomeOutput, TopAssetDto};
pub use net_worth::{
    GetNetWorth, GetNetWorthArgs, GetNetWorthOutput, NetWorthHistoryPointDto, NetWorthLineDto,
};
pub use performance::{
    GetPerformance, GetPerformanceArgs, GetPerformanceOutput, PerformanceAttributionOutput,
    PerformanceDataQualityOutput, PerformanceReturnsOutput, PerformanceRiskOutput,
};
pub use portfolios::{GetPortfolios, GetPortfoliosArgs, GetPortfoliosOutput, PortfolioDto};
pub use valuation::{
    GetValuationHistory, GetValuationHistoryArgs, GetValuationHistoryOutput, ValuationPointDto,
};

// Draft/suggest tools for external MCP clients.
pub use asset_classification::{
    AssignmentPreviewDto, CandidateAssignmentPreviewDto, ClassificationChangesDto,
    PrepareAssetClassification, PrepareAssetClassificationArgs, PrepareAssetClassificationOutput,
    PreparedAssignmentInput, PreparedTaxonomyDto,
};
pub use create_categorization_rule::{
    CreateCategorizationRule, CreateCategorizationRuleArgs, CreateCategorizationRuleOutput,
};
pub use pending_activities::{
    CreatePendingActivity, CreatePendingArgs, CreatePendingOutput, ListPendingActivities,
    ListPendingArgs, ListPendingOutput,
};
pub use propose_categories::{
    AiProposal, ProposalSummary, ProposeCategories, ProposeCategoriesArgs, ProposeCategoriesOutput,
};

// MCP-only commit tools.
pub use commit_asset_classification::{
    CommitAssetClassificationAssignmentInput, CommitAssetClassificationDraft,
    CommitAssetClassificationDraftArgs, CommitAssetClassificationDraftOutput,
    CommittedAssetClassificationAssignment,
};
pub use commit_categorization_rule::{
    CommitCategorizationRule, CommitCategorizationRuleArgs, CommitCategorizationRuleOutput,
};
pub use commit_transaction_categories::{
    CommitTransactionCategories, CommitTransactionCategoriesArgs, CommitTransactionCategoriesOutput,
};

// MCP-only transfer linking tools.
pub use transfer_links::{
    FindTransferMatches, FindTransferMatchesArgs, FindTransferMatchesOutput,
    LinkTransferActivities, LinkTransfersOutput, TransferCandidateDto, TransferDto,
    TransferPairDto, TransferPairError, UnlinkTransferActivities, UnlinkTransfersOutput,
    UnlinkedTransferDto,
};

// Ordinary income/expense reads and (desktop-only) writes.
pub use ordinary::{
    CreateOrdinaryActivityTool, GetOrdinaryActivity, QueryOrdinaryActivities,
    UpdateOrdinaryActivityTool,
};

/// Ordinary income/expense tools in catalog order. The host policy decides
/// whether the two writes are exposed.
pub fn ordinary_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(QueryOrdinaryActivities),
        Arc::new(GetOrdinaryActivity),
        Arc::new(CreateOrdinaryActivityTool),
        Arc::new(UpdateOrdinaryActivityTool),
    ]
}

// MCP-only activity update tools (preview, then apply in place).
pub use activity_updates::{
    ActivityUpdateError, ActivityUpdatePreviewRow, ActivityUpdatePreviewSummary,
    CommitActivityUpdates, CommitActivityUpdatesOutput, FieldChange, PrepareActivityUpdates,
    PrepareActivityUpdatesOutput, StoredChange, UpdatedActivity,
};

use std::sync::Arc;

use crate::tool::AgentTool;

/// The read-only tool set in stable catalog order.
pub fn v1_read_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(GetHoldings),
        Arc::new(GetAccounts),
        Arc::new(GetCashBalances),
        Arc::new(SearchActivities),
        Arc::new(GetValuationHistory),
        Arc::new(GetIncome),
        Arc::new(GetAssetAllocation),
        Arc::new(GetPerformance),
        Arc::new(GetHealthStatus),
        Arc::new(ListCategorizationContext),
        Arc::new(ListAssetTaxonomies),
        Arc::new(GetAssetTaxonomyAssignments),
        Arc::new(GetPortfolios),
        Arc::new(GetNetWorth),
    ]
}

/// The draft/suggest tools, in catalog (and LLM-visible) order. These prepare
/// drafts or proposals for user review — they never mutate data themselves.
pub fn draft_suggest_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreatePendingActivity),
        Arc::new(ListPendingActivities),
        Arc::new(ProposeCategories),
        Arc::new(CreateCategorizationRule),
        Arc::new(PrepareAssetClassification),
    ]
}

/// The MCP-only commit tools that persist reviewed drafts. Never exposed to the
/// in-app assistant (which writes through its confirmation widget instead).
/// Disabled for MCP this release: [`crate::catalog::mcp_write_blocked`] filters
/// every `Write` tool out of the default and custom catalogs.
pub fn commit_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CommitAssetClassificationDraft),
        Arc::new(CommitCategorizationRule),
        Arc::new(CommitTransactionCategories),
    ]
}

/// The MCP-only transfer linking tools: find unlinked transfers with their
/// candidates, then link or unlink pairs. Not exposed to the in-app assistant,
/// which links through the Link Transfer dialog. Linking is a `Write` and is
/// filtered out of MCP this release; only [`FindTransferMatches`] stays exposed.
pub fn transfer_link_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(FindTransferMatches),
        Arc::new(LinkTransferActivities),
        Arc::new(UnlinkTransferActivities),
    ]
}

/// The MCP-only activity update tools: preview corrections to existing
/// activities, then apply them in place. Not exposed to the in-app assistant,
/// which has no confirmation step for them; users edit in the activity grid.
/// Both are hidden from MCP this release: the apply step is a `Write` and the
/// preview is in [`crate::catalog::MCP_HIDDEN_TOOLS`].
pub fn activity_update_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(PrepareActivityUpdates),
        Arc::new(CommitActivityUpdates),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog order is LLM-visible (tool listing) and mirrors the
    /// read tools' historical relative order — append-only.
    #[test]
    fn v1_read_tools_names_and_order_are_stable() {
        let names: Vec<&str> = v1_read_tools().iter().map(|tool| tool.name()).collect();
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
            ]
        );
    }
}
