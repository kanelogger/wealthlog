#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use wealthfolio_business::investment_journal::*;
use wealthfolio_portfolio_engine::model::*;

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}
fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
}
fn activity(id: &str, kind: &str, quantity: Option<&str>, amount: Option<&str>) -> RawActivity {
    RawActivity {
        id: id.into(),
        account_id: "a".into(),
        asset_id: quantity.map(|_| "stock".into()),
        activity_type: kind.into(),
        activity_type_override: None,
        subtype: None,
        status: "POSTED".into(),
        timestamp: Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap(),
        created_at: Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap(),
        quantity: quantity.map(d),
        unit_price: quantity.map(|_| d("10")),
        amount: amount.map(d),
        fee: Some(d("1")),
        tax: Some(d("0.5")),
        currency: "CNY".into(),
        fx_rate: None,
        source_group_id: None,
        external_transfer: None,
        fx_conversion: None,
        source_system: Some("MANUAL".into()),
        is_user_modified: false,
        updated_at: Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap(),
    }
}
fn facts() -> RawFacts {
    RawFacts {
        policy: Policy::new(
            Currency::parse("CNY").unwrap(),
            "Asia/Shanghai".parse().unwrap(),
            day(),
        ),
        accounts: vec![RawAccount {
            id: "a".into(),
            currency: "CNY".into(),
            account_type: "SECURITIES".into(),
            tracking_mode: "TRANSACTIONS".into(),
            is_archived: false,
            cost_basis_method: None,
        }],
        assets: vec![RawAsset {
            id: "stock".into(),
            quote_currency: "CNY".into(),
            kind: "INVESTMENT".into(),
            instrument_type: Some("EQUITY".into()),
            contract_multiplier: None,
        }],
        activities: vec![activity("deposit", "DEPOSIT", None, Some("1000"))],
        quotes: vec![],
        fx_rates: vec![],
        observed_snapshots: vec![],
    }
}
fn delta(preview: &TradeImpactPreview, resource: FundResource) -> Decimal {
    preview
        .changes
        .iter()
        .find(|c| c.resource == resource)
        .map(|c| c.delta.value())
        .unwrap_or_default()
}

#[test]
fn final_amount_already_contains_fees_and_fx_books_once() {
    let mut trade = activity("buy", "BUY", Some("10"), Some("101.5"));
    let preview = preview_ledger_trade(facts(), trade.clone()).unwrap();
    assert!(
        preview.automatic_allocation_eligible,
        "{:?}",
        preview.issues
    );
    assert_eq!(preview.net_cash.unwrap().value(), d("-101.5"));
    assert_eq!(
        delta(
            &preview,
            FundResource::Security {
                account_id: "a".into(),
                asset_id: "stock".into()
            }
        ),
        d("10")
    );
    trade.currency = "USD".into();
    trade.fx_rate = Some(d("7.2"));
    let preview = preview_ledger_trade(facts(), trade.clone()).unwrap();
    assert_eq!(preview.net_cash.unwrap().value(), d("-730.8"));
    assert_eq!(preview.settlement_currency.as_deref(), Some("CNY"));
    assert!(
        preview.automatic_allocation_eligible,
        "{:?}",
        preview.issues
    );
    trade.fx_rate = None;
    let preview = preview_ledger_trade(facts(), trade).unwrap();
    assert_eq!(preview.net_cash.unwrap().value(), d("-101.5"));
    assert_eq!(preview.settlement_currency.as_deref(), Some("USD"));
}

#[test]
fn missing_final_cash_is_not_explicit_zero_and_missing_fees_remain_unknown() {
    let missing = preview_ledger_trade(facts(), activity("buy", "BUY", Some("10"), None)).unwrap();
    assert!(!missing.automatic_allocation_eligible);
    assert!(missing.net_cash.is_none());
    assert!(missing.issues.iter().any(|i| i == "MissingFinalCash"));
    let zero =
        preview_ledger_trade(facts(), activity("buy", "BUY", Some("10"), Some("0"))).unwrap();
    assert!(zero.automatic_allocation_eligible, "{:?}", zero.issues);
    assert_eq!(zero.net_cash.unwrap().value(), Decimal::ZERO);
    let mut trade = activity("buy", "BUY", Some("10"), Some("100"));
    trade.fee = None;
    trade.tax = None;
    let unknown = preview_ledger_trade(facts(), trade).unwrap();
    assert!(
        unknown.automatic_allocation_eligible,
        "{:?}",
        unknown.issues
    );
    assert!(unknown.fee.is_none() && unknown.tax.is_none());
}

#[test]
fn cost_reordering_does_not_block_matching_resources_but_quantity_mismatch_does() {
    let mut existing = facts();
    let mut old = activity("old-buy", "BUY", Some("10"), Some("100"));
    old.timestamp = Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
    existing.activities.push(old);
    let sell = activity("sell", "SELL", Some("5"), Some("60"));
    let preview = preview_ledger_trade(existing.clone(), sell.clone()).unwrap();
    assert!(
        preview.automatic_allocation_eligible,
        "{:?}",
        preview.issues
    );
    existing
        .activities
        .push(activity("other-buy", "BUY", Some("3"), Some("45")));
    let preview = preview_ledger_trade(existing, sell).unwrap();
    assert!(
        preview.automatic_allocation_eligible,
        "{:?}",
        preview.issues
    );
    let oversell = preview_ledger_trade(
        facts(),
        activity("oversell", "SELL", Some("20"), Some("200")),
    )
    .unwrap();
    assert!(!oversell.automatic_allocation_eligible);
    assert!(oversell
        .issues
        .iter()
        .any(|i| i == "ResourceImpactMismatch"));
}

#[test]
fn existing_source_preview_reuses_one_fact_set_and_refresh_is_not_a_conflict() {
    let trade = activity("buy", "BUY", Some("10"), Some("101.5"));
    let mut existing = facts();
    existing.activities.push(trade.clone());
    let a = preview_ledger_trade(existing.clone(), trade.clone()).unwrap();
    let b = preview_ledger_trade(facts(), trade.clone()).unwrap();
    assert_eq!(a.facts_hash, b.facts_hash);
    assert_eq!(a.changes, b.changes);
    existing.quotes.push(RawQuote {
        asset_id: "stock".into(),
        day: day(),
        close: d("999"),
        currency: "CNY".into(),
        source: "TEST".into(),
    });
    let c = preview_ledger_trade(existing, trade).unwrap();
    assert_eq!(a.facts_hash, c.facts_hash);
}

#[test]
fn blank_type_override_preserves_the_underlying_trade_kind() {
    let mut trade = activity("buy", "BUY", Some("10"), Some("100"));
    trade.activity_type_override = Some("  ".into());
    assert!(
        preview_ledger_trade(facts(), trade)
            .unwrap()
            .automatic_allocation_eligible
    );
}

#[test]
fn multiple_purposes_conserve_settlement_cash_keep_fee_currency_and_only_use_unallocated_topups() {
    use std::collections::BTreeMap;
    let mut trade = activity("buy", "BUY", Some("10"), Some("101.5"));
    trade.currency = "USD".into();
    trade.fx_rate = Some(d("7.2"));
    let preview = preview_ledger_trade(facts(), trade).unwrap();
    let cash = FundResource::Cash {
        account_id: "a".into(),
        currency: "CNY".into(),
    };
    let stock = FundResource::Security {
        account_id: "a".into(),
        asset_id: "stock".into(),
    };
    let observation = |resource, amount: &str| ResourceObservation {
        resource,
        actual: Some(ExactDecimal::parse(amount).unwrap()),
        observed_at: "now".into(),
        as_of_date: day(),
        tracking_mode: "TRANSACTIONS".into(),
        state_version: "version".into(),
        sources: vec![],
        unresolved_sources: vec![],
        issues: vec![],
    };
    let observations = vec![observation(cash.clone(), "1000"), observation(stock, "0")];
    let share = |id: &str, quantity: &str, amount: &str| TradePurposeShare {
        purpose_id: id.into(),
        quantity: ExactDecimal::parse(quantity).unwrap(),
        net_cash: ExactDecimal::parse(amount).unwrap(),
        top_ups: vec![OwnershipTopUp {
            resource: cash.clone(),
            amount: ExactDecimal::new(ExactDecimal::parse(amount).unwrap().value().abs()),
        }],
    };
    let shares = vec![
        share("first", "6", "-438.48"),
        share("second", "4", "-292.32"),
    ];
    let result =
        preview_trade_ownership(&preview, &shares, &BTreeMap::new(), &observations).unwrap();
    assert_eq!(result.charges[0].currency, "USD");
    assert_eq!(result.charges[0].fee.unwrap().value(), d("0.6"));
    assert_eq!(result.charges[1].tax.unwrap().value(), d("0.2"));
    let unavailable = BTreeMap::from([(
        ("other-purpose".into(), cash),
        ExactDecimal::parse("950").unwrap(),
    )]);
    assert!(matches!(
        preview_trade_ownership(&preview, &shares, &unavailable, &observations),
        Err(JournalError::InsufficientResource)
    ));
    let mut missing_topup = shares.clone();
    missing_topup[0].top_ups.clear();
    assert!(matches!(
        preview_trade_ownership(&preview, &missing_topup, &BTreeMap::new(), &observations),
        Err(JournalError::NegativeAllocation)
    ));
    let mut mismatched = shares;
    mismatched[0].quantity = ExactDecimal::parse("5").unwrap();
    assert!(matches!(
        preview_trade_ownership(&preview, &mismatched, &BTreeMap::new(), &observations),
        Err(JournalError::InvalidInput(_))
    ));
}

#[test]
fn journal_preview_keeps_derivatives_and_alternative_assets_out_of_the_first_release() {
    let trade = activity("buy", "BUY", Some("10"), Some("100"));
    let mut unsupported = facts();
    unsupported.assets[0].instrument_type = Some("OPTION".into());
    assert!(matches!(
        preview_ledger_trade(unsupported, trade.clone()),
        Err(JournalError::InvalidInput(_))
    ));
    let mut legacy = facts();
    legacy.assets[0].instrument_type = None;
    assert!(
        preview_ledger_trade(legacy, trade.clone())
            .unwrap()
            .automatic_allocation_eligible
    );
    let mut property = facts();
    property.assets[0].kind = "PROPERTY".into();
    assert!(matches!(
        preview_ledger_trade(property, trade),
        Err(JournalError::InvalidInput(_))
    ));
}
