//! Original-source verification is independent of legacy DTO fallback parsing.
use super::Quote;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteSourceQuality {
    Verified,
    InvalidPrice,
    InvalidTimestamp,
    InvalidCurrency,
    InconsistentSource,
    Unavailable,
    Unverified,
}
pub type QuoteSourceChecks = HashMap<String, QuoteSourceQuality>;
pub fn unverified_quote_sources(quotes: &[Quote]) -> QuoteSourceChecks {
    quotes
        .iter()
        .map(|quote| (quote.id.clone(), QuoteSourceQuality::Unverified))
        .collect()
}
