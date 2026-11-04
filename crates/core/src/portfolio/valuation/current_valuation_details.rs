//! Exact, reusable diagnostics emitted by the existing current-valuation branch.
use super::CurrentValuationResponse;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CurrentValuationIssueCode {
    ProjectionNotCurrent,
    MissingSnapshot,
    MissingFinalCash,
    MissingAsset,
    MissingQuote,
    InvalidQuoteSource,
    ArithmeticOverflow,
    MissingFx,
    AccountNotEligible,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValuationComponent {
    Cash,
    Investments,
    All,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentValuationIssue {
    pub code: CurrentValuationIssueCode,
    pub component: ValuationComponent,
    pub account_id: String,
    pub asset_id: Option<String>,
    pub currency: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentResourceValuation {
    pub kind: String,
    pub account_id: String,
    pub asset_id: Option<String>,
    pub currency: Option<String>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub native_amount: Option<Decimal>,
    pub valuation_currency: Option<String>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub unit_value: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub local_value: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub value_base: Option<Decimal>,
    pub valuation_date: Option<NaiveDate>,
    pub fx_source_date: Option<NaiveDate>,
    pub stale: bool,
    pub issues: Vec<CurrentValuationIssue>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailedCurrentAccountValuation {
    pub account_id: String,
    pub account_currency: String,
    pub base_currency: String,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub cash_balance_base: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub investment_market_value_base: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub total_value_base: Option<Decimal>,
    pub source_data_as_of: Option<NaiveDate>,
    pub calculated_at: DateTime<Utc>,
    pub warnings: Vec<String>,
    pub issues: Vec<CurrentValuationIssue>,
    pub resources: Vec<CurrentResourceValuation>,
    pub stale: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailedCurrentValuationResponse {
    #[serde(skip)]
    pub legacy: CurrentValuationResponse,
    pub scope_id: String,
    pub base_currency: String,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub cash_balance_base: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub investment_market_value_base: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub total_value_base: Option<Decimal>,
    pub source_data_as_of: Option<NaiveDate>,
    pub calculated_at: DateTime<Utc>,
    pub accounts: Vec<DetailedCurrentAccountValuation>,
    pub issues: Vec<CurrentValuationIssue>,
    pub stale: bool,
}
