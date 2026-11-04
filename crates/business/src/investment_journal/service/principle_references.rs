//! Explicit version references; a shared scenario tag never creates a causal link.
use super::*;
pub(super) fn snapshots(
    conn: &mut SqliteConnection,
    ids: &[String],
    effective_only: bool,
) -> JournalResult<Vec<Value>> {
    ids.iter().map(|id| {
        let row=diesel::sql_query("SELECT json_object('id',r.id,'principleId',r.principle_id,'revisionNumber',r.revision_number,'statusAtCapture',r.status,'body',json(r.body_json),'contentHash',r.content_hash,'contentOrigin',r.content_origin,'createdChannel',r.created_channel,'createdActorKind',r.created_actor_kind,'createdActorId',r.created_actor_id,'createdCredentialId',r.created_credential_id,'publishedAt',r.published_at,'publishedChannel',r.published_channel,'publishedActorKind',r.published_actor_kind,'publishedActorId',r.published_actor_id,'publishedCredentialId',r.published_credential_id,'sourceReviewSnapshot',json(r.source_review_snapshot_json),'currentRevisionIdAtCapture',p.current_revision_id,'archivedAtCapture',json(CASE WHEN p.is_archived=1 THEN 'true' ELSE 'false' END)) AS body FROM investment_principle_revisions r JOIN investment_principles p ON p.id=r.principle_id WHERE r.id=?1")
            .bind::<Text,_>(id).get_result::<JsonRow>(conn).optional()?.ok_or_else(||invalid("principle version not found"))?;
        let value:Value=serde_json::from_str(&row.body).map_err(json_error)?;
        if effective_only && value["statusAtCapture"]!="EFFECTIVE" { return Err(invalid("decision requires a published principle version")); }
        Ok(value)
    }).collect()
}
