-- M3: review/principle history depends on M1, M2 and this group only.
CREATE TABLE investment_reviews (
    id TEXT PRIMARY KEY NOT NULL,
    decision_revision_id TEXT NOT NULL REFERENCES investment_decision_revisions(id),
    root_id TEXT NOT NULL REFERENCES investment_reviews(id) DEFERRABLE INITIALLY DEFERRED,
    revision_number INTEGER NOT NULL CHECK(revision_number>0),
    corrects_review_id TEXT REFERENCES investment_reviews(id),
    status TEXT NOT NULL CHECK(status IN ('DRAFT','SUBMITTED','DISCARDED')),
    observation_cutoff TEXT NOT NULL,
    observation_timezone TEXT NOT NULL DEFAULT 'UTC',
    judgment TEXT NOT NULL,
    execution_assessment TEXT NOT NULL,
    deviation_reason TEXT,
    outcome_evidence_json TEXT NOT NULL CHECK(json_valid(outcome_evidence_json) AND json_type(outcome_evidence_json)='array'),
    other_explanations TEXT,
    open_questions TEXT,
    scenario_tags_json TEXT NOT NULL CHECK(json_valid(scenario_tags_json) AND json_type(scenario_tags_json)='array'),
    body_json TEXT NOT NULL CHECK(json_valid(body_json) AND json_extract(body_json,'$.schemaVersion') IS 1),
    facts_snapshot_json TEXT CHECK(facts_snapshot_json IS NULL OR (json_valid(facts_snapshot_json) AND json_extract(facts_snapshot_json,'$.schemaVersion') IS 1)),
    content_hash TEXT,
    facts_hash TEXT,
    version INTEGER NOT NULL DEFAULT 1 CHECK(version>0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_channel TEXT NOT NULL CHECK(created_channel IN ('gui','cli','mcp')),
    created_actor_kind TEXT NOT NULL CHECK(created_actor_kind IN ('GUI_SESSION','PROGRAM')),
    created_actor_id TEXT NOT NULL,
    created_credential_id TEXT,
    updated_channel TEXT NOT NULL CHECK(updated_channel IN ('gui','cli','mcp')),
    updated_actor_kind TEXT NOT NULL CHECK(updated_actor_kind IN ('GUI_SESSION','PROGRAM')),
    updated_actor_id TEXT NOT NULL,
    updated_credential_id TEXT,
    content_origin TEXT NOT NULL CHECK(content_origin IN ('USER_PROVIDED','ASSISTED_DRAFT','UNSPECIFIED')),
    submitted_at TEXT,
    submitted_channel TEXT CHECK(submitted_channel IN ('gui','cli','mcp')),
    submitted_actor_kind TEXT CHECK(submitted_actor_kind IN ('GUI_SESSION','PROGRAM')),
    submitted_actor_id TEXT,
    submitted_credential_id TEXT,
    submitted_request_id TEXT REFERENCES business_requests(id) DEFERRABLE INITIALLY DEFERRED,
    UNIQUE(root_id,revision_number),
    CHECK(status<>'SUBMITTED' OR (facts_snapshot_json IS NOT NULL AND content_hash IS NOT NULL AND facts_hash IS NOT NULL
      AND submitted_at IS NOT NULL AND submitted_channel IS NOT NULL AND submitted_actor_kind IS NOT NULL AND submitted_actor_id IS NOT NULL AND submitted_request_id IS NOT NULL))
);
CREATE UNIQUE INDEX investment_review_one_root_per_cutoff ON investment_reviews(decision_revision_id,observation_cutoff,observation_timezone) WHERE id=root_id;
CREATE UNIQUE INDEX investment_review_one_draft ON investment_reviews(root_id) WHERE status='DRAFT';
CREATE UNIQUE INDEX investment_review_no_fork ON investment_reviews(corrects_review_id) WHERE status='SUBMITTED' AND corrects_review_id IS NOT NULL;
CREATE UNIQUE INDEX investment_review_one_first_submission ON investment_reviews(root_id) WHERE status='SUBMITTED' AND corrects_review_id IS NULL;
CREATE INDEX investment_reviews_decision ON investment_reviews(decision_revision_id,observation_cutoff);

CREATE TABLE investment_principles (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    current_revision_id TEXT REFERENCES investment_principle_revisions(id) DEFERRABLE INITIALLY DEFERRED,
    draft_revision_id TEXT REFERENCES investment_principle_revisions(id) DEFERRABLE INITIALLY DEFERRED,
    is_archived INTEGER NOT NULL DEFAULT 0 CHECK(is_archived IN (0,1)),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version>0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE investment_principle_revisions (
    id TEXT PRIMARY KEY NOT NULL,
    principle_id TEXT NOT NULL REFERENCES investment_principles(id),
    revision_number INTEGER NOT NULL CHECK(revision_number>0),
    status TEXT NOT NULL CHECK(status IN ('DRAFT','EFFECTIVE','DISCARDED')),
    name TEXT NOT NULL,
    content TEXT NOT NULL,
    applicable_conditions TEXT,
    exceptions TEXT,
    revision_reason TEXT,
    source_review_id TEXT REFERENCES investment_reviews(id),
    source_reference TEXT,
    source_review_snapshot_json TEXT CHECK(source_review_snapshot_json IS NULL OR (json_valid(source_review_snapshot_json) AND json_extract(source_review_snapshot_json,'$.schemaVersion') IS 1)),
    scenario_tags_json TEXT NOT NULL CHECK(json_valid(scenario_tags_json) AND json_type(scenario_tags_json)='array'),
    body_json TEXT NOT NULL CHECK(json_valid(body_json) AND json_extract(body_json,'$.schemaVersion') IS 1),
    content_hash TEXT,
    source_context_hash TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_channel TEXT NOT NULL CHECK(created_channel IN ('gui','cli','mcp')),
    created_actor_kind TEXT NOT NULL CHECK(created_actor_kind IN ('GUI_SESSION','PROGRAM')),
    created_actor_id TEXT NOT NULL,
    created_credential_id TEXT,
    updated_channel TEXT NOT NULL CHECK(updated_channel IN ('gui','cli','mcp')),
    updated_actor_kind TEXT NOT NULL CHECK(updated_actor_kind IN ('GUI_SESSION','PROGRAM')),
    updated_actor_id TEXT NOT NULL,
    updated_credential_id TEXT,
    content_origin TEXT NOT NULL CHECK(content_origin IN ('USER_PROVIDED','ASSISTED_DRAFT','UNSPECIFIED')),
    published_at TEXT,
    published_channel TEXT CHECK(published_channel IN ('gui','cli','mcp')),
    published_actor_kind TEXT CHECK(published_actor_kind IN ('GUI_SESSION','PROGRAM')),
    published_actor_id TEXT,
    published_credential_id TEXT,
    published_request_id TEXT REFERENCES business_requests(id) DEFERRABLE INITIALLY DEFERRED,
    UNIQUE(principle_id,revision_number),
    CHECK(status<>'EFFECTIVE' OR (content_hash IS NOT NULL AND source_context_hash IS NOT NULL AND published_at IS NOT NULL
      AND published_channel IS NOT NULL AND published_actor_kind IS NOT NULL AND published_actor_id IS NOT NULL AND published_request_id IS NOT NULL
      AND (source_review_id IS NULL OR source_review_snapshot_json IS NOT NULL)))
);
CREATE UNIQUE INDEX investment_principle_one_draft ON investment_principle_revisions(principle_id) WHERE status='DRAFT';

CREATE TABLE investment_decision_principles (
    id TEXT PRIMARY KEY NOT NULL,
    decision_revision_id TEXT NOT NULL REFERENCES investment_decision_revisions(id),
    principle_revision_id TEXT NOT NULL REFERENCES investment_principle_revisions(id),
    UNIQUE(decision_revision_id,principle_revision_id)
);
CREATE INDEX investment_decision_principle_cases ON investment_decision_principles(principle_revision_id);

CREATE TABLE investment_review_principles (
    id TEXT PRIMARY KEY NOT NULL,
    review_id TEXT NOT NULL REFERENCES investment_reviews(id),
    position INTEGER NOT NULL CHECK(position>=0),
    principle_revision_id TEXT REFERENCES investment_principle_revisions(id),
    disposition TEXT NOT NULL CHECK(disposition IN ('KEEP','REVISE','RETIRE','NEW','UNDECIDED')),
    new_principle_revision_id TEXT REFERENCES investment_principle_revisions(id),
    reason TEXT,
    UNIQUE(review_id,position),
    CHECK(principle_revision_id IS NOT NULL OR disposition IN ('NEW','UNDECIDED'))
);
CREATE UNIQUE INDEX investment_review_evaluates_principle_once ON investment_review_principles(review_id,principle_revision_id) WHERE principle_revision_id IS NOT NULL;
CREATE INDEX investment_review_principle_cases ON investment_review_principles(principle_revision_id);

CREATE TRIGGER investment_review_root_guard BEFORE INSERT ON investment_reviews
WHEN (NEW.root_id=NEW.id AND (NEW.revision_number<>1 OR NEW.corrects_review_id IS NOT NULL))
 OR (NEW.root_id<>NEW.id AND NOT EXISTS(SELECT 1 FROM investment_reviews root WHERE root.id=NEW.root_id AND root.root_id=root.id
       AND root.decision_revision_id=NEW.decision_revision_id AND root.observation_cutoff=NEW.observation_cutoff AND root.observation_timezone=NEW.observation_timezone))
 OR (NEW.corrects_review_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM investment_reviews parent WHERE parent.id=NEW.corrects_review_id
       AND parent.status='SUBMITTED' AND parent.root_id=NEW.root_id AND parent.decision_revision_id=NEW.decision_revision_id
       AND parent.observation_cutoff=NEW.observation_cutoff AND parent.observation_timezone=NEW.observation_timezone AND NOT EXISTS(SELECT 1 FROM investment_reviews child WHERE child.corrects_review_id=parent.id AND child.status='SUBMITTED')))
 OR (NEW.corrects_review_id IS NULL AND EXISTS(SELECT 1 FROM investment_reviews old WHERE old.root_id=NEW.root_id AND old.status='SUBMITTED'))
BEGIN SELECT RAISE(ABORT,'review revisions must follow one root, cutoff and submitted chain'); END;
CREATE TRIGGER investment_review_confirmed_decision BEFORE INSERT ON investment_reviews
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=NEW.decision_revision_id AND status='CONFIRMED')
BEGIN SELECT RAISE(ABORT,'review requires a confirmed decision revision'); END;
CREATE TRIGGER investment_review_identity BEFORE UPDATE ON investment_reviews
WHEN NEW.id IS NOT OLD.id OR NEW.root_id IS NOT OLD.root_id OR NEW.decision_revision_id IS NOT OLD.decision_revision_id
 OR NEW.revision_number IS NOT OLD.revision_number OR NEW.corrects_review_id IS NOT OLD.corrects_review_id OR NEW.observation_cutoff IS NOT OLD.observation_cutoff OR NEW.observation_timezone IS NOT OLD.observation_timezone
 OR NEW.created_at IS NOT OLD.created_at OR NEW.created_channel IS NOT OLD.created_channel OR NEW.created_actor_kind IS NOT OLD.created_actor_kind
 OR NEW.created_actor_id IS NOT OLD.created_actor_id OR NEW.created_credential_id IS NOT OLD.created_credential_id
BEGIN SELECT RAISE(ABORT,'review identity and observation cutoff are fixed'); END;
CREATE TRIGGER investment_review_frozen BEFORE UPDATE ON investment_reviews WHEN OLD.status<>'DRAFT'
BEGIN SELECT RAISE(ABORT,'submitted or discarded review is immutable'); END;
CREATE TRIGGER investment_review_no_delete BEFORE DELETE ON investment_reviews
BEGIN SELECT RAISE(ABORT,'review history is retained'); END;
CREATE TRIGGER investment_principle_no_delete BEFORE DELETE ON investment_principles
BEGIN SELECT RAISE(ABORT,'principle history is retained'); END;
CREATE TRIGGER investment_principle_pointer_guard BEFORE UPDATE ON investment_principles
WHEN (NEW.current_revision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM investment_principle_revisions r WHERE r.id=NEW.current_revision_id AND r.principle_id=NEW.id AND r.status='EFFECTIVE'))
 OR (NEW.draft_revision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM investment_principle_revisions r WHERE r.id=NEW.draft_revision_id AND r.principle_id=NEW.id AND r.status='DRAFT'))
BEGIN SELECT RAISE(ABORT,'principle pointers must reference their matching versions'); END;
CREATE TRIGGER investment_principle_insert_pointer_guard BEFORE INSERT ON investment_principles
WHEN NEW.current_revision_id IS NOT NULL OR NEW.draft_revision_id IS NOT NULL
BEGIN SELECT RAISE(ABORT,'create principle root before its version pointers'); END;
CREATE TRIGGER investment_principle_revision_no_delete BEFORE DELETE ON investment_principle_revisions
BEGIN SELECT RAISE(ABORT,'principle version history is retained'); END;
CREATE TRIGGER investment_principle_revision_frozen BEFORE UPDATE ON investment_principle_revisions
WHEN OLD.status<>'DRAFT' OR NEW.id IS NOT OLD.id OR NEW.principle_id IS NOT OLD.principle_id OR NEW.revision_number IS NOT OLD.revision_number OR NEW.name IS NOT OLD.name OR NEW.content IS NOT OLD.content OR NEW.applicable_conditions IS NOT OLD.applicable_conditions OR NEW.exceptions IS NOT OLD.exceptions OR NEW.revision_reason IS NOT OLD.revision_reason OR NEW.source_review_id IS NOT OLD.source_review_id OR NEW.source_reference IS NOT OLD.source_reference OR NEW.scenario_tags_json IS NOT OLD.scenario_tags_json OR NEW.body_json IS NOT OLD.body_json OR NEW.content_origin IS NOT OLD.content_origin OR NEW.created_at IS NOT OLD.created_at OR NEW.created_channel IS NOT OLD.created_channel OR NEW.created_actor_kind IS NOT OLD.created_actor_kind OR NEW.created_actor_id IS NOT OLD.created_actor_id OR NEW.created_credential_id IS NOT OLD.created_credential_id
BEGIN SELECT RAISE(ABORT,'principle contents and version identity are append-only'); END;
CREATE TRIGGER investment_review_transition_body BEFORE UPDATE ON investment_reviews
WHEN NEW.status<>'DRAFT' AND (NEW.judgment IS NOT OLD.judgment OR NEW.execution_assessment IS NOT OLD.execution_assessment OR NEW.deviation_reason IS NOT OLD.deviation_reason OR NEW.outcome_evidence_json IS NOT OLD.outcome_evidence_json OR NEW.other_explanations IS NOT OLD.other_explanations OR NEW.open_questions IS NOT OLD.open_questions OR NEW.scenario_tags_json IS NOT OLD.scenario_tags_json OR NEW.body_json IS NOT OLD.body_json OR NEW.content_origin IS NOT OLD.content_origin)
BEGIN SELECT RAISE(ABORT,'review submission cannot replace contents'); END;
CREATE TRIGGER investment_reviews_tags_insert BEFORE INSERT ON investment_reviews
WHEN EXISTS(SELECT 1 FROM json_each(NEW.scenario_tags_json) WHERE type<>'text' OR length(trim(value))=0 OR value<>trim(value))
 OR (SELECT count(*) FROM json_each(NEW.scenario_tags_json))<>(SELECT count(DISTINCT value) FROM json_each(NEW.scenario_tags_json))
BEGIN SELECT RAISE(ABORT,'scenario tags must be unique nonempty strings'); END;
CREATE TRIGGER investment_reviews_tags_update BEFORE UPDATE ON investment_reviews
WHEN EXISTS(SELECT 1 FROM json_each(NEW.scenario_tags_json) WHERE type<>'text' OR length(trim(value))=0 OR value<>trim(value))
 OR (SELECT count(*) FROM json_each(NEW.scenario_tags_json))<>(SELECT count(DISTINCT value) FROM json_each(NEW.scenario_tags_json))
BEGIN SELECT RAISE(ABORT,'scenario tags must be unique nonempty strings'); END;
CREATE TRIGGER investment_principle_revisions_tags_insert BEFORE INSERT ON investment_principle_revisions
WHEN EXISTS(SELECT 1 FROM json_each(NEW.scenario_tags_json) WHERE type<>'text' OR length(trim(value))=0 OR value<>trim(value))
 OR (SELECT count(*) FROM json_each(NEW.scenario_tags_json))<>(SELECT count(DISTINCT value) FROM json_each(NEW.scenario_tags_json))
BEGIN SELECT RAISE(ABORT,'scenario tags must be unique nonempty strings'); END;
CREATE TRIGGER investment_principle_revisions_tags_update BEFORE UPDATE ON investment_principle_revisions
WHEN EXISTS(SELECT 1 FROM json_each(NEW.scenario_tags_json) WHERE type<>'text' OR length(trim(value))=0 OR value<>trim(value))
 OR (SELECT count(*) FROM json_each(NEW.scenario_tags_json))<>(SELECT count(DISTINCT value) FROM json_each(NEW.scenario_tags_json))
BEGIN SELECT RAISE(ABORT,'scenario tags must be unique nonempty strings'); END;
CREATE TRIGGER investment_decision_principles_draft_insert BEFORE INSERT ON investment_decision_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=NEW.decision_revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed case principle links are immutable'); END;
CREATE TRIGGER investment_decision_principles_draft_update BEFORE UPDATE ON investment_decision_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.decision_revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed case principle links are immutable'); END;
CREATE TRIGGER investment_decision_principles_draft_delete BEFORE DELETE ON investment_decision_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.decision_revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed case principle links are immutable'); END;
CREATE TRIGGER investment_decision_principles_identity BEFORE UPDATE ON investment_decision_principles
WHEN NEW.decision_revision_id IS NOT OLD.decision_revision_id OR NEW.id IS NOT OLD.id
BEGIN SELECT RAISE(ABORT,'case principle links cannot change identity'); END;
CREATE TRIGGER investment_review_principles_draft_insert BEFORE INSERT ON investment_review_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_reviews WHERE id=NEW.review_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed case principle links are immutable'); END;
CREATE TRIGGER investment_review_principles_draft_update BEFORE UPDATE ON investment_review_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_reviews WHERE id=OLD.review_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed case principle links are immutable'); END;
CREATE TRIGGER investment_review_principles_draft_delete BEFORE DELETE ON investment_review_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_reviews WHERE id=OLD.review_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed case principle links are immutable'); END;
CREATE TRIGGER investment_review_principles_identity BEFORE UPDATE ON investment_review_principles
WHEN NEW.review_id IS NOT OLD.review_id OR NEW.id IS NOT OLD.id
BEGIN SELECT RAISE(ABORT,'case principle links cannot change identity'); END;
CREATE TRIGGER investment_decision_effective_principle BEFORE INSERT ON investment_decision_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_principle_revisions WHERE id=NEW.principle_revision_id AND status='EFFECTIVE')
BEGIN SELECT RAISE(ABORT,'decision must reference a published principle version'); END;
CREATE TRIGGER investment_decision_effective_principle_update BEFORE UPDATE ON investment_decision_principles
WHEN NOT EXISTS(SELECT 1 FROM investment_principle_revisions WHERE id=NEW.principle_revision_id AND status='EFFECTIVE')
BEGIN SELECT RAISE(ABORT,'decision must reference a published principle version'); END;
