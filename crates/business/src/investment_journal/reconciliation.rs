use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    ExactDecimal, FundResource, JournalResult, ResourceBalance, ResourceObservation, SourceEffect,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FundReconciliationKind {
    PurposeExcess,
    ResourceUnavailable,
    SourceChanged,
    SourceInvalidated,
    CorporateAction,
    Calibration,
    TransferChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FundReconciliationItem {
    pub id: String,
    pub kind: FundReconciliationKind,
    pub resource: FundResource,
    #[serde(default)]
    pub operation_id: Option<String>,
    pub source_id: Option<String>,
    pub current_source_version: Option<String>,
    pub excess: Option<ExactDecimal>,
}

/// Compare confirmed source versions independently from numeric excess. Equal
/// totals never erase offsetting unconfirmed sources, and confirming sources
/// with no entries never erases a remaining numeric excess.
pub fn compare_fund_observation(
    baseline: &ResourceObservation,
    current: &ResourceObservation,
    allocated: ExactDecimal,
) -> JournalResult<Vec<FundReconciliationItem>> {
    if baseline.resource != current.resource {
        return Err(super::model::invalid(
            "baseline belongs to a different resource",
        ));
    }
    let mut result: Vec<_> = baseline
        .unresolved_sources
        .iter()
        .filter(|i| {
            !matches!(
                i.kind,
                FundReconciliationKind::PurposeExcess | FundReconciliationKind::ResourceUnavailable
            )
        })
        .cloned()
        .collect();
    let balance = ResourceBalance::calculate(current.actual, allocated)?;
    let make = |kind, source_id: Option<String>, version: Option<String>, excess| {
        let id = crate::requests::hash_json(&crate::requests::canonicalize(
            json!({"kind":kind,"resource":current.resource,"sourceId":source_id,"version":version}),
        ));
        FundReconciliationItem {
            id,
            kind,
            resource: current.resource.clone(),
            operation_id: None,
            source_id,
            current_source_version: version,
            excess,
        }
    };
    if balance.excess.is_some_and(|e| e > ExactDecimal::ZERO) {
        result.push(make(
            FundReconciliationKind::PurposeExcess,
            None,
            None,
            balance.excess,
        ));
    }
    if current.actual.is_none() && allocated > ExactDecimal::ZERO {
        result.push(make(
            FundReconciliationKind::ResourceUnavailable,
            None,
            None,
            None,
        ));
    }
    // A resource without ownership does not need a purpose reconciliation.
    if allocated == ExactDecimal::ZERO {
        return Ok(result);
    }
    let old: BTreeMap<_, _> = baseline
        .sources
        .iter()
        .map(|s| ((s.kind.clone(), s.id.clone()), s))
        .collect();
    let new: BTreeMap<_, _> = current
        .sources
        .iter()
        .map(|s| ((s.kind.clone(), s.id.clone()), s))
        .collect();
    for (key, source) in &new {
        let prior = old.get(key);
        let kind = match prior {
            Some(p) if p.version == source.version && p.applied == source.applied => continue,
            Some(p)
                if !p.applied
                    && !source.applied
                    && source.effect != SourceEffect::SecurityTransfer =>
            {
                continue
            }
            Some(p) if p.applied && !source.applied => FundReconciliationKind::SourceInvalidated,
            Some(_) if source.effect == SourceEffect::CashTransfer => {
                FundReconciliationKind::TransferChanged
            }
            Some(_) => FundReconciliationKind::SourceChanged,
            None if source.effect == SourceEffect::SecurityTransfer => {
                FundReconciliationKind::SourceChanged
            }
            None if source.applied && source.effect == SourceEffect::CorporateAction => {
                FundReconciliationKind::CorporateAction
            }
            None if source.applied && source.effect == SourceEffect::Calibration => {
                FundReconciliationKind::Calibration
            }
            // New ordinary spending changes B and can create excess, but is
            // never relabelled as a corporate action or an unexplained inflow.
            None => continue,
        };
        result.push(make(
            kind,
            Some(source.id.clone()),
            Some(source.version.clone()),
            None,
        ));
    }
    for (key, prior) in old {
        if !new.contains_key(&key) && prior.applied {
            result.push(make(
                FundReconciliationKind::SourceInvalidated,
                Some(prior.id.clone()),
                None,
                None,
            ));
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result.dedup_by(|a, b| a.id == b.id);
    Ok(result)
}
