//! Native synthetic-ledger coverage through the actual desktop tool catalog.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "../../business/tests/support/investment_harness.rs"]
mod support;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use support::Harness;
use wealthfolio_agent_tools::*;
use wealthfolio_business::investment_journal::*;

#[derive(Default)]
struct Env {
    journal: Option<Arc<InvestmentJournalService>>,
    accesses: AtomicUsize,
}
macro_rules! unused_service {
    ($name:ident,$ty:path) => {
        fn $name(&self) -> Arc<dyn $ty> {
            panic!("unexpected service access: {}", stringify!($name))
        }
    };
}
impl AgentEnvironment for Env {
    fn base_currency(&self) -> String {
        "CNY".into()
    }
    unused_service!(
        account_service,
        wealthfolio_core::accounts::AccountServiceTrait
    );
    unused_service!(
        activity_service,
        wealthfolio_core::activities::ActivityServiceTrait
    );
    unused_service!(
        holdings_service,
        wealthfolio_core::portfolio::holdings::HoldingsServiceTrait
    );
    unused_service!(
        valuation_service,
        wealthfolio_core::portfolio::valuation::ValuationServiceTrait
    );
    unused_service!(
        settings_service,
        wealthfolio_core::settings::SettingsServiceTrait
    );
    unused_service!(quote_service, wealthfolio_core::quotes::QuoteServiceTrait);
    unused_service!(asset_service, wealthfolio_core::assets::AssetServiceTrait);
    unused_service!(
        allocation_service,
        wealthfolio_core::portfolio::allocation::AllocationServiceTrait
    );
    unused_service!(
        performance_service,
        wealthfolio_core::portfolio::performance::PerformanceServiceTrait
    );
    unused_service!(
        income_service,
        wealthfolio_core::portfolio::income::IncomeServiceTrait
    );
    unused_service!(health_service, wealthfolio_core::health::HealthServiceTrait);
    unused_service!(
        taxonomy_service,
        wealthfolio_core::taxonomies::TaxonomyServiceTrait
    );
    unused_service!(
        portfolio_service,
        wealthfolio_core::portfolios::PortfolioServiceTrait
    );
    unused_service!(
        net_worth_service,
        wealthfolio_core::portfolio::net_worth::NetWorthServiceTrait
    );
    unused_service!(
        cash_activity_service,
        wealthfolio_spending::cash_activities::CashActivityServiceTrait
    );
    unused_service!(
        categorization_rules_service,
        wealthfolio_spending::categorization_rules::CategorizationRulesServiceTrait
    );
    fn investment_journal_service(&self) -> Option<Arc<InvestmentJournalService>> {
        self.accesses.fetch_add(1, Ordering::SeqCst);
        self.journal.clone()
    }
}
fn context(omit: Option<AgentScope>) -> ToolCallContext {
    let scopes = AgentScopeSet::from_strs(
        AgentScope::ALL
            .iter()
            .filter(|scope| Some(**scope) != omit)
            .map(|scope| scope.as_str()),
    );
    ToolCallContext::pat(
        "synthetic-program",
        Some("synthetic-credential".into()),
        scopes,
    )
}
fn environment(h: &Harness) -> Arc<Env> {
    Arc::new(Env {
        journal: Some(Arc::new(
            h.journal()
                .with_context_reader(h.context.clone())
                .with_review_facts_reader(h.context.clone()),
        )),
        ..Default::default()
    })
}
async fn call(env: &Arc<Env>, context: &ToolCallContext, name: &str, args: Value) -> Value {
    AgentToolCatalog::mcp_catalog_for(McpHost::Desktop)
        .execute_with_context(env.clone(), context, name, args)
        .await
        .unwrap()
        .content
}
fn schema_ref<'a>(root: &'a Value, value: &'a Value) -> &'a Value {
    if let Some(reference) = value["$ref"].as_str() {
        root.pointer(reference.strip_prefix('#').unwrap()).unwrap()
    } else {
        value
    }
}

#[tokio::test]
async fn every_journal_tool_is_desktop_only_and_checks_every_scope_before_services() {
    let desktop = AgentToolCatalog::mcp_catalog_for(McpHost::Desktop);
    let server = AgentToolCatalog::mcp_catalog_for(McpHost::Server);
    let env = Arc::new(Env::default());
    let mut names = std::collections::BTreeSet::new();
    let frontend = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/frontend/src/adapters/investment-journal-commands.generated.ts"
    ))
    .unwrap();
    assert_eq!(
        frontend
            .lines()
            .filter(|line| line.starts_with("  \""))
            .count(),
        InvestmentJournalCommand::ALL.len()
    );
    for command in InvestmentJournalCommand::ALL {
        assert!(
            frontend.contains(&format!("  \"{}\",", command.name())),
            "frontend command types are stale: {}",
            command.name()
        );
        assert!(names.insert(command.name()));
        assert!(server.get(command.name()).is_none());
        if !command.external_available() {
            assert!(desktop.get(command.name()).is_none());
            continue;
        }
        let tool = desktop.get(command.name()).unwrap();
        assert!(mcp_write_blocked_for(tool.as_ref(), McpHost::Server));
        assert!(!mcp_write_blocked_for(tool.as_ref(), McpHost::Desktop));
        assert_eq!(
            tool.access_level() == AgentToolAccess::Write,
            command.access() != JournalCommandAccess::Read
        );
        for scope in tool.required_scopes() {
            let error = desktop
                .execute_with_context(env.clone(), &context(Some(*scope)), tool.name(), json!({}))
                .await
                .unwrap_err();
            assert!(
                matches!(error, AgentToolError::ScopeDenied { .. }),
                "{} missing {scope}: {error}",
                tool.name()
            );
        }
        assert_eq!(
            tool.call(env.clone(), json!({}))
                .await
                .unwrap_err()
                .structured_value()["code"],
            "scope_denied"
        );
    }
    assert_eq!(env.accesses.load(Ordering::SeqCst), 0);
    assert!(!McpHost::Desktop.permits_formal_write("record_unknown_investment"));
    let read = desktop
        .get("get_investment_context")
        .unwrap()
        .input_schema();
    assert_eq!(read["additionalProperties"], false);
    let allocation = desktop.get("apply_fund_allocation").unwrap().input_schema();
    let entry = schema_ref(&allocation, &allocation["properties"]["entries"]["items"]);
    let amount = schema_ref(&allocation, &entry["properties"]["delta"]);
    assert_eq!(amount["type"], "string");
    let preview = desktop
        .get("preview_decision_manual_trade")
        .unwrap()
        .input_schema();
    assert!(preview["properties"].get("expectedPreviewHash").is_none());
    assert!(preview["properties"].get("reason").is_none());
}

#[tokio::test]
async fn native_program_flow_preserves_confirmation_boundaries_and_original_source_snapshots() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    let env = environment(&h);
    let c = context(None);
    let catalog = AgentToolCatalog::mcp_catalog_for(McpHost::Desktop);
    let definitions = call(&env, &c, "get_investment_commands", json!({})).await;
    assert_eq!(
        definitions.as_array().unwrap().len(),
        InvestmentJournalCommand::ALL
            .iter()
            .filter(|command| command.external_available())
            .count()
    );
    assert!(!definitions
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["name"] == "record_decision_ledger_trade"));
    let d=call(&env,&c,"save_investment_decision_draft",json!({"body":{"title":"程序整理的明确原话","action":"BUY","assetId":"stock","thesis":"用户提供的理由","plannedQuantity":"10","options":[{"action":"等待","description":"继续核实"}],"contentOrigin":"USER_PROVIDED"},"requestKey":"draft"})).await;
    let p=call(&env,&c,"preview_investment_decision_confirmation",json!({"decisionId":d["decisionId"],"draftRevisionId":d["draftRevisionId"],"expectedVersion":1})).await;
    let input = json!({"ledgerId":p["ledgerId"],"target":p["target"],"expectedContentHash":p["expectedContentHash"],"expectedContextHash":p["expectedContextHash"],"context":p["context"],"requestKey":"confirm"});
    assert_eq!(
        catalog
            .execute_with_context(
                env.clone(),
                &context(Some(AgentScope::InvestmentsConfirm)),
                "confirm_investment_decision",
                input.clone()
            )
            .await
            .unwrap_err()
            .structured_value()["code"],
        "scope_denied"
    );
    let confirmed = call(&env, &c, "confirm_investment_decision", input.clone()).await;
    assert_eq!(
        call(&env, &c, "confirm_investment_decision", input).await["replayed"],
        true
    );
    let decision = call(
        &env,
        &c,
        "get_investment_decision",
        json!({"decisionId":d["decisionId"]}),
    )
    .await;
    assert_eq!(decision["revisions"][0]["confirmedActorKind"], "PROGRAM");
    assert_eq!(
        decision["revisions"][0]["confirmedActorId"],
        "synthetic-program"
    );
    let trade = h.trade().await;
    let mut record = json!({"decision":{"decisionId":d["decisionId"],"revisionId":confirmed["currentRevisionId"]},"trade":trade,"ownership":{"mode":"RECONCILE_LATER"}});
    let p = call(&env, &c, "preview_decision_manual_trade", record.clone()).await;
    record["reason"] = json!("已成交，费用和归属按记录核对");
    record["expectedPreviewHash"] = p["previewHash"].clone();
    assert_eq!(
        catalog
            .execute_with_context(
                env.clone(),
                &context(Some(AgentScope::HoldingsWrite)),
                "record_decision_manual_trade",
                record.clone()
            )
            .await
            .unwrap_err()
            .structured_value()["code"],
        "scope_denied"
    );
    let recorded = call(&env, &c, "record_decision_manual_trade", record.clone()).await;
    assert_eq!(recorded["ownershipStatus"], "RECONCILIATION_REQUIRED");
    assert_eq!(
        call(&env, &c, "record_decision_manual_trade", record).await["replayed"],
        true
    );
    let review=call(&env,&c,"save_investment_review_draft",json!({"decisionRevisionId":confirmed["currentRevisionId"],"observationCutoff":h.day,"body":{"judgment":"仍待后续证据","executionAssessment":"仅核实已关联成交","outcomeEvidence":[{"informationKind":"UNKNOWN","content":"暂无足够证据归因"}],"scenarioTags":["核对"]},"requestKey":"review-draft"})).await;
    let principle=call(&env,&c,"save_investment_principle_draft",json!({"body":{"name":"先核实再归因","content":"分别保存事实和解释","sourceReviewId":review["reviewId"]},"requestKey":"principle-draft"})).await;
    let p=call(&env,&c,"preview_investment_principle_publication",json!({"principleId":principle["principleId"],"draftRevisionId":principle["draftRevisionId"],"expectedVersion":1})).await;
    assert_eq!(
        p["sourceContext"]["sourceReview"]["statusAtCapture"],
        "DRAFT"
    );
    let publish = json!({"ledgerId":p["ledgerId"],"target":p["target"],"expectedContentHash":p["expectedContentHash"],"expectedSourceContextHash":p["expectedSourceContextHash"],"sourceContext":p["sourceContext"],"requestKey":"publish"});
    assert_eq!(
        catalog
            .execute_with_context(
                env.clone(),
                &context(Some(AgentScope::InvestmentsConfirm)),
                "publish_investment_principle",
                publish.clone()
            )
            .await
            .unwrap_err()
            .structured_value()["code"],
        "scope_denied"
    );
    let published = call(&env, &c, "publish_investment_principle", publish).await;
    assert_eq!(
        call(
            &env,
            &c,
            "get_investment_review",
            json!({"reviewId":review["reviewId"]})
        )
        .await["status"],
        "DRAFT"
    );
    let p = call(
        &env,
        &c,
        "preview_investment_review_submission",
        json!({"reviewId":review["reviewId"],"expectedVersion":1}),
    )
    .await;
    assert_eq!(
        p["facts"]["observations"]["executions"][0]["currentSource"]["execution"]["quantity"],
        "10"
    );
    let submit = json!({"ledgerId":p["ledgerId"],"target":p["target"],"expectedContentHash":p["expectedContentHash"],"expectedFactsHash":p["expectedFactsHash"],"facts":p["facts"],"requestKey":"review-submit"});
    assert_eq!(
        catalog
            .execute_with_context(
                env.clone(),
                &context(Some(AgentScope::InvestmentsConfirm)),
                "submit_investment_review",
                submit.clone()
            )
            .await
            .unwrap_err()
            .structured_value()["code"],
        "scope_denied"
    );
    call(&env, &c, "submit_investment_review", submit).await;
    let cases = call(
        &env,
        &c,
        "search_investment_cases",
        json!({"principleRevisionId":published["currentRevisionId"]}),
    )
    .await;
    assert_eq!(cases.as_array().unwrap().len(), 1);
    let principle = call(
        &env,
        &c,
        "get_investment_principle",
        json!({"principleId":principle["principleId"]}),
    )
    .await;
    assert_eq!(
        principle["revisions"][0]["sourceContext"]["sourceReview"]["statusAtCapture"],
        "DRAFT"
    );
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM investment_execution_links", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM investment_executions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn unknown_system_fields_and_numeric_money_cannot_mutate_or_enable_the_module() {
    let h = Harness::new().await;
    let env = environment(&h);
    let c = context(None);
    let catalog = AgentToolCatalog::mcp_catalog_for(McpHost::Desktop);
    for (name, args) in [
        (
            "save_investment_decision_draft",
            json!({"body":{},"status":"CONFIRMED","requestKey":"bad-status"}),
        ),
        (
            "save_investment_decision_draft",
            json!({"body":{},"actorKind":"GUI_SESSION","requestKey":"bad-actor"}),
        ),
        (
            "save_fund_purpose",
            json!({"name":"无浮点","category":"CUSTOM","currency":"CNY","targetAmount":9007199254740993_u64,"requestKey":"bad-number"}),
        ),
    ] {
        assert_eq!(
            catalog
                .execute_with_context(env.clone(), &c, name, args)
                .await
                .unwrap_err()
                .structured_value()["code"],
            "invalid_input"
        );
    }
    let mut caller = c.journal_caller();
    caller.fact_permissions.activities_write = true;
    assert_eq!(InvestmentJournalCommand::LedgerRecord.execute(env.journal.as_ref().unwrap(),json!({"trade":{"accountId":"transactions","assetId":"stock","side":"BUY","activityDate":h.day,"quantity":"1","unitPrice":"10","amount":"10","fee":"0","tax":"0","currency":"CNY","requestKey":"disallowed-ledger"},"shares":[],"reason":"must be denied","expectedPreviewHash":"unused"}),&caller).await.unwrap_err().code(),"scope_denied");
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM business_requests", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn onboarding_is_atomic_separate_from_enablement_and_history_keeps_exact_native_values() {
    let h = Harness::new().await;
    let env = environment(&h);
    let c = context(None);
    let service = env.journal.as_ref().unwrap();
    let before = call(&env, &c, "get_investment_journal_status", json!({})).await;
    assert_eq!(before["ready"], true);
    assert!(before["enabledAt"].is_null());
    assert!(before["onboardingCompletedAt"].is_null());
    let mut gui = c.journal_caller();
    gui.channel = wealthfolio_business::BusinessChannel::Gui;
    gui.actor_kind = JournalActorKind::GuiSession;
    gui.credential_id = None;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER fail_onboarding_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'late audit'); END;").unwrap();
    let command = InvestmentJournalCommand::Onboarding;
    assert!(command
        .execute(service, json!({"requestKey":"onboarding"}), &gui)
        .await
        .is_err());
    assert!(
        call(&env, &c, "get_investment_journal_status", json!({})).await["onboardingCompletedAt"]
            .is_null()
    );
    db.execute_batch("DROP TRIGGER fail_onboarding_audit;")
        .unwrap();
    let completed = command
        .execute(service, json!({"requestKey":"onboarding"}), &gui)
        .await
        .unwrap();
    assert_eq!(
        command
            .execute(service, json!({"requestKey":"onboarding"}), &gui)
            .await
            .unwrap()["replayed"],
        true
    );
    let status = call(&env, &c, "get_investment_journal_status", json!({})).await;
    assert_eq!(
        status["onboardingCompletedAt"],
        completed["onboardingCompletedAt"]
    );
    assert!(status["enabledAt"].is_null());
    assert!(
        call(&env, &c, "get_investment_work_queue", json!({})).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let purpose = call(
        &env,
        &c,
        "save_fund_purpose",
        json!({"name":"精确归属","category":"CUSTOM","currency":"CNY","requestKey":"purpose"}),
    )
    .await;
    assert!(
        !call(&env, &c, "get_investment_journal_status", json!({})).await["enabledAt"].is_null()
    );
    assert_eq!(
        call(&env, &c, "get_investment_journal_status", json!({})).await["onboardingCompletedAt"],
        status["onboardingCompletedAt"]
    );
    db.execute(
        "UPDATE activities SET amount='9007199254740993.1234' WHERE id='deposit'",
        [],
    )
    .unwrap();
    let resources = json!([{"kind":"CASH","accountId":"transactions","currency":"CNY"}]);
    let p = call(
        &env,
        &c,
        "preview_fund_allocations",
        json!({"resources":resources}),
    )
    .await;
    let applied=call(&env,&c,"apply_fund_allocation",json!({"operationKind":"ALLOCATE","reason":"按原始余额明确分配","entries":[{"purposeId":purpose["purposeId"],"resource":resources[0],"delta":"9007199254740993.1234"}],"resources":resources,"expectedPreviewHash":p["previewHash"],"requestKey":"allocate"})).await;
    let history = call(
        &env,
        &c,
        "list_fund_history",
        json!({"purposeId":purpose["purposeId"],"limit":1}),
    )
    .await;
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["entries"][0]["delta"], "9007199254740993.1234");
    assert_eq!(history[0]["entries"][0]["resource"]["currency"], "CNY");
    assert_eq!(history[0]["createdActorKind"], "PROGRAM");
    assert_eq!(history[0]["requestId"], applied["requestId"]);
    assert!(call(&env, &c, "list_fund_history", json!({"offset":1}))
        .await
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn incomplete_schema_blocks_new_command_writes_before_any_partial_enablement() {
    let h = Harness::new().await;
    let env = environment(&h);
    let c = context(None);
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute_batch("DROP TABLE investment_review_principles;")
        .unwrap();
    let error=AgentToolCatalog::mcp_catalog_for(McpHost::Desktop).execute_with_context(env.clone(),&c,"save_fund_purpose",json!({"name":"must not write","category":"CUSTOM","currency":"CNY","requestKey":"partial-migration"})).await.unwrap_err();
    assert_eq!(error.structured_value()["code"], "resource_unavailable");
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_purposes", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
