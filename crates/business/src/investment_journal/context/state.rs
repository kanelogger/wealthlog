//! Financial source anchors. Refresh/audit clocks never stand in for source dates.
use super::super::model::invalid;
use super::{InvestmentContextQuery, JournalResult};
use chrono::NaiveDate;
use diesel::{prelude::*, sql_types::Text};
use serde_json::{json, Value};
#[derive(QueryableByName)]
struct Row {
    #[diesel(sql_type=Text)]
    body: String,
}

pub(super) fn source_version(
    conn: &mut SqliteConnection,
    query: &InvestmentContextQuery,
    day: NaiveDate,
    currency: &str,
    timezone: &str,
) -> JournalResult<String> {
    let queries=[
        "SELECT json_object('id',id,'name',name,'type',account_type,'currency',currency,'mode',tracking_mode,'active',is_active,'archived',is_archived,'meta',meta) AS body FROM accounts ORDER BY id",
        "SELECT json_object('id',id,'name',name,'displayCode',display_code,'kind',kind,'currency',quote_ccy,'instrument',instrument_type,'symbol',instrument_symbol,'quoteMode',quote_mode,'providerConfig',provider_config,'metadata',metadata,'active',is_active) AS body FROM assets ORDER BY id",
        "SELECT json_object('id',id,'account',account_id,'asset',asset_id,'kind',activity_type,'override',activity_type_override,'subtype',subtype,'status',status,'date',activity_date,'settlementDate',settlement_date,'quantity',quantity,'amount',amount,'price',unit_price,'currency',currency,'fee',fee,'tax',tax,'fx',fx_rate,'group',source_group_id,'metadata',metadata,'version',version) AS body FROM activities ORDER BY id",
        "SELECT json_object('account',account_id,'day',snapshot_date,'source',source,'currency',currency,'cash',CASE WHEN json_valid(cash_balances) THEN json(cash_balances) ELSE cash_balances END,'legacyPositions',CASE WHEN EXISTS(SELECT 1 FROM snapshot_positions p WHERE p.snapshot_id=s.id) THEN NULL WHEN json_valid(positions) THEN json(positions) ELSE positions END,'cost',cost_basis,'netContribution',net_contribution,'netContributionBase',net_contribution_base,'cashAccount',cash_total_account_currency,'cashBase',cash_total_base_currency) AS body FROM holdings_snapshots s ORDER BY account_id,snapshot_date,source",
        "SELECT json_object('account',s.account_id,'day',s.snapshot_date,'source',s.source,'asset',p.asset_id,'quantity',p.quantity,'currency',p.currency,'cost',p.total_cost_basis,'averageCost',p.average_cost,'multiplier',p.contract_multiplier,'alternative',p.is_alternative,'costBase',p.cost_basis_base,'costAccount',p.cost_basis_account) AS body FROM snapshot_positions p JOIN holdings_snapshots s ON s.id=p.snapshot_id ORDER BY s.account_id,s.snapshot_date,s.source,p.asset_id",
        "SELECT json_object('account',account_id,'day',valuation_date,'currency',account_currency,'base',base_currency,'fx',fx_rate_to_base,'cash',cash_balance,'investment',investment_market_value,'total',total_value,'cashBase',cash_balance_base,'investmentBase',investment_market_value_base,'totalBase',total_value_base,'status',value_status,'basisStatus',basis_status,'quoteInputs',quote_inputs) AS body FROM daily_account_valuation ORDER BY account_id,valuation_date",
        "SELECT json_object('scope',scope,'dirtyFrom',dirty_from) AS body FROM projection_state WHERE dirty_from IS NOT NULL ORDER BY scope",
        "SELECT DISTINCT json_object('account',p.scope,'activity',json_extract(CASE WHEN issue.type='object' THEN issue.value ELSE '{}' END,'$.activityId')) AS body FROM projection_state p JOIN json_each(CASE WHEN json_valid(p.activity_issues) THEN p.activity_issues ELSE '[]' END) issue WHERE json_extract(CASE WHEN issue.type='object' THEN issue.value ELSE '{}' END,'$.kind')='MISSING_AMOUNT' ORDER BY body",
        "SELECT json_object('id',id,'asset',asset_id,'day',day,'source',source,'timestamp',timestamp,'currency',currency,'close',close) AS body FROM quotes ORDER BY asset_id,timestamp,source,id",
        "SELECT json_object('id',id,'priority',priority,'enabled',enabled,'type',provider_type) AS body FROM market_data_providers ORDER BY id",
        "SELECT json_object('id',id,'operation',operation,'hash',request_hash,'state',state,'request',request_snapshot,'result',result_json) AS body FROM business_requests WHERE operation IN ('manual_trade_record','investment_manual_trade_record') ORDER BY id",
        "SELECT json_object('kind',source_kind,'id',source_id,'generation',generation,'deleted',is_deleted) AS body FROM investment_projection_state ORDER BY source_kind,source_id",
        "SELECT json_object('id',id,'name',name,'category',category,'currency',currency,'target',target_amount,'useOn',use_on,'liquidity',liquidity,'cashMinimum',minimum_cash,'lossBudget',loss_budget,'assetLimit',single_asset_limit_bps,'archived',is_archived,'version',version) AS body FROM fund_purposes ORDER BY id",
        "SELECT json_object('purpose',purpose_id,'kind',resource_kind,'account',account_id,'currency',currency,'asset',asset_id,'cash',cash_amount,'quantity',quantity,'operation',last_operation_id) AS body FROM fund_allocations ORDER BY purpose_id,account_id,resource_kind,currency,asset_id",
        "SELECT json_object('id',id,'kind',operation_kind,'reason',reason,'sourceKind',source_kind,'source',source_id,'counterparty',source_counterparty_id,'sourceVersion',source_version,'correction',correction_of,'observation',json(observation_json),'confirmation',json(source_confirmation_json),'detail',json(detail_json),'at',created_at) AS body FROM fund_allocation_operations ORDER BY rowid",
        "SELECT json_object('id',id,'operation',operation_id,'purpose',purpose_id,'kind',resource_kind,'account',account_id,'currency',currency,'asset',asset_id,'cash',cash_delta,'quantity',quantity_delta) AS body FROM fund_allocation_entries ORDER BY rowid",
    ];
    let mut sources = Vec::new();
    for sql in queries {
        let rows = diesel::sql_query(sql).load::<Row>(conn)?;
        let rows: Vec<Value> = rows
            .into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(|e| invalid(e.to_string())))
            .collect::<JournalResult<_>>()?;
        sources.push(rows);
    }
    Ok(crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"query":query,"day":day,"currency":currency,"timezone":timezone,"sources":sources}),
    )))
}

/// Keep this connection for the entire capture. Unlike a content hash, this
/// detects a value changing and changing back between independent service reads.
pub(super) fn read_epoch(conn: &mut SqliteConnection) -> JournalResult<i64> {
    #[derive(QueryableByName)]
    struct Epoch {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        data_version: i64,
    }
    Ok(diesel::sql_query("PRAGMA data_version")
        .get_result::<Epoch>(conn)?
        .data_version)
}
