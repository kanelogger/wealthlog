//! Tauri-side implementation of AgentEnvironment.
//!
//! Provides external MCP tools with access to Tauri services
//! for tool execution and settings management.

use std::sync::{Arc, RwLock};
use wealthfolio_agent_tools::AgentEnvironment;

use wealthfolio_core::{
    accounts::AccountServiceTrait, activities::ActivityServiceTrait,
    allocation::AllocationServiceTrait, assets::AssetServiceTrait, health::HealthServiceTrait,
    holdings::HoldingsServiceTrait, income::IncomeServiceTrait,
    performance::PerformanceServiceTrait, portfolio::net_worth::NetWorthServiceTrait,
    portfolios::PortfolioServiceTrait, quotes::QuoteServiceTrait, settings::SettingsServiceTrait,
    taxonomies::TaxonomyServiceTrait, valuation::ValuationServiceTrait,
};
use wealthfolio_spending::cash_activities::{CashActivityService, CashActivityServiceTrait};
use wealthfolio_spending::categorization_rules::{
    CategorizationRulesService, CategorizationRulesServiceTrait,
};

/// Tauri-side implementation of AgentEnvironment.
///
/// Wraps existing services from ServiceContext to provide access
/// to external MCP tools for tool execution.
pub struct TauriAgentEnvironment {
    base_currency: Arc<RwLock<String>>,
    account_service: Arc<dyn AccountServiceTrait + Send + Sync>,
    activity_service: Arc<dyn ActivityServiceTrait + Send + Sync>,
    holdings_service: Arc<dyn HoldingsServiceTrait + Send + Sync>,
    valuation_service: Arc<dyn ValuationServiceTrait + Send + Sync>,

    settings_service: Arc<dyn SettingsServiceTrait + Send + Sync>,

    quote_service: Arc<dyn QuoteServiceTrait + Send + Sync>,
    asset_service: Arc<dyn AssetServiceTrait + Send + Sync>,
    allocation_service: Arc<dyn AllocationServiceTrait + Send + Sync>,
    performance_service: Arc<dyn PerformanceServiceTrait + Send + Sync>,
    income_service: Arc<dyn IncomeServiceTrait + Send + Sync>,
    health_service: Arc<dyn HealthServiceTrait + Send + Sync>,
    taxonomy_service: Arc<dyn TaxonomyServiceTrait + Send + Sync>,
    portfolio_service: Arc<dyn PortfolioServiceTrait + Send + Sync>,
    net_worth_service: Arc<dyn NetWorthServiceTrait + Send + Sync>,

    cash_activity_service: Arc<CashActivityService>,

    categorization_rules_service: Arc<CategorizationRulesService>,

    business: Arc<wealthfolio_business::BusinessService>,
    pending: Arc<wealthfolio_business::PendingService>,
    investments: Option<Arc<wealthfolio_business::investments::InvestmentService>>,
}

impl TauriAgentEnvironment {
    /// Create a new Tauri AI environment.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_currency: Arc<RwLock<String>>,
        account_service: Arc<dyn AccountServiceTrait + Send + Sync>,
        activity_service: Arc<dyn ActivityServiceTrait + Send + Sync>,
        holdings_service: Arc<dyn HoldingsServiceTrait + Send + Sync>,
        valuation_service: Arc<dyn ValuationServiceTrait + Send + Sync>,

        settings_service: Arc<dyn SettingsServiceTrait + Send + Sync>,

        quote_service: Arc<dyn QuoteServiceTrait + Send + Sync>,
        asset_service: Arc<dyn AssetServiceTrait + Send + Sync>,
        allocation_service: Arc<dyn AllocationServiceTrait + Send + Sync>,
        performance_service: Arc<dyn PerformanceServiceTrait + Send + Sync>,
        income_service: Arc<dyn IncomeServiceTrait + Send + Sync>,
        health_service: Arc<dyn HealthServiceTrait + Send + Sync>,
        taxonomy_service: Arc<dyn TaxonomyServiceTrait + Send + Sync>,
        portfolio_service: Arc<dyn PortfolioServiceTrait + Send + Sync>,
        net_worth_service: Arc<dyn NetWorthServiceTrait + Send + Sync>,

        cash_activity_service: Arc<CashActivityService>,

        categorization_rules_service: Arc<CategorizationRulesService>,

        business: Arc<wealthfolio_business::BusinessService>,
        pending: Arc<wealthfolio_business::PendingService>,
    ) -> Self {
        Self {
            base_currency,
            account_service,
            activity_service,
            holdings_service,
            valuation_service,

            settings_service,

            quote_service,
            asset_service,
            allocation_service,
            performance_service,
            income_service,
            health_service,
            taxonomy_service,
            portfolio_service,
            net_worth_service,

            cash_activity_service,

            categorization_rules_service,

            business,
            pending,
            investments: None,
        }
    }

    pub fn with_investments(
        mut self,
        service: Arc<wealthfolio_business::investments::InvestmentService>,
    ) -> Self {
        self.investments = Some(service);
        self
    }
}

impl AgentEnvironment for TauriAgentEnvironment {
    fn base_currency(&self) -> String {
        self.base_currency
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn account_service(&self) -> Arc<dyn AccountServiceTrait> {
        self.account_service.clone()
    }

    fn activity_service(&self) -> Arc<dyn ActivityServiceTrait> {
        self.activity_service.clone()
    }

    fn holdings_service(&self) -> Arc<dyn HoldingsServiceTrait> {
        self.holdings_service.clone()
    }

    fn valuation_service(&self) -> Arc<dyn ValuationServiceTrait> {
        self.valuation_service.clone()
    }

    fn settings_service(&self) -> Arc<dyn SettingsServiceTrait> {
        self.settings_service.clone()
    }

    fn quote_service(&self) -> Arc<dyn QuoteServiceTrait> {
        self.quote_service.clone()
    }

    fn asset_service(&self) -> Arc<dyn AssetServiceTrait> {
        self.asset_service.clone()
    }

    fn allocation_service(&self) -> Arc<dyn AllocationServiceTrait> {
        self.allocation_service.clone()
    }

    fn performance_service(&self) -> Arc<dyn PerformanceServiceTrait> {
        self.performance_service.clone()
    }

    fn income_service(&self) -> Arc<dyn IncomeServiceTrait> {
        self.income_service.clone()
    }

    fn health_service(&self) -> Arc<dyn HealthServiceTrait> {
        self.health_service.clone()
    }

    fn taxonomy_service(&self) -> Arc<dyn TaxonomyServiceTrait> {
        self.taxonomy_service.clone()
    }

    fn portfolio_service(&self) -> Arc<dyn PortfolioServiceTrait> {
        self.portfolio_service.clone()
    }

    fn net_worth_service(&self) -> Arc<dyn NetWorthServiceTrait> {
        self.net_worth_service.clone()
    }

    fn cash_activity_service(&self) -> Arc<dyn CashActivityServiceTrait> {
        self.cash_activity_service.clone()
    }

    fn categorization_rules_service(&self) -> Arc<dyn CategorizationRulesServiceTrait> {
        self.categorization_rules_service.clone()
    }

    fn business_service(&self) -> Option<Arc<wealthfolio_business::BusinessService>> {
        Some(self.business.clone())
    }

    fn investment_service(
        &self,
    ) -> Option<Arc<wealthfolio_business::investments::InvestmentService>> {
        self.investments.clone()
    }

    fn pending_service(&self) -> Option<Arc<wealthfolio_business::PendingService>> {
        Some(self.pending.clone())
    }
}
