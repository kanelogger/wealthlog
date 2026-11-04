//! Independent scenario search and explicit principle evidence links.
use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestmentCaseQuery {
    #[serde(default)]
    pub scenario_tags: Vec<String>,
    pub principle_revision_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl InvestmentJournalService {
    /// Only confirmed/submitted versions are cases. Search matches categories;
    /// explicit principle references remain a separate, inspectable relationship.
    pub fn investment_cases(
        &self,
        mut query: InvestmentCaseQuery,
        caller: &JournalCaller,
    ) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        query.scenario_tags = super::super::normalize_scenario_tags(&query.scenario_tags)?;
        if let Some(id) = &query.principle_revision_id {
            nonblank(id, "principleRevisionId")?;
        }
        let limit = query.limit.unwrap_or(100);
        if !(1..=500).contains(&limit) {
            return Err(invalid("limit must be between 1 and 500"));
        }
        let mut conn = get_connection(self.store.pool())?;
        if let Some(id) = &query.principle_revision_id {
            super::principle_references::snapshots(&mut conn, std::slice::from_ref(id), false)?;
        }
        let tags = serde_json::to_string(&query.scenario_tags).map_err(json_error)?;
        let rows = diesel::sql_query(CASES)
            .bind::<Text, _>(tags)
            .bind::<Nullable<Text>, _>(query.principle_revision_id)
            .bind::<Integer, _>(limit as i32)
            .bind::<diesel::sql_types::BigInt, _>(i64::from(query.offset.unwrap_or(0)))
            .load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|row| serde_json::from_str(&row.body).map_err(json_error))
            .collect()
    }
}

const CASES: &str = r#"
SELECT body FROM (
    SELECT r.confirmed_at AS occurred_at,r.id AS id,'DECISION' AS kind,
      json_object('kind','DECISION','id',r.id,'decisionId',r.decision_id,'revisionNumber',r.revision_number,
        'title',json_extract(r.body_json,'$.title'),'status',r.status,'scenarioTags',json(r.scenario_tags_json),
        'body',json(r.body_json),'confirmedAt',r.confirmed_at,'archived',json(CASE WHEN d.lifecycle='ARCHIVED' THEN 'true' ELSE 'false' END),
        'principleRelations',json((SELECT json_group_array(json_object('principleRevisionId',p.principle_revision_id,'relation','REFERENCED'))
          FROM investment_decision_principles p WHERE p.decision_revision_id=r.id))) AS body
    FROM investment_decision_revisions r JOIN investment_decisions d ON d.id=r.decision_id
    WHERE r.status='CONFIRMED'
      AND NOT EXISTS(SELECT 1 FROM json_each(?1) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(r.scenario_tags_json) tag WHERE tag.value=wanted.value))
      AND (?2 IS NULL OR EXISTS(SELECT 1 FROM investment_decision_principles p WHERE p.decision_revision_id=r.id AND p.principle_revision_id=?2))
    UNION ALL
    SELECT r.submitted_at AS occurred_at,r.id AS id,'REVIEW' AS kind,
      json_object('kind','REVIEW','id',r.id,'decisionRevisionId',r.decision_revision_id,'rootId',r.root_id,
        'revisionNumber',r.revision_number,'decisionTitle',json_extract(dec.body_json,'$.title'),
        'decisionRevisionNumber',dec.revision_number,'status',r.status,'scenarioTags',json(r.scenario_tags_json),
        'body',json(r.body_json),'observationCutoff',r.observation_cutoff,'submittedAt',r.submitted_at,
        'principleRelations',json((SELECT json_group_array(json_object('principleRevisionId',p.principle_revision_id,'newPrincipleRevisionId',p.new_principle_revision_id,'relation',p.disposition))
          FROM investment_review_principles p WHERE p.review_id=r.id)),
        'derivedPrincipleRevisions',json((SELECT json_group_array(p.id) FROM investment_principle_revisions p WHERE p.source_review_id=r.id))) AS body
    FROM investment_reviews r JOIN investment_decision_revisions dec ON dec.id=r.decision_revision_id
    WHERE r.status='SUBMITTED'
      AND NOT EXISTS(SELECT 1 FROM json_each(?1) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(r.scenario_tags_json) tag WHERE tag.value=wanted.value))
      AND (?2 IS NULL OR EXISTS(SELECT 1 FROM investment_review_principles p WHERE p.review_id=r.id AND (p.principle_revision_id=?2 OR p.new_principle_revision_id=?2))
        OR EXISTS(SELECT 1 FROM investment_principle_revisions p WHERE p.id=?2 AND p.source_review_id=r.id))
) ORDER BY occurred_at DESC,kind,id LIMIT ?3 OFFSET ?4
"#;
