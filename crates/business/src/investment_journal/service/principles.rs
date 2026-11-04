//! Append-only principle drafts and independently authorized publication.
use super::*;
use crate::investment_journal::{
    ArchivePrinciple, DiscardPrincipleDraft, PrincipleBody, PrincipleDraftTarget, PrincipleQuery,
    PublishPrinciple, SavePrincipleDraft,
};
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinciplePublicationPreview {
    pub schema_version: u32,
    pub ledger_id: String,
    pub target: PrincipleDraftTarget,
    pub revision_number: i32,
    pub body: PrincipleBody,
    pub expected_content_hash: String,
    pub expected_source_context_hash: String,
    pub source_context: Value,
}
#[derive(QueryableByName)]
struct Root {
    #[diesel(sql_type=Text)]
    id: String,
    #[diesel(sql_type=Nullable<Text>)]
    current_revision_id: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    draft_revision_id: Option<String>,
    #[diesel(sql_type=Integer)]
    is_archived: i32,
    #[diesel(sql_type=Integer)]
    version: i32,
}
#[derive(QueryableByName)]
struct Revision {
    #[diesel(sql_type=Text)]
    id: String,
    #[diesel(sql_type=Text)]
    principle_id: String,
    #[diesel(sql_type=Integer)]
    revision_number: i32,
    #[diesel(sql_type=Text)]
    status: String,
    #[diesel(sql_type=Text)]
    body_json: String,
}
impl InvestmentJournalService {
    pub fn get_principle(&self, id: &str, caller: &JournalCaller) -> JournalResult<Value> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        detail(&mut conn, id)
    }
    pub fn list_principles(
        &self,
        mut query: PrincipleQuery,
        caller: &JournalCaller,
    ) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        query.scenario_tags = super::super::normalize_scenario_tags(&query.scenario_tags)?;
        let limit = query.limit.unwrap_or(100);
        if !(1..=500).contains(&limit) {
            return Err(invalid("limit must be between 1 and 500"));
        }
        let tags = serde_json::to_string(&query.scenario_tags).map_err(json_error)?;
        let mut conn = get_connection(self.store.pool())?;
        let rows=diesel::sql_query("SELECT json_object('principleId',p.id,'name',COALESCE((SELECT name FROM investment_principle_revisions WHERE id=p.current_revision_id),p.name),'draftName',(SELECT name FROM investment_principle_revisions WHERE id=p.draft_revision_id),'currentRevisionNumber',(SELECT revision_number FROM investment_principle_revisions WHERE id=p.current_revision_id),'currentRevisionId',p.current_revision_id,'draftRevisionId',p.draft_revision_id,'isArchived',json(CASE WHEN p.is_archived=1 THEN 'true' ELSE 'false' END),'version',p.version) AS body FROM investment_principles p WHERE (?1=1 OR p.is_archived=0) AND (json_array_length(?2)=0 OR EXISTS(SELECT 1 FROM investment_principle_revisions r WHERE r.principle_id=p.id AND (r.status='EFFECTIVE' OR r.id=p.draft_revision_id) AND NOT EXISTS(SELECT 1 FROM json_each(?2) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(r.scenario_tags_json) tag WHERE tag.value=wanted.value)))) ORDER BY p.updated_at DESC,p.id LIMIT ?3 OFFSET ?4")
            .bind::<Integer,_>(i32::from(query.include_archived)).bind::<Text,_>(tags).bind::<Integer,_>(limit as i32).bind::<diesel::sql_types::BigInt,_>(i64::from(query.offset.unwrap_or(0))).load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(json_error))
            .collect()
    }
    pub async fn save_principle_draft(
        &self,
        mut input: SavePrincipleDraft,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        input.body.normalize()?;
        if input.principle_id.is_some() != input.expected_version.is_some()
            || input.expected_version.is_some_and(|v| v <= 0)
            || (input.principle_id.is_none() && input.expected_draft_revision_id.is_some())
        {
            return Err(invalid(
                "editing a principle requires its id, expectedVersion and current draft identity",
            ));
        }
        let record = self.request(
            "investment_principle_draft_save",
            &input.request_key,
            &input,
            caller,
        )?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        let (day, zone) = self.business_day()?;
        transact(&self.store,move|conn|{
            if let Some(result)=replay(conn,&record)?{return Ok(result);}
            let at=now();
            let(id,number,version)=if let Some(id)=&input.principle_id {
                let root=root(conn,id)?;
                if root.version!=input.expected_version.unwrap_or_default()||root.draft_revision_id!=input.expected_draft_revision_id||root.is_archived!=0{return Err(JournalError::StateConflict);}
                if let Some(draft)=&root.draft_revision_id{discard_version(conn,draft,&caller,&at)?;}
                #[derive(QueryableByName)]struct Number{#[diesel(sql_type=Integer)]number:i32}
                let number=diesel::sql_query("SELECT COALESCE(MAX(revision_number),0) AS number FROM investment_principle_revisions WHERE principle_id=?1").bind::<Text,_>(id).get_result::<Number>(conn)?.number;
                (id.clone(),increment(number)?,increment(root.version)?)
            }else{
                let id=uuid::Uuid::new_v4().to_string();diesel::sql_query("INSERT INTO investment_principles(id,name,created_at,updated_at) VALUES(?1,?2,?3,?3)").bind::<Text,_>(&id).bind::<Text,_>(&input.body.name).bind::<Text,_>(&at).execute(conn)?;(id,1,1)
            };
            source_context(conn,&input.body)?;
            let draft=uuid::Uuid::new_v4().to_string();let data=payload(&input.body,&caller,&at,json!({"id":draft,"principleId":id,"number":number}));
            diesel::sql_query(INSERT_DRAFT).bind::<Text,_>(data.to_string()).execute(conn)?;
            diesel::sql_query("UPDATE investment_principles SET name=?1,draft_revision_id=?2,version=?3,updated_at=?4 WHERE id=?5")
                .bind::<Text,_>(&input.body.name).bind::<Text,_>(&draft).bind::<Integer,_>(version).bind::<Text,_>(&at).bind::<Text,_>(&id).execute(conn)?;
            complete(conn,&record,&caller,&ledger,&id,json!({"principleId":id,"draftRevisionId":draft,"revisionNumber":number,"version":version}),day,&zone)
        }).await
    }
    pub fn preview_principle_publication(
        &self,
        target: PrincipleDraftTarget,
        caller: &JournalCaller,
    ) -> JournalResult<PrinciplePublicationPreview> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            let (root, row, body) = target_row(conn, &target)?;
            body.validate_publication(root.current_revision_id.is_some())?;
            let context = source_context(conn, &body)?;
            Ok(PrinciplePublicationPreview {
                schema_version: 1,
                ledger_id: self.ledger_id.clone(),
                target,
                revision_number: row.revision_number,
                expected_content_hash: body_hash(&body)?,
                expected_source_context_hash: hash(&context),
                source_context: context,
                body,
            })
        })
    }
    pub async fn publish_principle(
        &self,
        input: PublishPrinciple,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_confirm()?;
        if input.ledger_id != self.ledger_id {
            return Err(JournalError::PermissionDenied);
        }
        let record = self.request(
            "investment_principle_publish",
            &input.request_key,
            &input,
            caller,
        )?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        let (day, zone) = self.business_day()?;
        transact(&self.store,move|conn|{
            if let Some(result)=replay(conn,&record)?{return Ok(result);}
            let(root,row,body)=target_row(conn,&input.target)?;body.validate_publication(root.current_revision_id.is_some())?;
            let context=source_context(conn,&body)?;
            if body_hash(&body)?!=input.expected_content_hash||hash(&input.source_context)!=input.expected_source_context_hash||hash(&context)!=input.expected_source_context_hash{return Err(JournalError::StateConflict);}
            let at=now();let request_id=uuid::Uuid::new_v4().to_string();let version=increment(root.version)?;
            let data=payload(&body,&caller,&at,json!({"contentHash":input.expected_content_hash,"sourceHash":input.expected_source_context_hash,"sourceContext":context,"requestId":request_id})).to_string();
            diesel::sql_query("UPDATE investment_principle_revisions SET status='EFFECTIVE',source_review_snapshot_json=json_extract(?1,'$.sourceContext'),content_hash=json_extract(?1,'$.contentHash'),source_context_hash=json_extract(?1,'$.sourceHash'),published_at=json_extract(?1,'$.at'),published_channel=json_extract(?1,'$.channel'),published_actor_kind=json_extract(?1,'$.actorKind'),published_actor_id=json_extract(?1,'$.actorId'),published_credential_id=json_extract(?1,'$.credentialId'),published_request_id=json_extract(?1,'$.requestId') WHERE id=?2 AND status='DRAFT'")
                .bind::<Text,_>(data).bind::<Text,_>(&row.id).execute(conn)?;
            diesel::sql_query("UPDATE investment_principles SET current_revision_id=?1,draft_revision_id=NULL,version=?2,updated_at=?3 WHERE id=?4")
                .bind::<Text,_>(&row.id).bind::<Integer,_>(version).bind::<Text,_>(&at).bind::<Text,_>(&root.id).execute(conn)?;
            complete_with_id(conn,&record,&caller,&ledger,&root.id,json!({"principleId":root.id,"currentRevisionId":row.id,"revisionNumber":row.revision_number,"version":version,"publishedAt":at}),day,&zone,&request_id)
        }).await
    }
    pub async fn archive_principle(
        &self,
        input: ArchivePrinciple,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        let record = self.request(
            "investment_principle_archive",
            &input.request_key,
            &input,
            caller,
        )?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        let (day, zone) = self.business_day()?;
        transact(&self.store,move|conn|{
            if let Some(result)=replay(conn,&record)?{return Ok(result);}
            let root=root(conn,&input.principle_id)?;if root.version!=input.expected_version{return Err(JournalError::StateConflict);}
            let version=increment(root.version)?;diesel::sql_query("UPDATE investment_principles SET is_archived=?1,version=?2,updated_at=?3 WHERE id=?4")
                .bind::<Integer,_>(i32::from(input.is_archived)).bind::<Integer,_>(version).bind::<Text,_>(now()).bind::<Text,_>(&root.id).execute(conn)?;
            complete(conn,&record,&caller,&ledger,&root.id,json!({"principleId":root.id,"version":version,"isArchived":input.is_archived}),day,&zone)
        }).await
    }
    pub async fn discard_principle_draft(
        &self,
        input: DiscardPrincipleDraft,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        let record = self.request(
            "investment_principle_draft_discard",
            &input.request_key,
            &input,
            caller,
        )?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        let (day, zone) = self.business_day()?;
        transact(&self.store,move|conn|{
            if let Some(result)=replay(conn,&record)?{return Ok(result);}
            let(root,row,_)=target_row(conn,&input.target)?;let at=now();discard_version(conn,&row.id,&caller,&at)?;let version=increment(root.version)?;
            diesel::sql_query("UPDATE investment_principles SET draft_revision_id=NULL,is_archived=CASE WHEN current_revision_id IS NULL THEN 1 ELSE is_archived END,version=?1,updated_at=?2 WHERE id=?3")
                .bind::<Integer,_>(version).bind::<Text,_>(&at).bind::<Text,_>(&root.id).execute(conn)?;
            complete(conn,&record,&caller,&ledger,&root.id,json!({"principleId":root.id,"discardedRevisionId":row.id,"version":version}),day,&zone)
        }).await
    }
}
fn increment(value: i32) -> JournalResult<i32> {
    value
        .checked_add(1)
        .ok_or_else(|| invalid("principle version out of range"))
}
fn hash(value: &Value) -> String {
    crate::requests::hash_json(&crate::requests::canonicalize(value.clone()))
}
fn body_hash(body: &PrincipleBody) -> JournalResult<String> {
    Ok(hash(&serde_json::to_value(body).map_err(json_error)?))
}
fn root(conn: &mut SqliteConnection, id: &str) -> JournalResult<Root> {
    diesel::sql_query("SELECT id,current_revision_id,draft_revision_id,is_archived,version FROM investment_principles WHERE id=?1").bind::<Text,_>(id).get_result(conn).optional()?.ok_or(JournalError::StateConflict)
}
fn target_row(
    conn: &mut SqliteConnection,
    target: &PrincipleDraftTarget,
) -> JournalResult<(Root, Revision, PrincipleBody)> {
    let root = root(conn, &target.principle_id)?;
    if root.version != target.expected_version
        || root.draft_revision_id.as_deref() != Some(&target.draft_revision_id)
        || root.is_archived != 0
    {
        return Err(JournalError::StateConflict);
    }
    let row=diesel::sql_query("SELECT id,principle_id,revision_number,status,body_json FROM investment_principle_revisions WHERE id=?1").bind::<Text,_>(&target.draft_revision_id).get_result::<Revision>(conn)?;
    if row.principle_id != root.id || row.status != "DRAFT" {
        return Err(JournalError::StateConflict);
    }
    let body = serde_json::from_str(&row.body_json).map_err(json_error)?;
    Ok((root, row, body))
}
fn payload(body: &PrincipleBody, caller: &JournalCaller, at: &str, mut data: Value) -> Value {
    data["body"] = json!(body);
    data["at"] = json!(at);
    data["channel"] = json!(caller.channel.as_str());
    data["actorKind"] = json!(caller.actor_kind.as_str());
    data["actorId"] = json!(caller.actor_id);
    data["credentialId"] = json!(caller.credential_id);
    data
}
fn discard_version(
    conn: &mut SqliteConnection,
    id: &str,
    caller: &JournalCaller,
    at: &str,
) -> JournalResult<()> {
    diesel::sql_query("UPDATE investment_principle_revisions SET status='DISCARDED',updated_at=?1,updated_channel=?2,updated_actor_kind=?3,updated_actor_id=?4,updated_credential_id=?5 WHERE id=?6 AND status='DRAFT'")
        .bind::<Text,_>(at).bind::<Text,_>(caller.channel.as_str()).bind::<Text,_>(caller.actor_kind.as_str()).bind::<Text,_>(&caller.actor_id).bind::<Nullable<Text>,_>(&caller.credential_id).bind::<Text,_>(id).execute(conn)?;
    Ok(())
}
fn source_context(conn: &mut SqliteConnection, body: &PrincipleBody) -> JournalResult<Value> {
    let review=body.source_review_id.as_ref().map(|id|{
        let row=diesel::sql_query("SELECT json_object('id',id,'decisionRevisionId',decision_revision_id,'rootId',root_id,'revisionNumber',revision_number,'statusAtCapture',status,'version',version,'observationCutoff',observation_cutoff,'observationTimezone',observation_timezone,'judgment',judgment,'executionAssessment',execution_assessment,'body',json(body_json),'factsSnapshot',json(facts_snapshot_json),'contentOrigin',content_origin,'createdChannel',created_channel,'createdActorKind',created_actor_kind,'createdActorId',created_actor_id,'createdCredentialId',created_credential_id,'updatedChannel',updated_channel,'updatedActorKind',updated_actor_kind,'updatedActorId',updated_actor_id,'updatedCredentialId',updated_credential_id,'submittedAt',submitted_at) AS body FROM investment_reviews WHERE id=?1")
            .bind::<Text,_>(id).get_result::<JsonRow>(conn).optional()?.ok_or_else(||invalid("source review version not found"))?;
        serde_json::from_str::<Value>(&row.body).map_err(json_error)
    }).transpose()?;
    Ok(
        json!({"schemaVersion":1,"sourceReview":review,"sourceReference":body.source_reference,"sourceStatusIsVerification":false}),
    )
}
fn detail(conn: &mut SqliteConnection, id: &str) -> JournalResult<Value> {
    let root=diesel::sql_query("SELECT json_object('principleId',id,'name',name,'currentRevisionId',current_revision_id,'draftRevisionId',draft_revision_id,'isArchived',json(CASE WHEN is_archived=1 THEN 'true' ELSE 'false' END),'version',version,'createdAt',created_at,'updatedAt',updated_at) AS body FROM investment_principles WHERE id=?1")
        .bind::<Text,_>(id).get_result::<JsonRow>(conn).optional()?.ok_or(JournalError::StateConflict)?;
    let mut result: Value = serde_json::from_str(&root.body).map_err(json_error)?;
    let rows=diesel::sql_query("SELECT json_object('id',id,'revisionNumber',revision_number,'status',status,'body',json(body_json),'sourceContext',json(source_review_snapshot_json),'contentHash',content_hash,'sourceContextHash',source_context_hash,'createdAt',created_at,'updatedAt',updated_at,'createdChannel',created_channel,'createdActorKind',created_actor_kind,'createdActorId',created_actor_id,'createdCredentialId',created_credential_id,'updatedChannel',updated_channel,'updatedActorKind',updated_actor_kind,'updatedActorId',updated_actor_id,'updatedCredentialId',updated_credential_id,'contentOrigin',content_origin,'publishedAt',published_at,'publishedChannel',published_channel,'publishedActorKind',published_actor_kind,'publishedActorId',published_actor_id,'publishedCredentialId',published_credential_id) AS body FROM investment_principle_revisions WHERE principle_id=?1 ORDER BY revision_number")
        .bind::<Text,_>(id).load::<JsonRow>(conn)?;
    result["revisions"] = Value::Array(
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(json_error))
            .collect::<JournalResult<Vec<_>>>()?,
    );
    Ok(result)
}
const INSERT_DRAFT: &str = r#"INSERT INTO investment_principle_revisions(id,principle_id,revision_number,status,name,content,applicable_conditions,exceptions,revision_reason,source_review_id,source_reference,scenario_tags_json,body_json,created_at,updated_at,created_channel,created_actor_kind,created_actor_id,created_credential_id,updated_channel,updated_actor_kind,updated_actor_id,updated_credential_id,content_origin)
VALUES(json_extract(?1,'$.id'),json_extract(?1,'$.principleId'),json_extract(?1,'$.number'),'DRAFT',json_extract(?1,'$.body.name'),json_extract(?1,'$.body.content'),json_extract(?1,'$.body.applicableConditions'),json_extract(?1,'$.body.exceptions'),json_extract(?1,'$.body.revisionReason'),json_extract(?1,'$.body.sourceReviewId'),json_extract(?1,'$.body.sourceReference'),json_extract(?1,'$.body.scenarioTags'),json_extract(?1,'$.body'),json_extract(?1,'$.at'),json_extract(?1,'$.at'),json_extract(?1,'$.channel'),json_extract(?1,'$.actorKind'),json_extract(?1,'$.actorId'),json_extract(?1,'$.credentialId'),json_extract(?1,'$.channel'),json_extract(?1,'$.actorKind'),json_extract(?1,'$.actorId'),json_extract(?1,'$.credentialId'),json_extract(?1,'$.body.contentOrigin'))"#;
