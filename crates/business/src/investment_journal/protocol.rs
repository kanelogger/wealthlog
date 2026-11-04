//! One typed command boundary for desktop GUI, CLI and desktop MCP.
//! Transport authentication is supplied separately and is never parsed from args.
use super::*;
use crate::investments::{CreateCashTransfer, RecordManualTrade};
use schemars::{generate::SchemaSettings, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalCommandAccess {
    Read,
    Write,
    Confirm,
    ManualTradeWrite,
    TransferWrite,
    GuiTradeWrite,
    GuiPreferenceWrite,
}

impl InvestmentJournalCommand {
    /// This is also the CLI discovery contract: it keeps command schemas and
    /// write/replay classification on the same authoritative business boundary.
    pub fn external_catalog() -> Value {
        serde_json::json!(Self::ALL.iter().filter(|command|command.external_available()).map(|command|{
            let access=match command.access(){JournalCommandAccess::Read=>"read",JournalCommandAccess::Confirm=>"confirm",_=>"write"};
            let request_key_pointer=match command.access(){JournalCommandAccess::Read=>None,JournalCommandAccess::ManualTradeWrite=>Some("/trade/requestKey"),JournalCommandAccess::TransferWrite=>Some("/transfer/requestKey"),_=>Some("/requestKey")};
            serde_json::json!({"name":command.name(),"description":command.description(),"access":access,"requestKeyPointer":request_key_pointer,"inputSchema":command.input_schema()})
        }).collect::<Vec<_>>())
    }
}

macro_rules! args {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
        #[serde(rename_all="camelCase",deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
    };
}
args!(JournalEmptyArgs {});
args!(JournalResourcesArgs {resources:Vec<FundResource>});
args!(JournalAccountStateArgs { account_id: String });
args!(JournalDecisionArgs {
    decision_id: String
});
args!(JournalRevisionArgs {
    decision_revision_id: String
});
args!(JournalReviewArgs { review_id: String });
args!(JournalPrincipleArgs {
    principle_id: String
});
args!(JournalOperationArgs {
    operation_id: String
});
args!(JournalCorrectionPreviewArgs {operation_id:String,entries:Vec<AllocationDelta>});
args!(JournalManualTradePreviewArgs {trade:RecordManualTrade,shares:Vec<TradePurposeShare>});
args!(JournalLedgerTradePreviewArgs {trade:RecordLedgerTrade,shares:Vec<TradePurposeShare>});
args!(JournalNewTransferPreviewArgs {transfer:CreateCashTransfer,shares:Vec<TransferPurposeShare>});
args!(JournalExistingTransferPreviewArgs {activity_id:String,shares:Vec<TransferPurposeShare>});
args!(JournalDecisionManualPreviewArgs {
    decision: DecisionExecutionTarget,
    trade: RecordManualTrade,
    ownership: DecisionTradeOwnership
});
args!(JournalDecisionLedgerPreviewArgs {
    decision: DecisionExecutionTarget,
    trade: RecordLedgerTrade,
    ownership: DecisionTradeOwnership
});

fn schema<T: JsonSchema>() -> Value {
    SchemaSettings::default()
        .for_deserialize()
        .with(|s| s.meta_schema = None)
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value()
}

macro_rules! commands {
    ($( $variant:ident($input:ty) => ($name:literal,$access:ident,$description:literal),
        |$service:ident,$args:ident,$caller:ident| $body:expr; )*) => {
        #[derive(Debug,Clone,Copy,PartialEq,Eq,Deserialize,Serialize,JsonSchema)]
        pub enum InvestmentJournalCommand { $(#[serde(rename=$name)] $variant),* }
        impl InvestmentJournalCommand {
            pub const ALL:&'static [Self]=&[$(Self::$variant),*];
            pub const fn name(self)->&'static str { match self { $(Self::$variant=>$name),* } }
            pub const fn description(self)->&'static str { match self { $(Self::$variant=>$description),* } }
            pub const fn access(self)->JournalCommandAccess { match self { $(Self::$variant=>JournalCommandAccess::$access),* } }
            pub const fn external_available(self)->bool { !matches!(self.access(),JournalCommandAccess::GuiTradeWrite|JournalCommandAccess::GuiPreferenceWrite) }
            pub fn input_schema(self)->Value { match self { $(Self::$variant=>schema::<$input>()),* } }
            pub async fn execute(self,service:&InvestmentJournalService,value:Value,caller:&JournalCaller)->JournalResult<Value> {
                match self.access() {
                    JournalCommandAccess::Read=>caller.require_read()?,
                    JournalCommandAccess::Confirm=>caller.require_confirm()?,
                    _=>caller.require_write()?,
                }
                service.require_ready()?;
                match self { $(Self::$variant=>{
                    let $service=service;let $caller=caller;
                    let $args:$input=serde_json::from_value(value).map_err(|e|model::invalid(e.to_string()))?;
                    let result=$body;
                    serde_json::to_value(result).map_err(|e|model::invalid(e.to_string()))
                }),* }
            }
        }
    };
}

commands! {
    Status(JournalEmptyArgs)=>("get_investment_journal_status",Read,"Read current ledger binding, module readiness, separate onboarding/business enable markers and indexing completeness. Reads never enable the module."),
        |s,_a,c| s.investment_journal_status(c)?;
    Onboarding(CompleteInvestmentOnboarding)=>("complete_investment_onboarding",GuiPreferenceWrite,"Desktop introduction preference only: record save/skip independently of business enablement. No resource allocation and no default queue enablement."),
        |s,a,c| s.complete_investment_onboarding(a,c).await?;
    FundHistory(FundHistoryQuery)=>("list_fund_history",Read,"Read append-only ownership operations, exact per-purpose entries, source confirmations and trusted recording identity with pagination. Zero-entry receipts remain in unfiltered history."),
        |s,a,c| s.fund_history(a,c)?;
    Commands(JournalEmptyArgs)=>("get_investment_commands",Read,"Read the desktop-only investment command catalog, exact input schemas, access classifications and requestKey paths. Does not grant permission to execute a listed write or confirmation."),
        |_s,_a,_c| Self::external_catalog();
    Context(InvestmentContextQuery)=>("get_investment_context",Read,"Read funding, full recent ordinary cash flow and household context with explicit scopes, source dates and quality; missing values stay unavailable."),
        |s,a,c| s.get_investment_context(a,c).await?;
    AccountState(JournalAccountStateArgs)=>("get_investment_journal_account_state",Read,"Read the native account snapshot, exact per-currency cash and stateVersion for a manual trade preview. Reads never initialize or enable the module."),
        |s,a,c| s.investment_journal_account_state(&a.account_id,c).await?;
    DecisionContext(DecisionBody)=>("preview_investment_decision_context",Read,"Read the native context and simulations for partial structured thinking without saving, enabling, confirming or allocating. Invalid/incomplete resource or scenario inputs cannot become a risk pass."),
        |s,a,c| s.preview_decision_context(a,c).await?;
    Purposes(JournalEmptyArgs)=>("list_fund_purposes",Read,"List purpose definitions. Purposes describe ownership; they never change actual account cash or securities."),
        |s,_a,c| s.list_purposes(c)?;
    SavePurpose(SaveFundPurpose)=>("save_fund_purpose",Write,"Create or update a purpose using its current version. Exact decimal strings; does not allocate resources or change ledger facts."),
        |s,a,c| s.save_purpose(a,c).await?;
    Allocations(JournalResourcesArgs)=>("preview_fund_allocations",Read,"Observe exact native resources and all purpose claims, unallocated capacity, excess and liabilities. Never infer actual balances from purpose ownership."),
        |s,a,c| s.preview_allocations(a.resources,c).await?;
    ApplyAllocation(ApplyFundAllocation)=>("apply_fund_allocation",Write,"Apply explicit ownership deltas against the exact preview hash. Zero-entry reconciliation confirms only named current sources; remaining excess cannot be dismissed."),
        |s,a,c| s.apply_allocation(a,c).await?;
    Reconciliation(JournalResourcesArgs)=>("get_fund_reconciliation",Read,"Read numeric excess, unavailable resources and original source changes. Acknowledgements cannot hide continuing numeric problems."),
        |s,a,c| s.fund_reconciliation(a.resources,c).await?;
    ReversePreview(JournalOperationArgs)=>("preview_fund_reversal",Read,"Preview reversal of an ownership operation. Original facts and history remain intact; each operation can be reversed once."),
        |s,a,c| s.preview_fund_reversal(&a.operation_id,c).await?;
    Reverse(ReverseFundOperation)=>("reverse_fund_operation",Write,"Append the exact previewed reversal with a reason and requestKey. Reverses ownership only, never a bank payment or brokerage trade."),
        |s,a,c| s.reverse_fund_operation(a,c).await?;
    CorrectionPreview(JournalCorrectionPreviewArgs)=>("preview_fund_correction",Read,"Preview explicit ownership correction entries against current actual resources and source versions."),
        |s,a,c| s.preview_fund_correction(&a.operation_id,a.entries,c).await?;
    Correct(CorrectFundOperation)=>("correct_fund_operation",Write,"Append the previewed ownership correction and reason; retain the original operation, facts and source history."),
        |s,a,c| s.correct_fund_operation(a,c).await?;
    ManualPreview(JournalManualTradePreviewArgs)=>("preview_manual_trade_ownership",Read,"Preview an already-executed trade in a manual HOLDINGS account and explicit purpose shares/topups. Original preparation is read-only; omitted fees remain unknown."),
        |s,a,c| s.preview_manual_trade_ownership(&a.trade,a.shares,c).await?;
    ManualRecord(RecordManualTradeWithOwnership)=>("record_manual_trade_with_ownership",ManualTradeWrite,"Atomically record an already-executed manual-account trade, execution index and purpose ownership. Requires holdings:write plus investments:write. Does not place an order; freeze and reuse requestKey after an unknown outcome."),
        |s,a,c| s.record_manual_trade_with_ownership(a,c).await?;
    LedgerPreview(JournalLedgerTradePreviewArgs)=>("preview_ledger_trade_ownership",Read,"Read-only native-kernel preview for a proposed transaction-account trade and ownership conversion. Program callers cannot create this ledger trade; use manual holdings recording or link existing ledger facts."),
        |s,a,c| s.preview_ledger_trade_ownership(&a.trade,a.shares,c).await?;
    LedgerRecord(RecordLedgerTradeWithOwnership)=>("record_ledger_trade_with_ownership",GuiTradeWrite,"Unlocked desktop GUI only: atomically record a transaction-account trade and ownership. Not available to CLI or MCP."),
        |s,a,c| s.record_ledger_trade_with_ownership(a,c).await?;
    NewTransferPreview(JournalNewTransferPreviewArgs)=>("preview_cash_transfer_ownership",Read,"Read-only preview of an already-completed internal cash transfer and purpose continuation. Keep both original currencies and exact recorded amounts."),
        |s,a,c| s.preview_new_cash_transfer_ownership(&a.transfer,a.shares,c).await?;
    NewTransfer(CreateCashTransferWithOwnership)=>("create_cash_transfer_with_ownership",TransferWrite,"Atomically record both posted legs of an already-completed internal transfer and purpose continuation. Requires transfers:write plus investments:write. Does not send a bank payment; reuse the frozen requestKey."),
        |s,a,c| s.create_cash_transfer_with_ownership(a,c).await?;
    ExistingTransferPreview(JournalExistingTransferPreviewArgs)=>("preview_existing_transfer_ownership",Read,"Preview purpose continuation for an already-recorded original cash-transfer pair. Partial allocation uses native recorded precision and stable apportionment."),
        |s,a,c| s.preview_existing_transfer_ownership(&a.activity_id,a.shares,c).await?;
    ExistingTransfer(ApplyExistingTransferOwnership)=>("apply_existing_transfer_ownership",Write,"Apply previewed purpose continuation once for both original transfer legs. No new facts are created and original currencies are retained."),
        |s,a,c| s.apply_existing_transfer_ownership(a,c).await?;
    Decisions(DecisionQuery)=>("list_investment_decisions",Read,"List decisions with exact scenario-tag intersection, current confirmed version and separate draft pointer. A draft never replaces the effective version."),
        |s,a,c| s.list_decisions(a,c)?;
    Decision(JournalDecisionArgs)=>("get_investment_decision",Read,"Read all immutable confirmed versions, the editable draft, original context quality, declared content origin and trusted recording/confirming identities."),
        |s,a,c| s.get_decision(&a.decision_id,c)?;
    SaveDecision(SaveDecisionDraft)=>("save_investment_decision_draft",Write,"Save a draft with CAS version, structured unknowns, unchosen alternatives and optional published principle versions. Ordinary saves KEEP check schedules. Cannot confirm or invent a user's thesis, evidence or probabilities."),
        |s,a,c| s.save_decision_draft(a,c).await?;
    ConfirmDecisionPreview(DecisionDraftTarget)=>("preview_investment_decision_confirmation",Read,"Show the exact draft/version, content hash and actual native context to be frozen. Missing context remains explicitly unavailable; preview never confirms."),
        |s,a,c| s.preview_decision_confirmation(a,c).await?;
    ConfirmDecision(ConfirmDecisionDraft)=>("confirm_investment_decision",Confirm,"Confirm the exact displayed version and context with ledger binding and investments:confirm. Cannot replace text during confirmation. Program confirmation is recorded as PROGRAM; source changes require a new preview."),
        |s,a,c| s.confirm_decision_draft(a,c).await?;
    MutateDecision(MutateDecision)=>("update_investment_decision",Write,"Explicitly archive/restore, discard a draft, close/reopen execution with a reason, or set/cancel review reminders. Never rewrites confirmed contents or closes because planned quantity was reached."),
        |s,a,c| s.mutate_decision(a,c).await?;
    DraftChecks(JournalEmptyArgs)=>("list_investment_draft_checks",Read,"Read unique checks for the current draft, including an amendment of a confirmed decision. These dates are separate from confirmed review reminders."),
        |s,_a,c| s.decision_draft_checks(c)?;
    ScenarioTags(JournalEmptyArgs)=>("list_investment_scenario_tags",Read,"Read historical/current decision scenario categories. Tags describe cases and do not imply principle causation or a transaction."),
        |s,_a,c| s.decision_scenario_tags(c)?;
    LinkPreview(ExecutionLinkTarget)=>("preview_investment_execution_link",Read,"Preview an existing execution's exact confirmed decision version and temporal classification. Date-only same-day ordering remains undetermined; no ownership conversion occurs."),
        |s,a,c| s.preview_execution_link(a,c)?;
    Link(LinkExistingExecution)=>("link_investment_execution",Write,"Associate an already-recorded execution with one confirmed version using the exact preview hash. Evidence only: no new trade, quantity or purpose conversion."),
        |s,a,c| s.link_existing_execution(a,c).await?;
    Unlink(UnlinkExecution)=>("unlink_investment_execution",Write,"Remove an evidence association with a reason while retaining its original snapshot/history. Actual cash, securities and ownership remain unchanged."),
        |s,a,c| s.unlink_execution(a,c).await?;
    Links(JournalDecisionArgs)=>("list_investment_execution_links",Read,"Read original and unlinked execution associations across all confirmed versions of one decision, preserving exact source versions and system confirmation timing."),
        |s,a,c| s.decision_execution_links(&a.decision_id,c)?;
    ExecutionProgress(JournalRevisionArgs)=>("get_investment_execution_progress",Read,"Read current linked progress for one confirmed version. Quantity remains unavailable when original units cannot be verified across corporate actions; never closes execution automatically."),
        |s,a,c| s.decision_execution_progress(&a.decision_revision_id,c)?;
    DecisionManualPreview(JournalDecisionManualPreviewArgs)=>("preview_decision_manual_trade",Read,"Preview an already-executed manual-account trade bound to the exact current confirmed version. Explicit ownership application or fact-only/link with reconciliation later; a separate draft does not change vN."),
        |s,a,c| s.preview_decision_manual_trade(&RecordDecisionManualTrade{decision:a.decision,trade:a.trade,ownership:a.ownership,reason:String::new(),expected_preview_hash:String::new()},c).await?;
    DecisionManualRecord(RecordDecisionManualTrade)=>("record_decision_manual_trade",ManualTradeWrite,"Atomically record manual-account facts, execution, exact current confirmed-version evidence and optional purpose ownership. Requires original holdings write permission. No implicit decision confirmation; deferred ownership remains a visible reconciliation task."),
        |s,a,c| s.record_decision_manual_trade(a,c).await?;
    DecisionLedgerPreview(JournalDecisionLedgerPreviewArgs)=>("preview_decision_ledger_trade",Read,"Read-only preview of native transaction-account facts bound to the current confirmed decision. Creating the transaction-account trade is GUI-only; CLI/MCP may link existing facts."),
        |s,a,c| s.preview_decision_ledger_trade(&RecordDecisionLedgerTrade{decision:a.decision,trade:a.trade,ownership:a.ownership,reason:String::new(),expected_preview_hash:String::new()},c).await?;
    DecisionLedgerRecord(RecordDecisionLedgerTrade)=>("record_decision_ledger_trade",GuiTradeWrite,"Unlocked desktop GUI only: atomic transaction-account fact, confirmed-version link and optional purpose ownership. Not available to CLI/MCP."),
        |s,a,c| s.record_decision_ledger_trade(a,c).await?;
    ExecutionReconciliationPreview(ExecutionReconciliationDraft)=>("preview_investment_execution_reconciliation",Read,"Preview current source changes and explicit ownership adjustments or NO_OWNERSHIP_CHANGE. Does not acknowledge a different source version or erase remaining excess."),
        |s,a,c| s.preview_execution_reconciliation(&a,c).await?;
    ExecutionReconcile(ReconcileExecution)=>("reconcile_investment_execution",Write,"Append a reasoned receipt for the exact source/preview and optional ownership deltas. Later source changes reopen the task; facts and original links remain intact."),
        |s,a,c| s.reconcile_execution(a,c).await?;
    WorkQueue(InvestmentWorkQuery)=>("get_investment_work_queue",Read,"Read purpose reconciliation, unlinked executions, source changes, ownership pending, draft checks and review reminders. Default date boundary uses original enable timezone; explicit historical reads do not enable or allocate. complete=false during indexing."),
        |s,a,c| s.investment_work_queue(a,c).await?;
    Reviews(JournalRevisionArgs)=>("list_investment_reviews",Read,"Read all review roots and fixed-cutoff corrections for one exact confirmed decision version, including drafts and trusted submitter identity."),
        |s,a,c| s.decision_reviews(&a.decision_revision_id,c)?;
    Review(JournalReviewArgs)=>("get_investment_review",Read,"Read the exact review body, fixed observation cutoff/timezone, original frozen decision context, observed facts and separately declared explanations."),
        |s,a,c| s.get_review(&a.review_id,c)?;
    SaveReview(SaveReviewDraft)=>("save_investment_review_draft",Write,"Save a CAS review draft. Keep judgment, execution, deviations, outcome evidence, other explanations and open questions separate. A new observation cutoff requires a new root; same-cutoff corrections append without forking."),
        |s,a,c| s.save_review_draft(a,c).await?;
    DiscardReview(DiscardReviewDraft)=>("discard_investment_review_draft",Write,"Discard the exact editable review draft while retaining its version identity and all submitted history."),
        |s,a,c| s.discard_review_draft(a,c).await?;
    SubmitReviewPreview(ReviewDraftTarget)=>("preview_investment_review_submission",Read,"Read the exact review draft and original confirmed context plus cutoff-scoped native facts and quality. Preview hashes bind the actual displayed content; unavailable evidence never becomes zero."),
        |s,a,c| s.preview_review_submission(a,c).await?;
    SubmitReview(SubmitReview)=>("submit_investment_review",Confirm,"Submit only the displayed review and facts with ledger binding and investments:confirm. Does not replace text or publish principles. Old-version/early-cutoff observations do not clear current review reminders."),
        |s,a,c| s.submit_review(a,c).await?;
    Principles(PrincipleQuery)=>("list_investment_principles",Read,"List principle definitions, effective and separate draft pointers using exact scenario-tag intersection. New drafts do not change effective rules."),
        |s,a,c| s.list_principles(a,c)?;
    Principle(JournalPrincipleArgs)=>("get_investment_principle",Read,"Read all principle versions, immutable published contents and exact source-review snapshots. A source captured as DRAFT remains labelled DRAFT; its status is not verification."),
        |s,a,c| s.get_principle(&a.principle_id,c)?;
    SavePrinciple(SavePrincipleDraft)=>("save_investment_principle_draft",Write,"Append a principle draft with explicit source references, applicability, exceptions and scenario categories. A source review may remain a draft; this never submits it or publishes the rule."),
        |s,a,c| s.save_principle_draft(a,c).await?;
    DiscardPrinciple(DiscardPrincipleDraft)=>("discard_investment_principle_draft",Write,"Discard the exact current principle draft; retain previous effective versions and immutable referenced proposal contents."),
        |s,a,c| s.discard_principle_draft(a,c).await?;
    PublishPrinciplePreview(PrincipleDraftTarget)=>("preview_investment_principle_publication",Read,"Show the exact principle draft and actual source-review contents/status to be frozen. Publication of an amendment requires its revision reason."),
        |s,a,c| s.preview_principle_publication(a,c)?;
    PublishPrinciple(PublishPrinciple)=>("publish_investment_principle",Confirm,"Publish only the displayed principle/source snapshot with ledger binding and investments:confirm. Keeps prior published versions and records trusted program/GUI identity; never implicitly submits a source review."),
        |s,a,c| s.publish_principle(a,c).await?;
    ArchivePrinciple(ArchivePrinciple)=>("archive_investment_principle",Write,"Explicitly archive or restore a principle definition using its current root version. Historical decision/review references stay on their exact versions."),
        |s,a,c| s.archive_principle(a,c).await?;
    Cases(InvestmentCaseQuery)=>("search_investment_cases",Read,"Search confirmed decision/submitted review versions by exact intersection of scenario categories, independently of explicit principle references. Supports pagination and historical revisions; matching tags never create causal links."),
        |s,a,c| s.investment_cases(a,c)?;
}
