//! Actual native services and migrated temporary ledgers; no live account data.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use serde_json::{json, Value};
use support::{journal_caller, manual_cash, Harness};
use wealthfolio_business::{investment_journal::*, BusinessChannel};

fn query(purposes: &[&str], accounts: &[&str]) -> InvestmentContextQuery {
    InvestmentContextQuery {
        purpose_ids: purposes.iter().map(|s| s.to_string()).collect(),
        account_ids: accounts.iter().map(|s| s.to_string()).collect(),
    }
}
async fn native(h: &Harness, q: InvestmentContextQuery) -> InvestmentContextCapture {
    h.journal()
        .with_context_reader(h.context.clone())
        .get_investment_context(q, &journal_caller())
        .await
        .unwrap()
}
fn seed_transaction_snapshot(h: &Harness) {
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO holdings_snapshots(id,account_id,snapshot_date,currency,positions,cash_balances,cost_basis,net_contribution,calculated_at,net_contribution_base,cash_total_account_currency,cash_total_base_currency,source) VALUES('tx-current','transactions',?1,'CNY','{}','{\"CNY\":\"1000\"}','0','1000','2000-01-01T00:00:00','1000','1000','1000','CALCULATED')",[&h.day]).unwrap();
    // This fixture explicitly represents a completed projection of the deposit.
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE projection_state SET dirty_from=NULL WHERE scope IN ('@all','transactions')",
            [],
        )
        .unwrap();
}
fn seed_quote(h: &Harness, asset: &str, price: &str, currency: &str, day: &str) {
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES(?1,?2,?3,'MANUAL',?4,?5,CURRENT_TIMESTAMP,?6)",(format!("{asset}-{day}"),asset,day,price,currency,format!("{day}T00:00:00Z"))).unwrap();
}
#[tokio::test]
async fn household_is_global_keeps_unvalued_names_and_never_adds_allocations_to_assets() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    seed_transaction_snapshot(&h);
    h.seed_purpose("purpose");
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO assets(id,kind,name,display_code,is_active,quote_mode,quote_ccy,created_at,updated_at) VALUES('home','PROPERTY','Home','HOME',1,'MANUAL','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP),('vehicle','VEHICLE','Unvalued vehicle','CAR',1,'MANUAL','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP),('foreign-property','PROPERTY','Foreign property','PROP',1,'MANUAL','EUR',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP);").unwrap();
    let old = (chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap()
        - chrono::Duration::days(100))
    .to_string();
    seed_quote(&h, "home", "5000", "CNY", &old);
    seed_quote(&h, "foreign-property", "10", "EUR", &h.day);
    let before = native(&h, query(&[], &["manual"])).await;
    assert_eq!(before.response.household.quality, ContextQuality::Partial);
    assert!(before.response.household.stale);
    let household = &before.response.household.data;
    assert_eq!(household["totalAssets"], Value::Null);
    assert_eq!(household["netWorth"], Value::Null);
    assert_eq!(household["financialAssets"], "2000");
    assert_eq!(household["otherAssets"], Value::Null);
    let items = household["items"].as_array().unwrap();
    let home = items.iter().find(|i| i["id"] == "home").unwrap();
    assert_eq!(home["value"], "5000");
    assert_eq!(home["valuationDate"], old);
    assert_eq!(home["stale"], true);
    let vehicle = items.iter().find(|i| i["id"] == "vehicle").unwrap();
    assert_eq!(vehicle["name"], "Unvalued vehicle");
    assert_eq!(vehicle["value"], Value::Null);
    let foreign = items
        .iter()
        .find(|i| i["id"] == "foreign-property")
        .unwrap();
    assert_eq!(foreign["value"], Value::Null);
    assert_eq!(foreign["substituteValue"], Value::Null);
    assert!(before
        .response
        .household
        .issues
        .iter()
        .any(|i| i.code == "MISSING_FX"));
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "native reads do not enable"
    );
    let service = h.journal();
    let p = service
        .preview_allocations(vec![manual_cash()], &journal_caller())
        .await
        .unwrap();
    service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Explicit ownership".into(),
                entries: vec![AllocationDelta {
                    purpose_id: "purpose".into(),
                    resource: manual_cash(),
                    delta: ExactDecimal::parse("600").unwrap(),
                }],
                resources: vec![manual_cash()],
                expected_preview_hash: p.preview_hash,
                request_key: "assign".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap();
    let after = native(&h, query(&["purpose"], &["transactions"])).await;
    assert_eq!(
        before.response.household.data, after.response.household.data,
        "page account selection and purpose ownership do not add assets or filter household scope"
    );
    assert_eq!(after.response.funding.data["risks"][0]["totalValue"], "600");
    assert_eq!(
        after.response.funding.data["risks"][0]["lossComparisonStatus"],
        "NO_SCENARIO_PROVIDED"
    );
}

#[tokio::test]
async fn real_context_conflicts_on_prices_and_freezes_exact_displayed_context() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    seed_transaction_snapshot(&h);
    h.seed_purpose("purpose");
    h.access.connect_rusqlite().unwrap().execute_batch("INSERT INTO assets(id,kind,name,display_code,is_active,quote_mode,quote_ccy,created_at,updated_at) VALUES('context-home','PROPERTY','Home','HOME',1,'MANUAL','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP);").unwrap();
    seed_quote(&h, "context-home", "1000", "CNY", &h.day);
    let service = h.journal().with_context_reader(h.context.clone());
    let mut caller = journal_caller();
    caller.can_confirm = true;
    let body:DecisionBody=serde_json::from_value(json!({"title":"计划","action":"BUY","thesis":"用户的理由","purposeIds":["purpose"],"fundingStatus":"PLANNED","funding":[{"purposeId":"purpose","resource":{"kind":"CASH","accountId":"manual","currency":"CNY"},"planned":"100"}],"options":[{"action":"等待","description":"暂不成交"}]})).unwrap();
    let saved = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "draft".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    let target = DecisionDraftTarget {
        decision_id: saved["decisionId"].as_str().unwrap().into(),
        draft_revision_id: saved["draftRevisionId"].as_str().unwrap().into(),
        expected_version: 1,
    };
    let first = service
        .preview_decision_confirmation(target.clone(), &caller)
        .await
        .unwrap();
    let second = service
        .preview_decision_confirmation(target.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(
        first.expected_context_hash, second.expected_context_hash,
        "refresh clocks are not financial source versions"
    );
    h.access.connect_rusqlite().unwrap().execute("UPDATE holdings_snapshots SET calculated_at='2099-01-01T00:00:00' WHERE id='tx-current'",[]).unwrap();
    let refreshed = service
        .preview_decision_confirmation(target.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(
        first.expected_context_hash, refreshed.expected_context_hash,
        "a derived refresh date must not change the displayed data date"
    );
    let input = ConfirmDecisionDraft {
        ledger_id: "ledger".into(),
        target: target.clone(),
        expected_content_hash: first.expected_content_hash,
        expected_context_hash: first.expected_context_hash,
        context: first.context,
        request_key: "confirm".into(),
    };
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE quotes SET close='1100' WHERE asset_id='context-home'",
            [],
        )
        .unwrap();
    assert_eq!(
        service
            .confirm_decision_draft(input, &caller)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let shown = service
        .preview_decision_confirmation(target, &caller)
        .await
        .unwrap();
    let expected = serde_json::to_value(&shown.context).unwrap();
    let input = ConfirmDecisionDraft {
        ledger_id: "ledger".into(),
        target: shown.target,
        expected_content_hash: shown.expected_content_hash,
        expected_context_hash: shown.expected_context_hash,
        context: shown.context,
        request_key: "confirm".into(),
    };
    service
        .confirm_decision_draft(input, &caller)
        .await
        .unwrap();
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE quotes SET close='1200' WHERE asset_id='context-home'",
            [],
        )
        .unwrap();
    assert_eq!(
        service
            .get_decision(saved["decisionId"].as_str().unwrap(), &caller)
            .unwrap()["revisions"][0]["context"],
        expected
    );
}

#[tokio::test]
async fn native_recent_cash_flow_covers_all_rows_and_retains_missing_amounts() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('cash','Cash','CASH','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS');").unwrap();
    for i in 0..256 {
        db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES(?1,'cash','DEPOSIT','POSTED',?2,'CNY','0.01',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(format!("ordinary-{i}"),format!("{}T12:00:00+08:00",h.day))).unwrap();
    }
    let first = native(&h, query(&[], &[])).await;
    assert_eq!(
        first.response.recent_cash_flow.data["ordinary"]["recordCount"],
        256
    );
    assert_eq!(
        first.response.recent_cash_flow.data["convertedOrdinary"]["income"],
        "2.56"
    );
    assert_eq!(
        first.response.recent_cash_flow.quality,
        ContextQuality::Complete
    );
    db.execute(
        "UPDATE activities SET amount=NULL WHERE id='ordinary-0'",
        [],
    )
    .unwrap();
    let changed = native(&h, query(&[], &[])).await;
    assert_eq!(
        changed.response.recent_cash_flow.data["convertedOrdinary"]["income"],
        Value::Null
    );
    assert_eq!(
        changed.response.recent_cash_flow.quality,
        ContextQuality::Partial
    );
    assert_eq!(
        changed.response.recent_cash_flow.data["ordinary"]["currencies"][0]["knownIncome"],
        "2.55"
    );
    assert!(changed
        .response
        .recent_cash_flow
        .issues
        .iter()
        .any(|i| i.code == "MISSING_AMOUNT"));
}

async fn assign(h: &Harness, entries: Vec<AllocationDelta>, key: &str) {
    let resources: Vec<_> = entries.iter().map(|e| e.resource.clone()).collect();
    let service = h.journal();
    let c = journal_caller();
    let p = service
        .preview_allocations(resources.clone(), &c)
        .await
        .unwrap();
    service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Explicit test ownership".into(),
                entries,
                resources,
                expected_preview_hash: p.preview_hash,
                request_key: key.into(),
            },
            &c,
        )
        .await
        .unwrap();
}
fn delta(resource: FundResource, amount: &str) -> AllocationDelta {
    AllocationDelta {
        purpose_id: "purpose".into(),
        resource,
        delta: ExactDecimal::parse(amount).unwrap(),
    }
}

#[tokio::test]
async fn paired_transfer_and_trade_ownership_stay_separate_from_ordinary_income() {
    use wealthfolio_business::investments::CreateCashTransfer;
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.seed_purpose("purpose");
    h.investments
        .create_transfer(
            CreateCashTransfer {
                from_account_id: "transactions".into(),
                to_account_id: "manual".into(),
                activity_date: h.day.clone(),
                source_currency: "CNY".into(),
                destination_currency: "CNY".into(),
                source_amount: "100".into(),
                destination_amount: Some("100".into()),
                notes: None,
                request_key: "transfer".into(),
                idempotency_key: None,
            },
            BusinessChannel::Gui,
            "session",
            true,
        )
        .await
        .unwrap();
    assign(&h, vec![delta(manual_cash(), "50")], "initial").await;
    let trade = h.trade().await;
    let c = journal_caller();
    let service = h.journal();
    let shares = vec![TradePurposeShare {
        purpose_id: "purpose".into(),
        quantity: ExactDecimal::parse("10").unwrap(),
        net_cash: ExactDecimal::parse("-101.5").unwrap(),
        top_ups: vec![OwnershipTopUp {
            resource: manual_cash(),
            amount: ExactDecimal::parse("51.5").unwrap(),
        }],
    }];
    let p = service
        .preview_manual_trade_ownership(&trade, shares.clone(), &c)
        .await
        .unwrap();
    service
        .record_manual_trade_with_ownership(
            RecordManualTradeWithOwnership {
                trade,
                shares,
                reason: "Record actual execution".into(),
                expected_preview_hash: p.preview_hash,
            },
            &c,
        )
        .await
        .unwrap();
    let captured = native(&h, query(&["purpose"], &["manual"])).await;
    let flow = &captured.response.recent_cash_flow;
    assert_eq!(flow.quality, ContextQuality::Complete);
    assert_eq!(flow.data["convertedOrdinary"]["income"], "0");
    assert_eq!(flow.data["convertedOrdinary"]["expense"], "0");
    let transfers = flow.data["internalCashTransfers"].as_array().unwrap();
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0]["sourceAmount"], "100");
    assert_eq!(transfers[0]["destinationAmount"], "100");
    let purposes = &flow.data["purposeFlows"];
    assert_eq!(purposes["allocationIn"].as_array().unwrap().len(), 2);
    assert_eq!(purposes["tradeConversions"].as_array().unwrap().len(), 2);
    let converted_cash = purposes["tradeConversions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["entry"]["resource"]["kind"] == "CASH")
        .unwrap();
    assert_eq!(
        converted_cash["entry"]["delta"], "-101.5",
        "original fees are not charged again"
    );
    assert_eq!(
        purposes["allocationIn"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["leg"] == "TOP_UP")
            .unwrap()["entry"]["delta"],
        "51.5"
    );
}

#[tokio::test]
async fn purpose_risk_uses_all_owned_accounts_and_cannot_pass_with_unresolved_excess() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.seed_purpose("purpose");
    let mut trade = h.trade().await;
    trade.request_key = "native-trade".into();
    h.investments
        .record_manual_trade(trade, BusinessChannel::Gui, "session", true)
        .await
        .unwrap();
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('buy','transactions','stock','BUY','POSTED',?1,'10','10','100','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    seed_quote(&h, "stock", "10", "CNY", &h.day);
    let txcash = FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    };
    assign(
        &h,
        vec![
            delta(
                FundResource::Security {
                    account_id: "manual".into(),
                    asset_id: "stock".into(),
                },
                "10",
            ),
            delta(
                FundResource::Security {
                    account_id: "transactions".into(),
                    asset_id: "stock".into(),
                },
                "5",
            ),
            delta(txcash, "500"),
        ],
        "assign-risk",
    )
    .await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("UPDATE fund_purposes SET minimum_cash='600',loss_budget='100',single_asset_limit_bps=2307 WHERE id='purpose'",[]).unwrap();
    let first = native(&h, query(&["purpose"], &["manual"])).await;
    let risk = &first.response.funding.data["risks"][0];
    assert_eq!(risk["totalValue"], "650");
    assert_eq!(risk["cashValue"], "500");
    assert_eq!(risk["minimumCashStatus"], "BELOW_MINIMUM");
    assert_eq!(risk["singleAssetLimitStatus"], "ABOVE_LIMIT");
    assert_eq!(risk["assets"].as_array().unwrap().len(), 1);
    assert_eq!(risk["assets"][0]["value"], "150");
    assert_eq!(risk["lossBudget"], "100");
    assert_eq!(risk["lossComparisonStatus"], "NO_SCENARIO_PROVIDED");
    db.execute(
        "UPDATE fund_purposes SET single_asset_limit_bps=2308 WHERE id='purpose'",
        [],
    )
    .unwrap();
    assert_eq!(
        native(&h, query(&["purpose"], &["manual"]))
            .await
            .response
            .funding
            .data["risks"][0]["singleAssetLimitStatus"],
        "WITHIN_LIMIT"
    );
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES('spend','transactions','WITHDRAWAL','POSTED',?1,'CNY','600',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let changed = native(&h, query(&["purpose"], &["manual"])).await;
    let risk = &changed.response.funding.data["risks"][0];
    assert_eq!(risk["totalValue"], Value::Null);
    assert_eq!(risk["singleAssetLimitStatus"], "UNABLE_TO_DETERMINE");
    assert_eq!(risk["assets"][0]["value"], "150");
}

#[tokio::test]
async fn failure_in_household_source_does_not_block_other_context_blocks_or_drafts() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('cash','Cash','CASH','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS'); INSERT INTO assets(id,kind,name,display_code,is_active,quote_mode,quote_ccy,created_at,updated_at) VALUES('loan','LIABILITY','Loan','LOAN',1,'MANUAL','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP);").unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,metadata,created_at,updated_at) VALUES('ordinary','cash','DEPOSIT','POSTED',?1,'CNY','10','invalid-json',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let captured = native(&h, query(&[], &[])).await;
    assert_eq!(
        captured.response.household.quality,
        ContextQuality::Unavailable
    );
    assert_eq!(
        captured.response.recent_cash_flow.quality,
        ContextQuality::Complete
    );
    assert_eq!(
        captured.response.recent_cash_flow.data["convertedOrdinary"]["income"],
        "10"
    );
    let body:DecisionBody=serde_json::from_value(json!({"title":"继续研究","action":"RESEARCH","thesis":"待核实资料","options":[{"action":"等待","description":"等待修复来源"}]})).unwrap();
    h.journal()
        .with_context_reader(h.context.clone())
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "still-draft".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn decision_assessment_compares_user_loss_scenarios_and_plans_without_reserving_funds() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    seed_transaction_snapshot(&h);
    h.seed_purpose("purpose");
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE fund_purposes SET loss_budget='100' WHERE id='purpose'",
            [],
        )
        .unwrap();
    let service = h.journal().with_context_reader(h.context.clone());
    let mut c = journal_caller();
    c.can_confirm = true;
    let body:DecisionBody=serde_json::from_value(json!({"title":"比较假设","action":"BUY","thesis":"用户提供的判断","purposeIds":["purpose"],"fundingStatus":"PLANNED","funding":[{"purposeId":"purpose","resource":{"kind":"CASH","accountId":"manual","currency":"CNY"},"planned":"100"}],"evidence":[{"informationKind":"HYPOTHESIS","content":"明确的比较情景","purposeId":"purpose","assumedLossAmount":"200","lossCurrency":"CNY","lossBasis":"用户自定的假设，未作为预测"}],"options":[{"action":"等待","description":"暂不执行"}]})).unwrap();
    let saved = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "assess-draft".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let target = DecisionDraftTarget {
        decision_id: saved["decisionId"].as_str().unwrap().into(),
        draft_revision_id: saved["draftRevisionId"].as_str().unwrap().into(),
        expected_version: 1,
    };
    let shown = service
        .preview_decision_confirmation(target, &c)
        .await
        .unwrap();
    let assessment = &shown.context.response.funding.data["decisionAssessment"];
    assert_eq!(
        assessment["lossScenarios"][0]["status"],
        "SCENARIO_EXCEEDS_BUDGET"
    );
    assert_eq!(assessment["lossScenarios"][0]["lossBudget"], "100");
    assert_eq!(assessment["lossScenarios"][0]["assumedLossAmount"], "200");
    assert_eq!(assessment["lossScenarios"][0]["isPrediction"], false);
    assert_eq!(assessment["plannedFunding"][0]["purposeAmountBefore"], "0");
    assert_eq!(
        assessment["plannedFunding"][0]["purposeAmountAfter"],
        "-100"
    );
    assert_eq!(assessment["plannedFunding"][0]["shortfall"], "100");
    assert_eq!(
        assessment["plannedFunding"][0]["unallocatedAvailable"],
        "1000"
    );
    assert_eq!(
        assessment["plannedFunding"][0]["status"],
        "PURPOSE_SHORTFALL"
    );
    assert_eq!(assessment["reservesResources"], false);
    let displayed = serde_json::to_value(&shown.context).unwrap();
    service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: "ledger".into(),
                target: shown.target,
                expected_content_hash: shown.expected_content_hash,
                expected_context_hash: shown.expected_context_hash,
                context: shown.context,
                request_key: "assess-confirm".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.execute(
        "UPDATE fund_purposes SET loss_budget='1000' WHERE id='purpose'",
        [],
    )
    .unwrap();
    let recorded = service
        .get_decision(saved["decisionId"].as_str().unwrap(), &c)
        .unwrap();
    assert_eq!(
        recorded["revisions"][0]["context"], displayed,
        "the comparison keeps the budget shown at confirmation"
    );
}

#[tokio::test]
async fn cash_flow_uses_each_source_business_days_exchange_rate() {
    let h = Harness::new().await;
    let day = chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap();
    let earlier = (day - chrono::Duration::days(2)).to_string();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('cash','Cash','CASH','EUR',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS'); INSERT INTO assets(id,kind,name,display_code,is_active,quote_mode,quote_ccy,instrument_type,instrument_symbol,created_at,updated_at) VALUES('fx-eur-cny','FX','EUR/CNY','EUR/CNY',1,'MANUAL','CNY','FX','EUR',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP);").unwrap();
    seed_quote(&h, "fx-eur-cny", "2", "EUR", &earlier);
    seed_quote(&h, "fx-eur-cny", "3", "EUR", &h.day);
    for (id, date) in [("earlier", &earlier), ("today", &h.day)] {
        db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES(?1,'cash','DEPOSIT','POSTED',?2,'EUR','10',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,format!("{date}T12:00:00+08:00"))).unwrap();
    }
    let context = native(&h, query(&[], &[])).await;
    assert_eq!(
        context.response.recent_cash_flow.data["ordinary"]["currencies"][0]["income"],
        "20"
    );
    assert_eq!(
        context.response.recent_cash_flow.data["convertedOrdinary"]["income"],
        "50"
    );
    assert_eq!(
        context.response.recent_cash_flow.data["convertedOrdinary"]["rateDateBasis"],
        "ACTIVITY_BUSINESS_DAY"
    );
}

#[tokio::test]
async fn source_change_and_restore_during_native_reads_retries_the_entire_capture() {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    use wealthfolio_core::portfolio::net_worth::{
        DetailedNetWorthResponse, NetWorthHistoryPoint, NetWorthResponse, NetWorthServiceTrait,
    };
    struct ChangeAndRestore {
        inner: Arc<dyn NetWorthServiceTrait>,
        path: String,
        once: AtomicBool,
        calls: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl NetWorthServiceTrait for ChangeAndRestore {
        async fn get_net_worth(
            &self,
            date: chrono::NaiveDate,
        ) -> wealthfolio_core::Result<NetWorthResponse> {
            self.inner.get_net_worth(date).await
        }
        async fn get_net_worth_detailed(
            &self,
            date: chrono::NaiveDate,
        ) -> wealthfolio_core::Result<DetailedNetWorthResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let change = self.once.swap(false, Ordering::SeqCst);
            let access = wealthfolio_storage_sqlite::db::DbAccess::plaintext(&self.path);
            let db = access.connect_rusqlite().unwrap();
            if change {
                db.execute(
                    "UPDATE quotes SET close='999' WHERE asset_id='epoch-home'",
                    [],
                )
                .unwrap();
            }
            let result = self.inner.get_net_worth_detailed(date).await;
            if change {
                db.execute(
                    "UPDATE quotes SET close='1000' WHERE asset_id='epoch-home'",
                    [],
                )
                .unwrap();
            }
            result
        }
        fn get_net_worth_history(
            &self,
            from: chrono::NaiveDate,
            to: chrono::NaiveDate,
        ) -> wealthfolio_core::Result<Vec<NetWorthHistoryPoint>> {
            self.inner.get_net_worth_history(from, to)
        }
    }
    let h = Harness::new().await;
    h.initialize_manual().await;
    seed_transaction_snapshot(&h);
    h.access.connect_rusqlite().unwrap().execute_batch("INSERT INTO assets(id,kind,name,display_code,is_active,quote_mode,quote_ccy,created_at,updated_at) VALUES('epoch-home','PROPERTY','Home','HOME',1,'MANUAL','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP);").unwrap();
    seed_quote(&h, "epoch-home", "1000", "CNY", &h.day);
    let source = Arc::new(ChangeAndRestore {
        inner: h.net_worth.clone(),
        path: h.access.path().into(),
        once: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    });
    let reader = Arc::new(h.context.as_ref().clone().with_net_worth(source.clone()));
    let captured = h
        .journal()
        .with_context_reader(reader)
        .get_investment_context(query(&[], &[]), &journal_caller())
        .await
        .unwrap();
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        2,
        "content returning to its old value cannot hide a mixed source read"
    );
    assert_eq!(captured.response.household.data["totalAssets"], "3000");
}

#[tokio::test]
async fn purpose_risk_waits_for_changed_security_units_even_without_excess() {
    let h = Harness::new().await;
    h.seed_purpose("purpose");
    let db = h.access.connect_rusqlite().unwrap();
    let day: chrono::NaiveDate = h.day.parse().unwrap();
    let trade_day = (day - chrono::Duration::days(2)).to_string();
    let split_day = (day - chrono::Duration::days(1)).to_string();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('buy','transactions','stock','BUY','POSTED',?1,'10','10','100','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[trade_day]).unwrap();
    seed_quote(&h, "stock", "10", "CNY", &h.day);
    db.execute("UPDATE fund_purposes SET minimum_cash='100',single_asset_limit_bps=5000 WHERE id='purpose'", []).unwrap();
    let security = FundResource::Security {
        account_id: "transactions".into(),
        asset_id: "stock".into(),
    };
    let cash = FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    };
    assign(
        &h,
        vec![delta(security.clone(), "5"), delta(cash, "200")],
        "seed-unit-risk",
    )
    .await;
    let initial = native(&h, query(&["purpose"], &["transactions"])).await;
    assert_eq!(
        initial.response.funding.data["resourceReferences"]["accounts"][0]["name"],
        "Transactions"
    );
    assert_eq!(
        initial.response.funding.data["resourceReferences"]["assets"][0]["name"],
        "Stock"
    );

    assert_eq!(
        initial.response.funding.data["risks"][0]["totalValue"],
        "250"
    );
    assert_eq!(
        initial.response.funding.data["risks"][0]["singleAssetLimitStatus"],
        "WITHIN_LIMIT"
    );

    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,currency,created_at,updated_at) VALUES('split','transactions','stock','SPLIT','POSTED',?1,'2','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[split_day]).unwrap();
    db.execute("UPDATE quotes SET close='5' WHERE asset_id='stock'", [])
        .unwrap();
    let service = h.journal();
    let caller = journal_caller();
    let observed = service
        .preview_allocations(vec![security.clone()], &caller)
        .await
        .unwrap();
    assert_eq!(
        observed.resources[0].balance.actual,
        Some(ExactDecimal::parse("20").unwrap())
    );
    assert_eq!(
        observed.resources[0].balance.excess,
        Some(ExactDecimal::ZERO)
    );
    let pending = native(&h, query(&["purpose"], &["transactions"])).await;
    let risk = &pending.response.funding.data["risks"][0];
    assert_eq!(risk["assets"][0]["value"], Value::Null);
    assert_eq!(risk["totalValue"], Value::Null);
    assert_eq!(risk["singleAssetLimitStatus"], "UNABLE_TO_DETERMINE");
    assert_eq!(risk["cashValue"], "200");
    assert_eq!(risk["minimumCashStatus"], "AT_OR_ABOVE_MINIMUM");
    assert!(pending.response.funding.data["reconciliation"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "CORPORATE_ACTION"));

    service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "Explicitly verify the split and update owned units".into(),
                entries: vec![delta(security.clone(), "5")],
                resources: vec![security],
                expected_preview_hash: observed.preview_hash,
                request_key: "acknowledge-unit-risk".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    let verified = native(&h, query(&["purpose"], &["transactions"])).await;
    assert_eq!(
        verified.response.funding.data["risks"][0]["assets"][0]["value"],
        "50"
    );
    assert_eq!(
        verified.response.funding.data["risks"][0]["singleAssetLimitStatus"],
        "WITHIN_LIMIT"
    );

    for multiplier in ["unverified", "2"] {
        let metadata = json!({"contractMultiplier":multiplier}).to_string();
        db.execute("UPDATE assets SET metadata=?1 WHERE id='stock'", [metadata])
            .unwrap();
        let unsupported = native(&h, query(&["purpose"], &["transactions"])).await;
        assert_eq!(
            unsupported.response.funding.data["risks"][0]["assets"][0]["value"],
            Value::Null
        );
        assert_eq!(
            unsupported.response.funding.data["risks"][0]["singleAssetLimitStatus"],
            "UNABLE_TO_DETERMINE"
        );
    }
}
