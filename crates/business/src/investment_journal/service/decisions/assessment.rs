//! Deterministic comparisons of user-provided plans against the displayed context.
use crate::investment_journal::{
    DecisionBody, ExactDecimal, FundResource, InvestmentContextCapture, JournalResult,
};
use serde_json::{json, Value};

pub(super) fn attach(
    capture: &mut InvestmentContextCapture,
    body: &DecisionBody,
) -> JournalResult<()> {
    let funding = &mut capture.response.funding.data;
    let purposes = funding
        .get("purposes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let resources = funding
        .pointer("/allocation/resources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut ids = body.purpose_ids.clone();
    ids.extend(body.funding.iter().map(|f| f.purpose_id.clone()));
    ids.sort();
    ids.dedup();
    let mut losses = Vec::new();
    for id in ids {
        let purpose = purposes.iter().find(|p| p["id"].as_str() == Some(&id));
        let budget = purpose
            .and_then(|p| p["lossBudget"].as_str())
            .map(ExactDecimal::parse)
            .transpose()?;
        let currency = purpose.and_then(|p| p["currency"].as_str());
        let hypothesis = body
            .evidence
            .iter()
            .find(|e| e.purpose_id.as_deref() == Some(&id));
        let amount = hypothesis.and_then(|e| e.assumed_loss_amount);
        let status = match (hypothesis, amount, purpose, budget) {
            (None, _, _, _) => "NO_SCENARIO_PROVIDED",
            (_, _, None, _) => "UNABLE_TO_DETERMINE",
            (_, _, _, None) => "NOT_CONFIGURED",
            (Some(e), Some(amount), _, Some(budget)) if e.loss_currency.as_deref() == currency => {
                if amount > budget {
                    "SCENARIO_EXCEEDS_BUDGET"
                } else {
                    "SCENARIO_WITHIN_BUDGET"
                }
            }
            _ => "UNABLE_TO_DETERMINE",
        };
        losses.push(json!({"purposeId":id,"currency":currency,"lossBudget":budget,"assumedLossAmount":amount,"hypothesisBasis":hypothesis.and_then(|e|e.loss_basis.as_deref()),"status":status,"isPrediction":false}));
    }
    let mut planned = Vec::new();
    for line in &body.funding {
        let source = resources.iter().find(|r| {
            serde_json::from_value::<FundResource>(r["observation"]["resource"].clone())
                .is_ok_and(|r| r == line.resource)
        });
        let ownership_known = funding.get("allocation").is_some_and(Value::is_object);
        let before = if ownership_known {
            Some(
                source
                    .and_then(|r| r["purposes"][&line.purpose_id].as_str())
                    .map(ExactDecimal::parse)
                    .transpose()?
                    .unwrap_or(ExactDecimal::ZERO),
            )
        } else {
            None
        };
        let after = before
            .and_then(|v| v.value().checked_sub(line.planned.value()))
            .map(ExactDecimal::new);
        let available = source
            .and_then(|r| r["balance"]["unallocated"].as_str())
            .map(ExactDecimal::parse)
            .transpose()?;
        let known = source
            .is_some_and(|r| r["balance"]["actual"].is_string() && r["balance"]["excess"] == "0");
        let status = if !known || after.is_none() {
            "UNABLE_TO_DETERMINE"
        } else if after.is_some_and(|v| v < ExactDecimal::ZERO) {
            "PURPOSE_SHORTFALL"
        } else {
            "WITHIN_PURPOSE_OWNERSHIP"
        };
        let shortfall = after.map(|v| {
            if v < ExactDecimal::ZERO {
                ExactDecimal::new(-v.value())
            } else {
                ExactDecimal::ZERO
            }
        });
        planned.push(json!({"purposeId":line.purpose_id,"resource":line.resource,"planned":line.planned,"purposeAmountBefore":before,"purposeAmountAfter":after,"shortfall":shortfall,"unallocatedAvailable":available,"status":status,"topUpAssumed":false}));
    }
    if !funding.is_object() {
        *funding = json!({});
    }
    if let Some(risks) = funding.get_mut("risks").and_then(Value::as_array_mut) {
        for risk in risks {
            if let Some(loss) = losses.iter().find(|l| l["purposeId"] == risk["purposeId"]) {
                risk["lossComparisonStatus"] = loss["status"].clone();
            }
        }
    }
    funding["decisionAssessment"] = json!({"schemaVersion":1,"lossScenarios":losses,"fundingStatus":body.funding_status,"plannedFunding":planned,"reservesResources":false});
    Ok(())
}
