mod assessment;
use super::*;
use crate::investment_journal::{
    CheckScheduleChange, DecisionBody, DecisionDraftTarget, DecisionMutation, DecisionQuery,
    InvestmentContextCapture, InvestmentContextQuery, MutateDecision, SaveDecisionDraft,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionConfirmationPreview {
    pub schema_version: u32,
    pub ledger_id: String,
    pub target: DecisionDraftTarget,
    pub revision_number: i32,
    pub body: DecisionBody,
    pub next_check_at: Option<NaiveDate>,
    pub expected_content_hash: String,
    pub expected_context_hash: String,
    pub context: InvestmentContextCapture,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmDecisionDraft {
    pub ledger_id: String,
    pub target: DecisionDraftTarget,
    pub expected_content_hash: String,
    pub expected_context_hash: String,
    /// The displayed capture is frozen after comparison with authoritative sources.
    pub context: InvestmentContextCapture,
    pub request_key: String,
}

#[derive(QueryableByName)]
struct DecisionRoot {
    #[diesel(sql_type=Text)]
    id: String,
    #[diesel(sql_type=Text)]
    lifecycle: String,
    #[diesel(sql_type=Text)]
    execution_state: String,
    #[diesel(sql_type=Nullable<Text>)]
    current_revision_id: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    draft_revision_id: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    next_review_at: Option<String>,
    #[diesel(sql_type=Integer)]
    version: i32,
}
#[derive(QueryableByName)]
struct RevisionRow {
    #[diesel(sql_type=Text)]
    id: String,
    #[diesel(sql_type=Text)]
    decision_id: String,
    #[diesel(sql_type=Integer)]
    revision_number: i32,
    #[diesel(sql_type=Text)]
    status: String,
    #[diesel(sql_type=Text)]
    body_json: String,
    #[diesel(sql_type=Nullable<Text>)]
    next_check_at: Option<String>,
}

impl InvestmentJournalService {
    pub async fn preview_decision_context(
        &self,
        mut body: DecisionBody,
        caller: &JournalCaller,
    ) -> JournalResult<InvestmentContextCapture> {
        caller.require_read()?;
        body.normalize()?;
        let query = InvestmentContextQuery::for_decision(&body);
        let mut capture = self.get_investment_context(query, caller).await?;
        assessment::attach(&mut capture, &body)?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            validate_purpose_references(conn, &body)?;
            attach_principles(conn, &mut capture, &body, &self.business_day()?.1)?;
            self.context_reader
                .as_ref()
                .ok_or(JournalError::ResourceUnavailable)?
                .validate(conn, &capture)
        })?;
        Ok(capture)
    }
    pub async fn get_investment_context(
        &self,
        query: InvestmentContextQuery,
        caller: &JournalCaller,
    ) -> JournalResult<InvestmentContextCapture> {
        caller.require_read()?;
        let (day, _) = self.business_day()?;
        let reader = self
            .context_reader
            .as_ref()
            .ok_or(JournalError::ResourceUnavailable)?;
        let capture = reader.capture(&query, day).await?;
        capture.validate_envelope(&query, day)?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| reader.validate(conn, &capture))?;
        Ok(capture)
    }
    /// Reading a draft, history or an empty list never enables the module.
    pub fn get_decision(&self, id: &str, caller: &JournalCaller) -> JournalResult<Value> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        decision_detail(&mut conn, id)
    }

    pub fn list_decisions(
        &self,
        mut query: DecisionQuery,
        caller: &JournalCaller,
    ) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        query.scenario_tags = super::super::normalize_scenario_tags(&query.scenario_tags)?;
        if query
            .lifecycle
            .as_ref()
            .is_some_and(|s| !["DRAFT", "CONFIRMED", "ARCHIVED"].contains(&s.as_str()))
        {
            return Err(invalid("unknown decision lifecycle"));
        }
        let limit = query.limit.unwrap_or(100);
        if !(1..=500).contains(&limit) {
            return Err(invalid("limit must be between 1 and 500"));
        }
        let mut conn = get_connection(self.store.pool())?;
        let tags = serde_json::to_string(&query.scenario_tags).map_err(json_error)?;
        let rows=diesel::sql_query(r#"SELECT json_object('decisionId',d.id,'title',COALESCE((SELECT json_extract(body_json,'$.title') FROM investment_decision_revisions WHERE id=d.current_revision_id),d.title),'draftTitle',(SELECT json_extract(body_json,'$.title') FROM investment_decision_revisions WHERE id=d.draft_revision_id),'currentRevisionNumber',(SELECT revision_number FROM investment_decision_revisions WHERE id=d.current_revision_id),'draftRevisionNumber',(SELECT revision_number FROM investment_decision_revisions WHERE id=d.draft_revision_id),'lifecycle',d.lifecycle,'executionState',d.execution_state,'version',d.version,'currentRevisionId',d.current_revision_id,'draftRevisionId',d.draft_revision_id,'nextReviewAt',d.next_review_at) AS body
            FROM investment_decisions d WHERE (?1 IS NULL OR d.lifecycle=?1)
            AND (json_array_length(?2)=0 OR EXISTS(SELECT 1 FROM investment_decision_revisions r
              WHERE r.decision_id=d.id AND (r.status='CONFIRMED' OR r.id=d.draft_revision_id)
              AND NOT EXISTS(SELECT 1 FROM json_each(?2) wanted WHERE NOT EXISTS(
                SELECT 1 FROM json_each(r.scenario_tags_json) tag WHERE tag.value=wanted.value))))
            ORDER BY d.updated_at DESC,d.id LIMIT ?3 OFFSET ?4"#)
            .bind::<Nullable<Text>,_>(query.lifecycle).bind::<Text,_>(tags).bind::<Integer,_>(limit as i32).bind::<diesel::sql_types::BigInt,_>(i64::from(query.offset.unwrap_or(0))).load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(json_error))
            .collect()
    }

    pub async fn save_decision_draft(
        &self,
        mut input: SaveDecisionDraft,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        input.body.normalize()?;
        if input.decision_id.is_some() != input.expected_version.is_some()
            || input.expected_version.is_some_and(|v| v <= 0)
            || (input.decision_id.is_none() && input.draft_revision_id.is_some())
        {
            return Err(invalid(
                "editing a decision requires decisionId and expectedVersion",
            ));
        }
        let record = self.request(
            "investment_decision_draft_save",
            &input.request_key,
            &input,
            caller,
        )?;
        let (day, timezone) = self.business_day()?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store, move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok(result); }
            let at=now();
            let (id,draft,number,version,old_check)=if let Some(id)=&input.decision_id {
                let root=decision_root(conn,id)?;
                if root.version!=input.expected_version.unwrap_or_default() || root.lifecycle=="ARCHIVED" || root.draft_revision_id!=input.draft_revision_id {
                    return Err(JournalError::StateConflict);
                }
                if let Some(draft)=&root.draft_revision_id {
                    let row=revision_row(conn,draft)?;
                    if row.status!="DRAFT" || row.decision_id!=root.id { return Err(JournalError::StateConflict); }
                    (id.clone(),draft.clone(),row.revision_number,increment(root.version)?,row.next_check_at)
                } else {
                    #[derive(QueryableByName)] struct Number { #[diesel(sql_type=Integer)] number:i32 }
                    let number=diesel::sql_query("SELECT COALESCE(MAX(revision_number),0) AS number FROM investment_decision_revisions WHERE decision_id=?1")
                        .bind::<Text,_>(id).get_result::<Number>(conn)?.number;
                    (id.clone(),uuid::Uuid::new_v4().to_string(),increment(number)?,increment(root.version)?,None)
                }
            } else {
                let id=uuid::Uuid::new_v4().to_string();
                diesel::sql_query("INSERT INTO investment_decisions(id,title,lifecycle,execution_state,created_at,updated_at) VALUES(?1,?2,'DRAFT','NOT_APPLICABLE',?3,?3)")
                    .bind::<Text,_>(&id).bind::<Text,_>(&input.body.title).bind::<Text,_>(&at).execute(conn)?;
                (id,uuid::Uuid::new_v4().to_string(),1,1,None)
            };
            validate_purpose_references(conn,&input.body)?;
            let check=match &input.check_schedule { CheckScheduleChange::Keep=>old_check,CheckScheduleChange::Set{date}=>Some(date.to_string()),CheckScheduleChange::Cancel=>None };
            let payload=json!({"id":draft,"decisionId":id,"number":number,"body":input.body,"nextCheckAt":check,"at":at,"channel":caller.channel.as_str(),"actorKind":caller.actor_kind.as_str(),"actorId":caller.actor_id,"credentialId":caller.credential_id}).to_string();
            if input.draft_revision_id.is_some() {
                diesel::sql_query(UPDATE_REVISION).bind::<Text,_>(&payload).execute(conn)?;
            } else {
                diesel::sql_query(INSERT_REVISION).bind::<Text,_>(&payload).execute(conn)?;
            }
            save_children(conn,&draft,&input.body)?;
            diesel::sql_query("UPDATE investment_decisions SET title=?1,draft_revision_id=?2,version=?3,updated_at=?4 WHERE id=?5")
                .bind::<Text,_>(&input.body.title).bind::<Text,_>(&draft).bind::<Integer,_>(version).bind::<Text,_>(&at).bind::<Text,_>(&id).execute(conn)?;
            let result=json!({"decisionId":id,"draftRevisionId":draft,"revisionNumber":number,"version":version});
            complete(conn,&record,&caller,&ledger,&id,result,day,&timezone)
        }).await
    }

    pub async fn preview_decision_confirmation(
        &self,
        target: DecisionDraftTarget,
        caller: &JournalCaller,
    ) -> JournalResult<DecisionConfirmationPreview> {
        caller.require_read()?;
        let (day, timezone) = self.business_day()?;
        let (body, number, next_check_at, content_hash) = {
            let mut conn = get_connection(self.store.pool())?;
            let row = validate_target(&mut conn, &target)?;
            let body: DecisionBody = serde_json::from_str(&row.body_json).map_err(json_error)?;
            body.validate_confirmation()?;
            validate_purpose_references(&mut conn, &body)?;
            let hash = decision_content_hash(&body, &row.next_check_at)?;
            let check = row.next_check_at.as_deref().map(parse_date).transpose()?;
            (body, row.revision_number, check, hash)
        };
        let reader = self
            .context_reader
            .as_ref()
            .ok_or(JournalError::ResourceUnavailable)?;
        let query = InvestmentContextQuery::for_decision(&body);
        let mut context = reader.capture(&query, day).await?;
        context.validate_envelope(&query, day)?;
        assessment::attach(&mut context, &body)?;
        {
            let mut conn = get_connection(self.store.pool())?;
            attach_principles(&mut conn, &mut context, &body, &timezone)?;
        }
        let expected_context_hash = context.stable_hash()?;
        // A draft can change while native context services are collecting their blocks.
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            validate_target(conn, &target)?;
            validate_purpose_references(conn, &body)?;
            validate_principle_context(conn, &context, &body, &timezone)?;
            reader.validate(conn, &context)
        })?;
        Ok(DecisionConfirmationPreview {
            schema_version: 1,
            ledger_id: self.ledger_id.clone(),
            target,
            revision_number: number,
            body,
            next_check_at,
            expected_content_hash: content_hash,
            expected_context_hash,
            context,
        })
    }

    pub async fn confirm_decision_draft(
        &self,
        input: ConfirmDecisionDraft,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_confirm()?;
        if input.ledger_id != self.ledger_id {
            return Err(JournalError::PermissionDenied);
        }
        let record = self.request(
            "investment_decision_confirm",
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
        let (day, timezone) = self.business_day()?;
        let body = {
            let mut conn = get_connection(self.store.pool())?;
            let row = validate_target(&mut conn, &input.target)?;
            let body: DecisionBody = serde_json::from_str(&row.body_json).map_err(json_error)?;
            if decision_content_hash(&body, &row.next_check_at)? != input.expected_content_hash {
                return Err(JournalError::StateConflict);
            }
            body.validate_confirmation()?;
            body
        };
        let query = InvestmentContextQuery::for_decision(&body);
        input.context.validate_envelope(&query, day)?;
        if input.context.stable_hash()? != input.expected_context_hash {
            return Err(JournalError::StateConflict);
        }
        let reader = self
            .context_reader
            .as_ref()
            .ok_or(JournalError::ResourceUnavailable)?
            .clone();
        let mut current = reader.capture(&query, day).await?;
        current.validate_envelope(&query, day)?;
        assessment::attach(&mut current, &body)?;
        {
            let mut conn = get_connection(self.store.pool())?;
            attach_principles(&mut conn, &mut current, &body, &timezone)?;
        }
        if current.stable_hash()? != input.expected_context_hash {
            return Err(JournalError::StateConflict);
        }
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok(result); }
            let row=validate_target(conn,&input.target)?;
            let body:DecisionBody=serde_json::from_str(&row.body_json).map_err(json_error)?;
            if decision_content_hash(&body,&row.next_check_at)?!=input.expected_content_hash { return Err(JournalError::StateConflict); }
            body.validate_confirmation()?; validate_purpose_references(conn,&body)?; reader.validate(conn,&current)?;
            validate_principle_context(conn,&current,&body,&timezone)?;
            let at=now();
            let data=json!({"context":input.context,"contentHash":input.expected_content_hash,"contextHash":input.expected_context_hash,"at":at,"channel":caller.channel.as_str(),"actorKind":caller.actor_kind.as_str(),"actorId":caller.actor_id,"credentialId":caller.credential_id}).to_string();
            diesel::sql_query("UPDATE investment_decision_revisions SET status='CONFIRMED',context_snapshot_json=json_extract(?1,'$.context'),content_hash=json_extract(?1,'$.contentHash'),context_hash=json_extract(?1,'$.contextHash'),confirmed_at=json_extract(?1,'$.at'),confirmed_channel=json_extract(?1,'$.channel'),confirmed_actor_kind=json_extract(?1,'$.actorKind'),confirmed_actor_id=json_extract(?1,'$.actorId'),confirmed_credential_id=json_extract(?1,'$.credentialId') WHERE id=?2 AND status='DRAFT'")
                .bind::<Text,_>(data).bind::<Text,_>(&row.id).execute(conn)?;
            let state=if body.action.is_some_and(|a|a.is_trade()) { "OPEN" } else { "NOT_APPLICABLE" };
            let version=increment(input.target.expected_version)?;
            // A draft check is not a review schedule. Existing explicit review dates survive a revision.
            diesel::sql_query("UPDATE investment_decisions SET lifecycle='CONFIRMED',current_revision_id=?1,draft_revision_id=NULL,execution_state=?2,close_reason=NULL,version=?3,updated_at=?4 WHERE id=?5")
                .bind::<Text,_>(&row.id).bind::<Text,_>(state).bind::<Integer,_>(version).bind::<Text,_>(&at).bind::<Text,_>(&input.target.decision_id).execute(conn)?;
            let result=json!({"decisionId":input.target.decision_id,"currentRevisionId":row.id,"revisionNumber":row.revision_number,"version":version,"confirmedAt":at});
            complete(conn,&record,&caller,&ledger,&input.target.decision_id,result,day,&timezone)
        }).await
    }

    pub async fn mutate_decision(
        &self,
        input: MutateDecision,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        if input.expected_version <= 0 {
            return Err(invalid("expectedVersion must be positive"));
        }
        let record = self.request(
            "investment_decision_update",
            &input.request_key,
            &input,
            caller,
        )?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        let (day, timezone) = self.business_day()?;
        transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok(result); }
            let root=decision_root(conn,&input.decision_id)?;
            if root.version!=input.expected_version { return Err(JournalError::StateConflict); }
            let mut lifecycle=root.lifecycle.clone(); let mut draft=root.draft_revision_id; let mut state=root.execution_state;
            let mut review=root.next_review_at; let mut close_reason=None;
            match &input.mutation {
                DecisionMutation::DiscardDraft{draft_revision_id} => {
                    if draft.as_deref()!=Some(draft_revision_id) || lifecycle=="ARCHIVED" { return Err(JournalError::StateConflict); }
                    diesel::sql_query("UPDATE investment_decision_revisions SET status='DISCARDED' WHERE id=?1 AND status='DRAFT'")
                        .bind::<Text,_>(draft_revision_id).execute(conn)?; draft=None;
                    if root.current_revision_id.is_none() { lifecycle="ARCHIVED".into(); }
                },
                DecisionMutation::Archive => lifecycle="ARCHIVED".into(),
                DecisionMutation::Restore => { if lifecycle!="ARCHIVED" { return Err(JournalError::StateConflict); } lifecycle=if root.current_revision_id.is_some(){"CONFIRMED"}else{"DRAFT"}.into(); },
                DecisionMutation::Close{reason} => { nonblank(reason,"close reason")?; if lifecycle!="CONFIRMED" || state!="OPEN" { return Err(JournalError::StateConflict); } state="CLOSED".into(); close_reason=Some(reason.clone()); },
                DecisionMutation::Reopen => { if lifecycle!="CONFIRMED" || state!="CLOSED" { return Err(JournalError::StateConflict); } state="OPEN".into(); },
                DecisionMutation::SetReviewDate{date} => { if lifecycle!="CONFIRMED" { return Err(JournalError::StateConflict); } review=Some(date.to_string()); },
                DecisionMutation::CancelReview => review=None,
            }
            // Preserve the explicit close reason for mutations unrelated to execution state.
            let payload=json!({"lifecycle":lifecycle,"draft":draft,"state":state,"review":review,"reason":close_reason,"replaceReason":matches!(input.mutation,DecisionMutation::Close{..}|DecisionMutation::Reopen)}).to_string();
            let version=increment(root.version)?;
            diesel::sql_query("UPDATE investment_decisions SET lifecycle=json_extract(?1,'$.lifecycle'),draft_revision_id=json_extract(?1,'$.draft'),execution_state=json_extract(?1,'$.state'),next_review_at=json_extract(?1,'$.review'),close_reason=CASE WHEN json_extract(?1,'$.replaceReason')=1 THEN json_extract(?1,'$.reason') ELSE close_reason END,version=?2,updated_at=?3 WHERE id=?4")
                .bind::<Text,_>(payload).bind::<Integer,_>(version).bind::<Text,_>(now()).bind::<Text,_>(&root.id).execute(conn)?;
            complete(conn,&record,&caller,&ledger,&root.id,json!({"decisionId":root.id,"version":version}),day,&timezone)
        }).await
    }

    pub fn decision_draft_checks(&self, caller: &JournalCaller) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        let (day, _) = self.business_day()?;
        let mut conn = get_connection(self.store.pool())?;
        let rows=diesel::sql_query("SELECT json_object('id','draft-check:'||r.id,'kind','DRAFT_CHECK_DUE','decisionId',d.id,'revisionId',r.id,'revisionNumber',r.revision_number,'title',d.title,'dueOn',r.next_check_at,'version',d.version) AS body FROM investment_decisions d JOIN investment_decision_revisions r ON r.id=d.draft_revision_id WHERE d.lifecycle<>'ARCHIVED' AND r.status='DRAFT' AND r.next_check_at<=?1 ORDER BY r.next_check_at,d.id")
            .bind::<Text,_>(day.to_string()).load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(json_error))
            .collect()
    }

    pub fn decision_scenario_tags(&self, caller: &JournalCaller) -> JournalResult<Vec<String>> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        #[derive(QueryableByName)]
        struct Tag {
            #[diesel(sql_type=Text)]
            tag: String,
        }
        let rows=diesel::sql_query("SELECT DISTINCT CAST(t.value AS TEXT) AS tag FROM investment_decision_revisions r,json_each(r.scenario_tags_json) t WHERE r.status<>'DISCARDED' ORDER BY tag").load::<Tag>(&mut conn)?;
        Ok(rows.into_iter().map(|r| r.tag).collect())
    }
}

fn increment(version: i32) -> JournalResult<i32> {
    version
        .checked_add(1)
        .ok_or_else(|| invalid("version is outside the supported range"))
}
fn parse_date(date: &str) -> JournalResult<NaiveDate> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| invalid("invalid stored decision date"))
}
fn decision_content_hash(body: &DecisionBody, check: &Option<String>) -> JournalResult<String> {
    Ok(crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"body":body,"nextCheckAt":check}),
    )))
}
fn decision_root(conn: &mut SqliteConnection, id: &str) -> JournalResult<DecisionRoot> {
    diesel::sql_query("SELECT id,lifecycle,execution_state,current_revision_id,draft_revision_id,next_review_at,version FROM investment_decisions WHERE id=?1").bind::<Text,_>(id).get_result(conn).optional()?.ok_or(JournalError::StateConflict)
}
fn revision_row(conn: &mut SqliteConnection, id: &str) -> JournalResult<RevisionRow> {
    diesel::sql_query("SELECT id,decision_id,revision_number,status,body_json,next_check_at FROM investment_decision_revisions WHERE id=?1").bind::<Text,_>(id).get_result(conn).optional()?.ok_or(JournalError::StateConflict)
}
fn validate_target(
    conn: &mut SqliteConnection,
    target: &DecisionDraftTarget,
) -> JournalResult<RevisionRow> {
    let root = decision_root(conn, &target.decision_id)?;
    if root.version != target.expected_version
        || root.lifecycle == "ARCHIVED"
        || root.draft_revision_id.as_deref() != Some(&target.draft_revision_id)
    {
        return Err(JournalError::StateConflict);
    }
    let row = revision_row(conn, &target.draft_revision_id)?;
    if row.status != "DRAFT" || row.decision_id != root.id {
        return Err(JournalError::StateConflict);
    }
    Ok(row)
}
fn validate_purpose_references(
    conn: &mut SqliteConnection,
    body: &DecisionBody,
) -> JournalResult<()> {
    let purposes = purpose_rows(conn)?;
    for id in body
        .purpose_ids
        .iter()
        .chain(body.funding.iter().map(|f| &f.purpose_id))
    {
        if !purposes
            .iter()
            .any(|p| p["id"].as_str() == Some(id) && p["isArchived"] == false)
        {
            return Err(invalid("decision purpose is missing or archived"));
        }
    }
    for evidence in &body.evidence {
        if let Some(id) = &evidence.purpose_id {
            if !purposes.iter().any(|p| {
                p["id"].as_str() == Some(id)
                    && p["currency"].as_str() == evidence.loss_currency.as_deref()
            }) {
                return Err(invalid(
                    "loss scenario currency must match the purpose budget currency",
                ));
            }
        }
    }
    Ok(())
}
fn decision_detail(conn: &mut SqliteConnection, id: &str) -> JournalResult<Value> {
    let row=diesel::sql_query(r#"SELECT json_object('decisionId',d.id,'title',d.title,'lifecycle',d.lifecycle,'executionState',d.execution_state,'closeReason',d.close_reason,'currentRevisionId',d.current_revision_id,'draftRevisionId',d.draft_revision_id,'nextReviewAt',d.next_review_at,'version',d.version,'createdAt',d.created_at,'updatedAt',d.updated_at,
        'revisions',json((SELECT json_group_array(json_object('id',r.id,'revisionNumber',r.revision_number,'status',r.status,'body',json(r.body_json),'nextCheckAt',r.next_check_at,'context',json(r.context_snapshot_json),'contentHash',r.content_hash,'contextHash',r.context_hash,'createdAt',r.created_at,'updatedAt',r.updated_at,'confirmedAt',r.confirmed_at,
        'createdChannel',r.created_channel,'createdActorKind',r.created_actor_kind,'createdActorId',r.created_actor_id,'createdCredentialId',r.created_credential_id,'updatedChannel',r.updated_channel,'updatedActorKind',r.updated_actor_kind,'updatedActorId',r.updated_actor_id,'updatedCredentialId',r.updated_credential_id,'contentOrigin',r.content_origin,'confirmedChannel',r.confirmed_channel,'confirmedActorKind',r.confirmed_actor_kind,'confirmedActorId',r.confirmed_actor_id,'confirmedCredentialId',r.confirmed_credential_id)) FROM (SELECT * FROM investment_decision_revisions WHERE decision_id=d.id ORDER BY revision_number) r))) AS body FROM investment_decisions d WHERE d.id=?1"#)
        .bind::<Text,_>(id).get_result::<JsonRow>(conn).optional()?.ok_or(JournalError::StateConflict)?;
    serde_json::from_str(&row.body).map_err(json_error)
}

fn save_children(
    conn: &mut SqliteConnection,
    revision: &str,
    body: &DecisionBody,
) -> JournalResult<()> {
    super::principle_references::snapshots(conn, &body.principle_revision_ids, true)?;
    diesel::sql_query("DELETE FROM investment_decision_principles WHERE decision_revision_id=?1")
        .bind::<Text, _>(revision)
        .execute(conn)?;
    for id in &body.principle_revision_ids {
        diesel::sql_query("INSERT INTO investment_decision_principles(id,decision_revision_id,principle_revision_id) VALUES(?1,?2,?3)")
            .bind::<Text,_>(uuid::Uuid::new_v4().to_string()).bind::<Text,_>(revision).bind::<Text,_>(id).execute(conn)?;
    }
    for table in [
        "investment_decision_options",
        "investment_decision_evidence",
        "investment_decision_funding",
    ] {
        diesel::sql_query(format!("DELETE FROM {table} WHERE revision_id=?1"))
            .bind::<Text, _>(revision)
            .execute(conn)?;
    }
    let body = serde_json::to_string(body).map_err(json_error)?;
    for sql in [INSERT_OPTIONS, INSERT_EVIDENCE, INSERT_FUNDING] {
        diesel::sql_query(sql)
            .bind::<Text, _>(revision)
            .bind::<Text, _>(&body)
            .execute(conn)?;
    }
    Ok(())
}

fn attach_principles(
    conn: &mut SqliteConnection,
    context: &mut InvestmentContextCapture,
    body: &DecisionBody,
    timezone: &str,
) -> JournalResult<()> {
    context.response.funding.data["decisionReferences"] =
        decision_references(conn, body, context.response.as_of_date, timezone)?;
    if !body.principle_revision_ids.is_empty() {
        context.response.funding.data["principleVersions"] = json!(
            super::principle_references::snapshots(conn, &body.principle_revision_ids, true)?
        );
    }
    Ok(())
}
fn validate_principle_context(
    conn: &mut SqliteConnection,
    context: &InvestmentContextCapture,
    body: &DecisionBody,
    timezone: &str,
) -> JournalResult<()> {
    if context.response.funding.data["decisionReferences"]
        != decision_references(conn, body, context.response.as_of_date, timezone)?
    {
        return Err(JournalError::StateConflict);
    }
    if !body.principle_revision_ids.is_empty()
        && context.response.funding.data["principleVersions"]
            != json!(super::principle_references::snapshots(
                conn,
                &body.principle_revision_ids,
                true
            )?)
    {
        return Err(JournalError::StateConflict);
    }
    Ok(())
}

fn decision_references(
    conn: &mut SqliteConnection,
    body: &DecisionBody,
    day: NaiveDate,
    timezone: &str,
) -> JournalResult<Value> {
    let accounts: Vec<_> = body
        .funding
        .iter()
        .map(|line| line.resource.account_id())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let assets: Vec<_> = body
        .asset_id
        .iter()
        .map(String::as_str)
        .chain(body.funding.iter().filter_map(|line| match &line.resource {
            FundResource::Security { asset_id, .. } => Some(asset_id.as_str()),
            _ => None,
        }))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let accounts=diesel::sql_query("SELECT json_object('id',id,'name',name,'currency',currency) AS body FROM accounts WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY id").bind::<Text,_>(serde_json::to_string(&accounts).map_err(json_error)?).load::<JsonRow>(conn)?.into_iter().map(|row|serde_json::from_str::<Value>(&row.body).map_err(json_error)).collect::<JournalResult<Vec<_>>>()?;
    let mut assets=diesel::sql_query("SELECT json_object('id',id,'name',name,'displayCode',display_code,'currency',quote_ccy) AS body FROM assets WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY id").bind::<Text,_>(serde_json::to_string(&assets).map_err(json_error)?).load::<JsonRow>(conn)?.into_iter().map(|row|serde_json::from_str::<Value>(&row.body).map_err(json_error)).collect::<JournalResult<Vec<_>>>()?;
    for asset in &mut assets {
        asset["unitBasis"] = super::links::asset_unit_basis(
            conn,
            asset["id"].as_str().unwrap_or_default(),
            day,
            timezone,
        )?;
    }
    Ok(json!({"accounts":accounts,"assets":assets,"unitTimezone":timezone}))
}

const INSERT_OPTIONS: &str = r#"INSERT INTO investment_decision_options(id,revision_id,position,action,description,advantages,disadvantages,not_selected_reason)
 SELECT ?1||':option:'||key,?1,key,json_extract(value,'$.action'),json_extract(value,'$.description'),json_extract(value,'$.advantages'),json_extract(value,'$.disadvantages'),json_extract(value,'$.notSelectedReason') FROM json_each(?2,'$.options')"#;
const INSERT_EVIDENCE: &str = r#"INSERT INTO investment_decision_evidence(id,revision_id,position,information_kind,content,source,probability_lower_bps,probability_upper_bps,probability_basis,purpose_id,assumed_loss_amount,loss_currency,loss_basis)
 SELECT ?1||':evidence:'||key,?1,key,json_extract(value,'$.informationKind'),json_extract(value,'$.content'),json_extract(value,'$.source'),json_extract(value,'$.probabilityLowerBps'),json_extract(value,'$.probabilityUpperBps'),json_extract(value,'$.probabilityBasis'),json_extract(value,'$.purposeId'),json_extract(value,'$.assumedLossAmount'),json_extract(value,'$.lossCurrency'),json_extract(value,'$.lossBasis') FROM json_each(?2,'$.evidence')"#;
const INSERT_FUNDING: &str = r#"INSERT INTO investment_decision_funding(id,revision_id,position,purpose_id,resource_kind,account_id,currency,asset_id,planned_amount,planned_quantity)
 SELECT ?1||':funding:'||key,?1,key,json_extract(value,'$.purposeId'),json_extract(value,'$.resource.kind'),json_extract(value,'$.resource.accountId'),json_extract(value,'$.resource.currency'),json_extract(value,'$.resource.assetId'),CASE WHEN json_extract(value,'$.resource.kind')='CASH' THEN json_extract(value,'$.planned') END,CASE WHEN json_extract(value,'$.resource.kind')='SECURITY' THEN json_extract(value,'$.planned') END FROM json_each(?2,'$.funding')"#;

const INSERT_REVISION: &str = r#"INSERT INTO investment_decision_revisions(id,decision_id,revision_number,action,asset_id,planned_amount,planned_quantity,currency,thesis,accepted_disadvantages,information_value,waiting_cost,declared_at,funding_status,scenario_tags_json,body_json,next_check_at,content_origin,created_at,created_channel,created_actor_kind,created_actor_id,created_credential_id,updated_at,updated_channel,updated_actor_kind,updated_actor_id,updated_credential_id,status)
 VALUES(json_extract(?1,'$.id'),json_extract(?1,'$.decisionId'),json_extract(?1,'$.number'),json_extract(?1,'$.body.action'),json_extract(?1,'$.body.assetId'),json_extract(?1,'$.body.plannedAmount'),json_extract(?1,'$.body.plannedQuantity'),json_extract(?1,'$.body.currency'),json_extract(?1,'$.body.thesis'),json_extract(?1,'$.body.acceptedDisadvantages'),json_extract(?1,'$.body.informationValue'),json_extract(?1,'$.body.waitingCost'),json_extract(?1,'$.body.declaredAt'),json_extract(?1,'$.body.fundingStatus'),json_extract(?1,'$.body.scenarioTags'),json_extract(?1,'$.body'),json_extract(?1,'$.nextCheckAt'),json_extract(?1,'$.body.contentOrigin'),json_extract(?1,'$.at'),json_extract(?1,'$.channel'),json_extract(?1,'$.actorKind'),json_extract(?1,'$.actorId'),json_extract(?1,'$.credentialId'),json_extract(?1,'$.at'),json_extract(?1,'$.channel'),json_extract(?1,'$.actorKind'),json_extract(?1,'$.actorId'),json_extract(?1,'$.credentialId'),'DRAFT')"#;
const UPDATE_REVISION: &str = r#"UPDATE investment_decision_revisions SET action=json_extract(?1,'$.body.action'),asset_id=json_extract(?1,'$.body.assetId'),planned_amount=json_extract(?1,'$.body.plannedAmount'),planned_quantity=json_extract(?1,'$.body.plannedQuantity'),currency=json_extract(?1,'$.body.currency'),thesis=json_extract(?1,'$.body.thesis'),accepted_disadvantages=json_extract(?1,'$.body.acceptedDisadvantages'),information_value=json_extract(?1,'$.body.informationValue'),waiting_cost=json_extract(?1,'$.body.waitingCost'),declared_at=json_extract(?1,'$.body.declaredAt'),funding_status=json_extract(?1,'$.body.fundingStatus'),scenario_tags_json=json_extract(?1,'$.body.scenarioTags'),body_json=json_extract(?1,'$.body'),next_check_at=json_extract(?1,'$.nextCheckAt'),content_origin=json_extract(?1,'$.body.contentOrigin'),updated_at=json_extract(?1,'$.at'),updated_channel=json_extract(?1,'$.channel'),updated_actor_kind=json_extract(?1,'$.actorKind'),updated_actor_id=json_extract(?1,'$.actorId'),updated_credential_id=json_extract(?1,'$.credentialId') WHERE id=json_extract(?1,'$.id') AND status='DRAFT'"#;
