//! Mutable review drafts; submitted corrections append inside one fixed cutoff root.
use super::*;
use crate::investment_journal::{DiscardReviewDraft, ReviewBody, SaveReviewDraft};
use crate::investment_journal::{
    ReviewDraftTarget, ReviewFactsCapture, ReviewFactsQuery, SubmitReview,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSubmissionPreview {
    pub schema_version: u32,
    pub ledger_id: String,
    pub target: ReviewDraftTarget,
    pub body: ReviewBody,
    pub expected_content_hash: String,
    pub expected_facts_hash: String,
    pub facts: ReviewFactsCapture,
}
#[derive(QueryableByName)]
struct Row {
    #[diesel(sql_type=Text)]
    id: String,
    #[diesel(sql_type=Text)]
    root_id: String,
    #[diesel(sql_type=Text)]
    decision_revision_id: String,
    #[diesel(sql_type=Text)]
    observation_cutoff: String,
    #[diesel(sql_type=Text)]
    observation_timezone: String,
    #[diesel(sql_type=Integer)]
    revision_number: i32,
    #[diesel(sql_type=Text)]
    status: String,
    #[diesel(sql_type=Integer)]
    version: i32,
}
impl InvestmentJournalService {
    pub async fn preview_review_submission(
        &self,
        target: ReviewDraftTarget,
        caller: &JournalCaller,
    ) -> JournalResult<ReviewSubmissionPreview> {
        caller.require_read()?;
        let (query, body, expected_content_hash) = {
            let mut conn = get_connection(self.store.pool())?;
            submission_target(&mut conn, &target)?
        };
        let reader = self
            .review_facts_reader
            .as_ref()
            .ok_or(JournalError::ResourceUnavailable)?;
        let facts = reader.capture_review(&query).await?;
        facts.validate_envelope(&query)?;
        let expected_facts_hash = facts.stable_hash()?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            if submission_target(conn, &target)?.2 != expected_content_hash {
                return Err(JournalError::StateConflict);
            }
            reader.validate_review(conn, &facts)
        })?;
        Ok(ReviewSubmissionPreview {
            schema_version: 1,
            ledger_id: self.ledger_id.clone(),
            target,
            body,
            expected_content_hash,
            expected_facts_hash,
            facts,
        })
    }

    pub async fn submit_review(
        &self,
        input: SubmitReview,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_confirm()?;
        if input.ledger_id != self.ledger_id {
            return Err(JournalError::PermissionDenied);
        }
        let record = self.request(
            "investment_review_submit",
            &input.request_key,
            &input,
            caller,
        )?;
        let (query, _, hash) = {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
            submission_target(&mut conn, &input.target)?
        };
        if hash != input.expected_content_hash {
            return Err(JournalError::StateConflict);
        }
        input.facts.validate_envelope(&query)?;
        if input.facts.stable_hash()? != input.expected_facts_hash {
            return Err(JournalError::StateConflict);
        }
        let reader = self
            .review_facts_reader
            .as_ref()
            .ok_or(JournalError::ResourceUnavailable)?
            .clone();
        let current = reader.capture_review(&query).await?;
        current.validate_envelope(&query)?;
        if current.stable_hash()? != input.expected_facts_hash {
            return Err(JournalError::StateConflict);
        }
        let (day, zone) = self.business_day()?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store, move |conn| {
            if let Some(result) = replay(conn, &record)? { return Ok(result); }
            if submission_target(conn, &input.target)?.2 != input.expected_content_hash { return Err(JournalError::StateConflict); }
            reader.validate_review(conn, &current)?;
            let row = review_row(conn, &input.target.review_id)?;
            let at = now(); let request_id=uuid::Uuid::new_v4().to_string();
            let version = increment(row.version)?;
            let data=json!({"facts":input.facts,"contentHash":input.expected_content_hash,"factsHash":input.expected_facts_hash,"at":at,"channel":caller.channel.as_str(),"actorKind":caller.actor_kind.as_str(),"actorId":caller.actor_id,"credentialId":caller.credential_id,"requestId":request_id,"version":version}).to_string();
            diesel::sql_query("UPDATE investment_reviews SET status='SUBMITTED',facts_snapshot_json=json_extract(?1,'$.facts'),content_hash=json_extract(?1,'$.contentHash'),facts_hash=json_extract(?1,'$.factsHash'),version=json_extract(?1,'$.version'),submitted_at=json_extract(?1,'$.at'),submitted_channel=json_extract(?1,'$.channel'),submitted_actor_kind=json_extract(?1,'$.actorKind'),submitted_actor_id=json_extract(?1,'$.actorId'),submitted_credential_id=json_extract(?1,'$.credentialId'),submitted_request_id=json_extract(?1,'$.requestId'),updated_at=json_extract(?1,'$.at'),updated_channel=json_extract(?1,'$.channel'),updated_actor_kind=json_extract(?1,'$.actorKind'),updated_actor_id=json_extract(?1,'$.actorId'),updated_credential_id=json_extract(?1,'$.credentialId') WHERE id=?2 AND status='DRAFT'")
                .bind::<Text,_>(data).bind::<Text,_>(&row.id).execute(conn)?;
            // Only an observation covering the current version's due date resolves that reminder.
            // The capture guard also freezes the exact schedule, so rescheduling requires a new preview.
            let reminder = &input.facts.observations["reminder"];
            let resolved = reminder["currentRevisionId"].as_str()==Some(&row.decision_revision_id)
                && reminder["nextReviewAt"].as_str().is_some_and(|due|due<=row.observation_cutoff.as_str());
            if resolved {
                diesel::sql_query("UPDATE investment_decisions SET next_review_at=NULL,version=version+1,updated_at=?1 WHERE id=?2 AND current_revision_id=?3 AND next_review_at=?4")
                    .bind::<Text,_>(&at).bind::<Text,_>(reminder["decisionId"].as_str().ok_or(JournalError::StateConflict)?)
                    .bind::<Text,_>(&row.decision_revision_id).bind::<Text,_>(reminder["nextReviewAt"].as_str().ok_or(JournalError::StateConflict)?).execute(conn)?;
            }
            complete_with_id(conn,&record,&caller,&ledger,&row.id,json!({"reviewId":row.id,"rootId":row.root_id,"revisionNumber":row.revision_number,"version":version,"submittedAt":at,"reviewReminderResolved":resolved}),day,&zone,&request_id)
        }).await
    }
    pub fn get_review(&self, id: &str, caller: &JournalCaller) -> JournalResult<Value> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        review_detail(&mut conn, id)
    }
    pub fn decision_reviews(
        &self,
        decision_revision_id: &str,
        caller: &JournalCaller,
    ) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        let rows=diesel::sql_query("SELECT id AS body FROM investment_reviews WHERE decision_revision_id=?1 ORDER BY observation_cutoff,root_id,revision_number").bind::<Text,_>(decision_revision_id).load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|r| review_detail(&mut conn, &r.body))
            .collect()
    }
    pub async fn save_review_draft(
        &self,
        mut input: SaveReviewDraft,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        input.body.normalize()?;
        nonblank(&input.decision_revision_id, "decisionRevisionId")?;
        if input.review_id.is_some() != input.expected_version.is_some()
            || input.expected_version.is_some_and(|v| v <= 0)
        {
            return Err(invalid(
                "editing a review requires reviewId and expectedVersion",
            ));
        }
        let record = self.request(
            "investment_review_draft_save",
            &input.request_key,
            &input,
            caller,
        )?;
        {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
        }
        let (day, zone) = self.business_day()?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store,move|conn|{
            if let Some(result)=replay(conn,&record)?{return Ok(result);}
            validate_principles(conn,&input.body)?;
            let decision=diesel::sql_query("SELECT id AS body FROM investment_decision_revisions WHERE id=?1 AND status='CONFIRMED'").bind::<Text,_>(&input.decision_revision_id).get_result::<JsonRow>(conn).optional()?;
            if decision.is_none(){return Err(invalid("review requires a confirmed decision revision"));}
            let (id,root,number,parent,version,observation_timezone)=if let Some(id)=&input.review_id {
                let row=review_row(conn,id)?;
                if row.status!="DRAFT"||row.version!=input.expected_version.unwrap_or_default(){return Err(JournalError::StateConflict);}
                if row.decision_revision_id!=input.decision_revision_id||row.observation_cutoff!=input.observation_cutoff.to_string()||input.root_id.as_ref().is_some_and(|id|id!=&row.root_id){return Err(invalid("a new decision version or observation cutoff requires a new review root"));}
                (id.clone(),row.root_id,row.revision_number,None,increment(row.version)?,row.observation_timezone)
            }else{
                let id=uuid::Uuid::new_v4().to_string();
                if let Some(root)=&input.root_id {
                    let row=review_row(conn,root)?;
                    if row.root_id!=row.id||row.decision_revision_id!=input.decision_revision_id||row.observation_cutoff!=input.observation_cutoff.to_string(){return Err(invalid("review root has a different decision or observation cutoff"));}
                    let active=diesel::sql_query("SELECT id AS body FROM investment_reviews WHERE root_id=?1 AND status='DRAFT'").bind::<Text,_>(root).get_result::<JsonRow>(conn).optional()?;
                    if active.is_some(){return Err(JournalError::StateConflict);}
                    let parent=diesel::sql_query("SELECT id AS body FROM investment_reviews WHERE root_id=?1 AND status='SUBMITTED' ORDER BY revision_number DESC LIMIT 1").bind::<Text,_>(root).get_result::<JsonRow>(conn).optional()?.map(|r|r.body);
                    #[derive(QueryableByName)]struct Number{#[diesel(sql_type=Integer)]number:i32}
                    let number=diesel::sql_query("SELECT MAX(revision_number) AS number FROM investment_reviews WHERE root_id=?1").bind::<Text,_>(root).get_result::<Number>(conn)?.number;
                    (id,root.clone(),increment(number)?,parent,1,row.observation_timezone)
                }else{
                    let duplicate=diesel::sql_query("SELECT id AS body FROM investment_reviews WHERE id=root_id AND decision_revision_id=?1 AND observation_cutoff=?2 AND observation_timezone=?3").bind::<Text,_>(&input.decision_revision_id).bind::<Text,_>(input.observation_cutoff.to_string()).bind::<Text,_>(&zone).get_result::<JsonRow>(conn).optional()?;
                    if duplicate.is_some(){return Err(JournalError::StateConflict);}
                    (id.clone(),id,1,None,1,zone.clone())
                }
            };
            let origin_today=wealthfolio_core::utils::time_utils::user_today(parse_user_timezone(&observation_timezone)?);
            if input.observation_cutoff>origin_today{return Err(invalid("observationCutoff cannot be in the future"));}
            let at=now();let data=json!({"observationTimezone":observation_timezone,"id":id,"rootId":root,"number":number,"parent":parent,"decisionRevisionId":input.decision_revision_id,"cutoff":input.observation_cutoff,"version":version,"body":input.body,"at":at,"channel":caller.channel.as_str(),"actorKind":caller.actor_kind.as_str(),"actorId":caller.actor_id,"credentialId":caller.credential_id}).to_string();
            if input.review_id.is_some(){diesel::sql_query(UPDATE_DRAFT).bind::<Text,_>(&data).execute(conn)?;}else{diesel::sql_query(INSERT_DRAFT).bind::<Text,_>(&data).execute(conn)?;}
            save_principles(conn,&id,&input.body)?;
            complete(conn,&record,&caller,&ledger,&id,json!({"reviewId":id,"rootId":root,"revisionNumber":number,"version":version}),day,&zone)
        }).await
    }
    pub async fn discard_review_draft(
        &self,
        input: DiscardReviewDraft,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        let record = self.request(
            "investment_review_draft_discard",
            &input.request_key,
            &input,
            caller,
        )?;
        let (day, zone) = self.business_day()?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store,move|conn|{
            if let Some(result)=replay(conn,&record)?{return Ok(result);}
            let row=review_row(conn,&input.target.review_id)?;if row.status!="DRAFT"||row.version!=input.target.expected_version{return Err(JournalError::StateConflict);}
            let version=increment(row.version)?;
            diesel::sql_query("UPDATE investment_reviews SET status='DISCARDED',version=?1,updated_at=?2,updated_channel=?3,updated_actor_kind=?4,updated_actor_id=?5,updated_credential_id=?6 WHERE id=?7")
                .bind::<Integer,_>(version).bind::<Text,_>(now()).bind::<Text,_>(caller.channel.as_str()).bind::<Text,_>(caller.actor_kind.as_str()).bind::<Text,_>(&caller.actor_id).bind::<Nullable<Text>,_>(&caller.credential_id).bind::<Text,_>(&row.id).execute(conn)?;
            complete(conn,&record,&caller,&ledger,&row.id,json!({"reviewId":row.id,"rootId":row.root_id,"version":version}),day,&zone)
        }).await
    }
}
fn submission_target(
    conn: &mut SqliteConnection,
    target: &ReviewDraftTarget,
) -> JournalResult<(ReviewFactsQuery, ReviewBody, String)> {
    let row = review_row(conn, &target.review_id)?;
    if row.status != "DRAFT" || row.version != target.expected_version {
        return Err(JournalError::StateConflict);
    }
    let detail = review_detail(conn, &row.id)?;
    let body: ReviewBody = serde_json::from_value(detail["body"].clone()).map_err(json_error)?;
    body.validate_submission()?;
    validate_principles(conn, &body)?;
    let cutoff = NaiveDate::parse_from_str(&row.observation_cutoff, "%Y-%m-%d")
        .map_err(|_| invalid("invalid observation cutoff"))?;
    let zone = parse_user_timezone(&row.observation_timezone)?;
    if cutoff > user_today(zone) {
        return Err(invalid("observation cutoff cannot be in the future"));
    }
    // A submitted correction always extends the last submitted version, never a sibling.
    let latest=diesel::sql_query("SELECT id AS body FROM investment_reviews WHERE root_id=?1 AND status='SUBMITTED' ORDER BY revision_number DESC LIMIT 1")
        .bind::<Text,_>(&row.root_id).get_result::<JsonRow>(conn).optional()?.map(|r|r.body);
    if latest.as_deref() != detail["correctsReviewId"].as_str() {
        return Err(JournalError::StateConflict);
    }
    let query = ReviewFactsQuery {
        decision_revision_id: row.decision_revision_id,
        observation_cutoff: cutoff,
        observation_timezone: row.observation_timezone,
        principle_revision_ids: body.principle_ids(),
    };
    let hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"body":body,"rootId":row.root_id,"revisionNumber":row.revision_number,"correctsReviewId":latest,"query":query}),
    ));
    Ok((query, body, hash))
}

/// Both capture and writer validation use the same original confirmed snapshot and links.
pub(crate) fn review_observations(
    conn: &mut SqliteConnection,
    query: &ReviewFactsQuery,
) -> JournalResult<Value> {
    let decision=diesel::sql_query("SELECT json_object('id',id,'decisionId',decision_id,'revisionNumber',revision_number,'body',json(body_json),'contextAtConfirmation',json(context_snapshot_json),'contentHash',content_hash,'contextHash',context_hash,'confirmedAt',confirmed_at,'confirmedChannel',confirmed_channel,'confirmedActorKind',confirmed_actor_kind,'confirmedActorId',confirmed_actor_id,'confirmedCredentialId',confirmed_credential_id,'contentOrigin',content_origin) AS body FROM investment_decision_revisions WHERE id=?1 AND status='CONFIRMED'")
        .bind::<Text,_>(&query.decision_revision_id).get_result::<JsonRow>(conn).optional()?.ok_or(JournalError::StateConflict)?;
    let decision: Value = serde_json::from_str(&decision.body).map_err(json_error)?;
    let reminder=diesel::sql_query("SELECT json_object('decisionId',id,'currentRevisionId',current_revision_id,'nextReviewAt',next_review_at) AS body FROM investment_decisions WHERE id=?1")
        .bind::<Text,_>(decision["decisionId"].as_str().ok_or(JournalError::StateConflict)?).get_result::<JsonRow>(conn)?;
    let reminder: Value = serde_json::from_str(&reminder.body).map_err(json_error)?;
    let rows=diesel::sql_query("SELECT json_object('id',id,'executionId',execution_id,'linkedAt',linked_at,'sourceVersion',source_version,'temporalClassification',temporal_classification,'deviationReason',deviation_reason,'snapshotAtLink',json(snapshot_json),'unlinkedAt',unlinked_at,'unlinkReason',unlink_reason) AS body FROM investment_execution_links WHERE decision_revision_id=?1 ORDER BY id")
        .bind::<Text,_>(&query.decision_revision_id).load::<JsonRow>(conn)?;
    let mut executions = Vec::new();
    for row in rows {
        let link: Value = serde_json::from_str(&row.body).map_err(json_error)?;
        let id = link["executionId"]
            .as_str()
            .ok_or(JournalError::StateConflict)?;
        let source = super::execution_reconciliation::source_anchor(conn, id)?;
        let event_day =
            super::queue::execution_date(&source.execution, &query.observation_timezone).ok();
        let scope = match event_day {
            None => "UNDETERMINED",
            Some(day) if day > query.observation_cutoff => "AFTER_CUTOFF",
            _ => "IN_SCOPE",
        };
        let acknowledged = super::execution_reconciliation::acknowledged(conn, &source)?;
        executions.push(json!({"link":link,"eventDate":event_day,"observationScope":scope,"currentSource":source,"sourceAcknowledged":acknowledged}));
    }
    let principles =
        super::principle_references::snapshots(conn, &query.principle_revision_ids, false)?;
    Ok(
        json!({"schemaVersion":1,"decision":decision,"reminder":reminder,"executions":executions,"principles":principles,"executionScope":"LINKS_AT_CAPTURE_WITH_EVENT_CUTOFF","quantityBasis":"ORIGINAL_EVENT_UNITS","automaticOutcomeAttribution":false}),
    )
}
fn increment(v: i32) -> JournalResult<i32> {
    v.checked_add(1)
        .ok_or_else(|| invalid("review version out of range"))
}
fn review_row(conn: &mut SqliteConnection, id: &str) -> JournalResult<Row> {
    diesel::sql_query("SELECT id,root_id,decision_revision_id,observation_cutoff,observation_timezone,revision_number,status,version FROM investment_reviews WHERE id=?1").bind::<Text,_>(id).get_result(conn).optional()?.ok_or(JournalError::StateConflict)
}
fn validate_principles(conn: &mut SqliteConnection, body: &ReviewBody) -> JournalResult<()> {
    // Assess an already published rule; a proposed replacement can remain a
    // draft and is captured with that explicit status without publishing it.
    super::principle_references::snapshots(
        conn,
        &body
            .principles
            .iter()
            .filter_map(|p| p.principle_revision_id.clone())
            .collect::<Vec<_>>(),
        true,
    )?;
    for item in &body.principles {
        for id in [&item.principle_revision_id, &item.new_principle_revision_id]
            .into_iter()
            .flatten()
        {
            if diesel::sql_query(
                "SELECT id AS body FROM investment_principle_revisions WHERE id=?1",
            )
            .bind::<Text, _>(id)
            .get_result::<JsonRow>(conn)
            .optional()?
            .is_none()
            {
                return Err(invalid("review principle version not found"));
            }
        }
    }
    Ok(())
}
fn save_principles(conn: &mut SqliteConnection, id: &str, body: &ReviewBody) -> JournalResult<()> {
    diesel::sql_query("DELETE FROM investment_review_principles WHERE review_id=?1")
        .bind::<Text, _>(id)
        .execute(conn)?;
    diesel::sql_query("INSERT INTO investment_review_principles(id,review_id,position,principle_revision_id,disposition,new_principle_revision_id,reason) SELECT ?1||':principle:'||key,?1,key,json_extract(value,'$.principleRevisionId'),json_extract(value,'$.disposition'),json_extract(value,'$.newPrincipleRevisionId'),json_extract(value,'$.reason') FROM json_each(?2,'$.principles')")
        .bind::<Text,_>(id).bind::<Text,_>(serde_json::to_string(body).map_err(json_error)?).execute(conn)?;
    Ok(())
}
fn review_detail(conn: &mut SqliteConnection, id: &str) -> JournalResult<Value> {
    let row=diesel::sql_query("SELECT json_object('id',id,'decisionRevisionId',decision_revision_id,'rootId',root_id,'revisionNumber',revision_number,'correctsReviewId',corrects_review_id,'status',status,'observationCutoff',observation_cutoff,'observationTimezone',observation_timezone,'version',version,'body',json(body_json),'factsSnapshot',json(facts_snapshot_json),'contentHash',content_hash,'factsHash',facts_hash,'createdAt',created_at,'updatedAt',updated_at,'createdChannel',created_channel,'createdActorKind',created_actor_kind,'createdActorId',created_actor_id,'createdCredentialId',created_credential_id,'updatedChannel',updated_channel,'updatedActorKind',updated_actor_kind,'updatedActorId',updated_actor_id,'updatedCredentialId',updated_credential_id,'contentOrigin',content_origin,'submittedAt',submitted_at,'submittedChannel',submitted_channel,'submittedActorKind',submitted_actor_kind,'submittedActorId',submitted_actor_id,'submittedCredentialId',submitted_credential_id) AS body FROM investment_reviews WHERE id=?1")
        .bind::<Text,_>(id).get_result::<JsonRow>(conn).optional()?.ok_or(JournalError::StateConflict)?;
    serde_json::from_str(&row.body).map_err(json_error)
}

const INSERT_DRAFT: &str = r#"INSERT INTO investment_reviews(id,root_id,decision_revision_id,observation_cutoff,revision_number,corrects_review_id,observation_timezone,created_at,created_channel,created_actor_kind,created_actor_id,created_credential_id,judgment,execution_assessment,deviation_reason,outcome_evidence_json,other_explanations,open_questions,scenario_tags_json,body_json,version,updated_at,updated_channel,updated_actor_kind,updated_actor_id,updated_credential_id,content_origin,status) VALUES(json_extract(?1,'$.id'),json_extract(?1,'$.rootId'),json_extract(?1,'$.decisionRevisionId'),json_extract(?1,'$.cutoff'),json_extract(?1,'$.number'),json_extract(?1,'$.parent'),json_extract(?1,'$.observationTimezone'),json_extract(?1,'$.at'),json_extract(?1,'$.channel'),json_extract(?1,'$.actorKind'),json_extract(?1,'$.actorId'),json_extract(?1,'$.credentialId'),json_extract(?1,'$.body.judgment'),json_extract(?1,'$.body.executionAssessment'),json_extract(?1,'$.body.deviationReason'),json_extract(?1,'$.body.outcomeEvidence'),json_extract(?1,'$.body.otherExplanations'),json_extract(?1,'$.body.openQuestions'),json_extract(?1,'$.body.scenarioTags'),json_extract(?1,'$.body'),json_extract(?1,'$.version'),json_extract(?1,'$.at'),json_extract(?1,'$.channel'),json_extract(?1,'$.actorKind'),json_extract(?1,'$.actorId'),json_extract(?1,'$.credentialId'),json_extract(?1,'$.body.contentOrigin'),'DRAFT')"#;
const UPDATE_DRAFT: &str = r#"UPDATE investment_reviews SET judgment=json_extract(?1,'$.body.judgment'),execution_assessment=json_extract(?1,'$.body.executionAssessment'),deviation_reason=json_extract(?1,'$.body.deviationReason'),outcome_evidence_json=json_extract(?1,'$.body.outcomeEvidence'),other_explanations=json_extract(?1,'$.body.otherExplanations'),open_questions=json_extract(?1,'$.body.openQuestions'),scenario_tags_json=json_extract(?1,'$.body.scenarioTags'),body_json=json_extract(?1,'$.body'),version=json_extract(?1,'$.version'),updated_at=json_extract(?1,'$.at'),updated_channel=json_extract(?1,'$.channel'),updated_actor_kind=json_extract(?1,'$.actorKind'),updated_actor_id=json_extract(?1,'$.actorId'),updated_credential_id=json_extract(?1,'$.credentialId'),content_origin=json_extract(?1,'$.body.contentOrigin') WHERE id=json_extract(?1,'$.id') AND status='DRAFT'"#;
