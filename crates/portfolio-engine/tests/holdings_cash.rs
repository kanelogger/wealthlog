//! Cash transfers stay live while securities remain manually observed.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod support;
use chrono::NaiveDate;
use rust_decimal_macros::dec;
use support::*;
use wealthfolio_portfolio_engine::{aggregate_scope, model::*, Window};

fn scenario() -> Scenario {
    serde_yaml::from_str(include_str!("fixtures/manual-holdings-cash.yaml")).unwrap()
}

#[test]
fn transfers_change_cash_and_not_securities_or_portfolio_returns() {
    let pipeline = Pipeline::from_scenario(&scenario());
    let broker = &pipeline.series[&AccountId::new("broker")];
    let day = |date| {
        broker
            .days
            .iter()
            .find(|d| d.date == NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap())
            .unwrap()
    };
    assert_eq!(day("2026-10-29").cash_balance, dec!(67002.17));
    assert_eq!(day("2026-10-29").investment_market_value, dec!(1000));
    assert_eq!(day("2026-10-29").flow.inflow_base, dec!(5000));
    assert_eq!(day("2026-10-30").cash_balance, dec!(66002.17));
    assert_eq!(day("2026-10-30").flow.outflow_base, dec!(1000));
    assert_eq!(day("2026-10-31").cash_balance, dec!(65000));
    assert_eq!(day("2026-11-02").cash_balance, dec!(66200));
    let portfolio = aggregate_scope(
        &pipeline.effects(
            &pipeline.bundle.disposals,
            &pipeline.lots(),
            &pipeline.bundle.rejected_activities(),
        ),
        &pipeline.series,
        &pipeline.portfolio_scope(),
        Window::default(),
    )
    .unwrap();
    let before = portfolio
        .days
        .iter()
        .find(|d| d.date.to_string() == "2026-10-28")
        .unwrap();
    let after = portfolio
        .days
        .iter()
        .find(|d| d.date.to_string() == "2026-10-29")
        .unwrap();
    assert_eq!(before.total_value_base, after.total_value_base);
    assert_eq!(after.flow.inflow_base, dec!(0));
    assert_eq!(after.flow.outflow_base, dec!(0));
}

#[test]
fn editing_or_removing_a_transfer_recomputes_from_the_original_snapshot() {
    let mut raw = scenario().raw_facts();
    raw.activities.retain(|a| a.id != "tin" && a.id != "out");
    let pipeline = Pipeline::run(raw.clone()).unwrap();
    let day = &pipeline.series[&AccountId::new("broker")].days[2];
    assert_eq!(day.cash_balance, dec!(62002.17));
    let mut transfer = scenario()
        .raw_facts()
        .activities
        .into_iter()
        .find(|a| a.id == "tin")
        .unwrap();
    transfer.amount = Some(dec!(3000));
    transfer.source_group_id = None;
    transfer.external_transfer = Some(true);
    raw.activities.push(transfer);
    let pipeline = Pipeline::run(raw).unwrap();
    assert_eq!(
        pipeline.series[&AccountId::new("broker")].days[2].cash_balance,
        dec!(65002.17)
    );
}

#[test]
fn unpriced_securities_do_not_erase_a_known_cash_transfer() {
    let mut raw = scenario().raw_facts();
    raw.quotes.clear();
    raw.observed_snapshots[1].date = NaiveDate::from_ymd_opt(2026, 10, 30).unwrap();
    let pipeline = Pipeline::run(raw).unwrap();
    let broker = &pipeline.series[&AccountId::new("broker")];
    let day = broker
        .days
        .iter()
        .find(|d| d.date.to_string() == "2026-10-30")
        .unwrap();
    assert_eq!(day.flow.outflow_base, dec!(1000));
    assert_eq!(day.flow.source, FlowSource::UnpricedHoldingsTransition);
    assert_ne!(day.value_status, ValueStatus::Complete);
}
