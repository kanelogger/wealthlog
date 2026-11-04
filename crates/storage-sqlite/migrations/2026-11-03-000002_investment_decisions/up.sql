-- M2 depends only on the legacy ledger, M1 and this group; never on M3.
CREATE TABLE investment_decisions (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK(lifecycle IN ('DRAFT','CONFIRMED','ARCHIVED')),
    execution_state TEXT NOT NULL CHECK(execution_state IN ('NOT_APPLICABLE','OPEN','CLOSED')),
    close_reason TEXT,
    current_revision_id TEXT REFERENCES investment_decision_revisions(id) DEFERRABLE INITIALLY DEFERRED,
    draft_revision_id TEXT REFERENCES investment_decision_revisions(id) DEFERRABLE INITIALLY DEFERRED,
    next_review_at TEXT,
    version INTEGER NOT NULL DEFAULT 1 CHECK(version>0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(execution_state<>'CLOSED' OR (close_reason IS NOT NULL AND length(trim(close_reason))>0)),
    CHECK(lifecycle<>'CONFIRMED' OR current_revision_id IS NOT NULL),
    CHECK(lifecycle<>'DRAFT' OR current_revision_id IS NULL)
);

CREATE TABLE investment_decision_revisions (
    id TEXT PRIMARY KEY NOT NULL,
    decision_id TEXT NOT NULL REFERENCES investment_decisions(id),
    revision_number INTEGER NOT NULL CHECK(revision_number>0),
    status TEXT NOT NULL CHECK(status IN ('DRAFT','CONFIRMED','DISCARDED')),
    action TEXT CHECK(action IN ('BUY','SELL','HOLD','WAIT','RESEARCH','OTHER')),
    asset_id TEXT,
    planned_amount TEXT CHECK (planned_amount IS NULL OR (planned_amount <> '' AND planned_amount NOT GLOB '*[^0-9.]*'
      AND planned_amount GLOB '[0-9]*' AND planned_amount NOT LIKE '%.%.%'
      AND (planned_amount = '0' OR substr(planned_amount,1,1) <> '0' OR substr(planned_amount,2,1) = '.')
      AND (instr(planned_amount,'.') = 0 OR (substr(planned_amount,-1) <> '.' AND substr(planned_amount,-1) <> '0')))),
    planned_quantity TEXT CHECK (planned_quantity IS NULL OR (planned_quantity <> '' AND planned_quantity NOT GLOB '*[^0-9.]*'
      AND planned_quantity GLOB '[0-9]*' AND planned_quantity NOT LIKE '%.%.%'
      AND (planned_quantity = '0' OR substr(planned_quantity,1,1) <> '0' OR substr(planned_quantity,2,1) = '.')
      AND (instr(planned_quantity,'.') = 0 OR (substr(planned_quantity,-1) <> '.' AND substr(planned_quantity,-1) <> '0')))),
    currency TEXT CHECK(currency IS NULL OR (length(currency)=3 AND currency NOT GLOB '*[^A-Z]*')),
    thesis TEXT NOT NULL,
    accepted_disadvantages TEXT,
    information_value TEXT,
    waiting_cost TEXT,
    next_check_at TEXT,
    declared_at TEXT,
    funding_status TEXT NOT NULL CHECK(funding_status IN ('UNKNOWN','PLANNED')),
    scenario_tags_json TEXT NOT NULL CHECK(json_valid(scenario_tags_json) AND json_type(scenario_tags_json)='array'),
    body_json TEXT NOT NULL CHECK(json_valid(body_json) AND json_extract(body_json,'$.schemaVersion') IS 1),
    context_snapshot_json TEXT CHECK(context_snapshot_json IS NULL OR (json_valid(context_snapshot_json) AND json_extract(context_snapshot_json,'$.schemaVersion') IS 1)),
    content_hash TEXT,
    context_hash TEXT,
    confirmed_at TEXT,
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
    confirmed_channel TEXT CHECK(confirmed_channel IN ('gui','cli','mcp')),
    confirmed_actor_kind TEXT CHECK(confirmed_actor_kind IN ('GUI_SESSION','PROGRAM')),
    confirmed_actor_id TEXT,
    confirmed_credential_id TEXT,
    UNIQUE(decision_id,revision_number),
    CHECK(status<>'CONFIRMED' OR (confirmed_at IS NOT NULL AND content_hash IS NOT NULL AND context_hash IS NOT NULL
      AND context_snapshot_json IS NOT NULL AND confirmed_actor_id IS NOT NULL AND confirmed_channel IS NOT NULL AND confirmed_actor_kind IS NOT NULL))
);
CREATE UNIQUE INDEX investment_decision_one_draft ON investment_decision_revisions(decision_id) WHERE status='DRAFT';
CREATE INDEX investment_decision_check ON investment_decision_revisions(next_check_at) WHERE status='DRAFT';

CREATE TABLE investment_decision_options (
    id TEXT PRIMARY KEY NOT NULL,
    revision_id TEXT NOT NULL REFERENCES investment_decision_revisions(id),
    position INTEGER NOT NULL CHECK(position>=0),
    action TEXT NOT NULL CHECK(length(trim(action))>0),
    description TEXT NOT NULL,
    advantages TEXT,
    disadvantages TEXT,
    not_selected_reason TEXT,
    UNIQUE(revision_id,position)
);

CREATE TABLE investment_decision_evidence (
    id TEXT PRIMARY KEY NOT NULL,
    revision_id TEXT NOT NULL REFERENCES investment_decision_revisions(id),
    position INTEGER NOT NULL CHECK(position>=0),
    information_kind TEXT NOT NULL CHECK(information_kind IN ('FACT','HYPOTHESIS','UNKNOWN')),
    content TEXT NOT NULL,
    source TEXT,
    probability_lower_bps INTEGER,
    probability_upper_bps INTEGER,
    probability_basis TEXT,
    purpose_id TEXT REFERENCES fund_purposes(id),
    assumed_loss_amount TEXT CHECK (assumed_loss_amount IS NULL OR (assumed_loss_amount <> '' AND assumed_loss_amount NOT GLOB '*[^0-9.]*'
      AND assumed_loss_amount GLOB '[0-9]*' AND assumed_loss_amount NOT LIKE '%.%.%'
      AND (assumed_loss_amount = '0' OR substr(assumed_loss_amount,1,1) <> '0' OR substr(assumed_loss_amount,2,1) = '.')
      AND (instr(assumed_loss_amount,'.') = 0 OR (substr(assumed_loss_amount,-1) <> '.' AND substr(assumed_loss_amount,-1) <> '0')))),
    loss_currency TEXT,
    loss_basis TEXT,
    UNIQUE(revision_id,position),
    CHECK((probability_lower_bps IS NULL AND probability_upper_bps IS NULL AND probability_basis IS NULL)
       OR (probability_lower_bps IS NOT NULL AND probability_upper_bps IS NOT NULL
         AND probability_lower_bps BETWEEN 0 AND 10000 AND probability_upper_bps BETWEEN probability_lower_bps AND 10000
         AND probability_basis IS NOT NULL AND length(trim(probability_basis))>0)),
    CHECK((purpose_id IS NULL AND assumed_loss_amount IS NULL AND loss_currency IS NULL AND loss_basis IS NULL)
       OR (information_kind='HYPOTHESIS' AND purpose_id IS NOT NULL AND assumed_loss_amount IS NOT NULL
         AND loss_currency IS NOT NULL AND length(loss_currency)=3 AND loss_currency NOT GLOB '*[^A-Z]*'
         AND loss_basis IS NOT NULL AND length(trim(loss_basis))>0))
);
CREATE UNIQUE INDEX investment_decision_one_loss_scenario ON investment_decision_evidence(revision_id,purpose_id) WHERE purpose_id IS NOT NULL;

CREATE TABLE investment_decision_funding (
    id TEXT PRIMARY KEY NOT NULL,
    revision_id TEXT NOT NULL REFERENCES investment_decision_revisions(id),
    position INTEGER NOT NULL CHECK(position>=0),
    purpose_id TEXT NOT NULL REFERENCES fund_purposes(id),
    resource_kind TEXT NOT NULL CHECK(resource_kind IN ('CASH','SECURITY')),
    account_id TEXT NOT NULL CHECK(length(trim(account_id))>0),
    currency TEXT,
    asset_id TEXT,
    planned_amount TEXT CHECK (planned_amount IS NULL OR (planned_amount <> '' AND planned_amount NOT GLOB '*[^0-9.]*'
      AND planned_amount GLOB '[0-9]*' AND planned_amount NOT LIKE '%.%.%'
      AND (planned_amount = '0' OR substr(planned_amount,1,1) <> '0' OR substr(planned_amount,2,1) = '.')
      AND (instr(planned_amount,'.') = 0 OR (substr(planned_amount,-1) <> '.' AND substr(planned_amount,-1) <> '0')))),
    planned_quantity TEXT CHECK (planned_quantity IS NULL OR (planned_quantity <> '' AND planned_quantity NOT GLOB '*[^0-9.]*'
      AND planned_quantity GLOB '[0-9]*' AND planned_quantity NOT LIKE '%.%.%'
      AND (planned_quantity = '0' OR substr(planned_quantity,1,1) <> '0' OR substr(planned_quantity,2,1) = '.')
      AND (instr(planned_quantity,'.') = 0 OR (substr(planned_quantity,-1) <> '.' AND substr(planned_quantity,-1) <> '0')))),
    UNIQUE(revision_id,position),
    CHECK((resource_kind='CASH' AND currency IS NOT NULL AND length(currency)=3 AND currency NOT GLOB '*[^A-Z]*'
       AND asset_id IS NULL AND planned_amount IS NOT NULL AND planned_quantity IS NULL)
      OR (resource_kind='SECURITY' AND currency IS NULL AND asset_id IS NOT NULL AND length(trim(asset_id))>0
       AND planned_amount IS NULL AND planned_quantity IS NOT NULL))
);

CREATE TABLE investment_execution_links (
    id TEXT PRIMARY KEY NOT NULL,
    execution_id TEXT NOT NULL REFERENCES investment_executions(id),
    decision_revision_id TEXT NOT NULL REFERENCES investment_decision_revisions(id),
    linked_at TEXT NOT NULL,
    unlinked_at TEXT,
    unlink_reason TEXT,
    source_version TEXT NOT NULL,
    temporal_classification TEXT NOT NULL CHECK(temporal_classification IN ('BEFORE_CONFIRMATION','AFTER_CONFIRMATION','UNDETERMINED')),
    deviation_reason TEXT,
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json) AND json_extract(snapshot_json,'$.schemaVersion') IS 1),
    created_channel TEXT NOT NULL CHECK(created_channel IN ('gui','cli','mcp')),
    created_actor_kind TEXT NOT NULL CHECK(created_actor_kind IN ('GUI_SESSION','PROGRAM')),
    created_actor_id TEXT NOT NULL,
    created_credential_id TEXT,
    unlinked_channel TEXT CHECK(unlinked_channel IN ('gui','cli','mcp')),
    unlinked_actor_kind TEXT CHECK(unlinked_actor_kind IN ('GUI_SESSION','PROGRAM')),
    unlinked_actor_id TEXT,
    unlinked_credential_id TEXT,
    CHECK((unlinked_at IS NULL AND unlink_reason IS NULL) OR (unlinked_at IS NOT NULL AND unlink_reason IS NOT NULL AND length(trim(unlink_reason))>0))
);
CREATE UNIQUE INDEX investment_execution_one_link ON investment_execution_links(execution_id) WHERE unlinked_at IS NULL;
CREATE INDEX investment_links_revision ON investment_execution_links(decision_revision_id);

CREATE TRIGGER investment_decision_pointer_guard BEFORE UPDATE ON investment_decisions
WHEN (NEW.current_revision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM investment_decision_revisions r WHERE r.id=NEW.current_revision_id AND r.decision_id=NEW.id AND r.status='CONFIRMED'))
 OR (NEW.draft_revision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM investment_decision_revisions r WHERE r.id=NEW.draft_revision_id AND r.decision_id=NEW.id AND r.status='DRAFT'))
BEGIN SELECT RAISE(ABORT,'decision pointer must reference its matching revision'); END;
CREATE TRIGGER investment_decision_insert_pointer_guard BEFORE INSERT ON investment_decisions
WHEN (NEW.current_revision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM investment_decision_revisions r WHERE r.id=NEW.current_revision_id AND r.decision_id=NEW.id AND r.status='CONFIRMED'))
 OR (NEW.draft_revision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM investment_decision_revisions r WHERE r.id=NEW.draft_revision_id AND r.decision_id=NEW.id AND r.status='DRAFT'))
BEGIN SELECT RAISE(ABORT,'decision pointer must reference its matching revision'); END;
CREATE TRIGGER investment_decision_no_delete BEFORE DELETE ON investment_decisions
BEGIN SELECT RAISE(ABORT,'investment decision history is retained'); END;
CREATE TRIGGER investment_revision_frozen BEFORE UPDATE ON investment_decision_revisions WHEN OLD.status<>'DRAFT'
BEGIN SELECT RAISE(ABORT,'confirmed or discarded decision is immutable'); END;
CREATE TRIGGER investment_revision_no_delete BEFORE DELETE ON investment_decision_revisions
BEGIN SELECT RAISE(ABORT,'investment decision history is retained'); END;
CREATE TRIGGER investment_link_confirmed BEFORE INSERT ON investment_execution_links
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=NEW.decision_revision_id AND status='CONFIRMED')
BEGIN SELECT RAISE(ABORT,'execution requires a confirmed decision revision'); END;
CREATE TRIGGER investment_link_no_delete BEFORE DELETE ON investment_execution_links
BEGIN SELECT RAISE(ABORT,'investment execution evidence is retained'); END;
CREATE TRIGGER investment_link_unlink_only BEFORE UPDATE ON investment_execution_links
WHEN OLD.unlinked_at IS NOT NULL OR NEW.unlinked_at IS NULL OR NEW.id IS NOT OLD.id OR NEW.execution_id IS NOT OLD.execution_id
 OR NEW.decision_revision_id IS NOT OLD.decision_revision_id OR NEW.linked_at IS NOT OLD.linked_at
 OR NEW.source_version IS NOT OLD.source_version OR NEW.temporal_classification IS NOT OLD.temporal_classification
 OR NEW.deviation_reason IS NOT OLD.deviation_reason OR NEW.snapshot_json IS NOT OLD.snapshot_json
 OR NEW.created_channel IS NOT OLD.created_channel OR NEW.created_actor_kind IS NOT OLD.created_actor_kind
 OR NEW.created_actor_id IS NOT OLD.created_actor_id OR NEW.created_credential_id IS NOT OLD.created_credential_id
BEGIN SELECT RAISE(ABORT,'execution evidence permits one explicit unlink only'); END;

CREATE TRIGGER investment_decision_options_draft_insert BEFORE INSERT ON investment_decision_options
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=NEW.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;

CREATE TRIGGER investment_decision_options_draft_update BEFORE UPDATE ON investment_decision_options
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;

CREATE TRIGGER investment_decision_options_draft_delete BEFORE DELETE ON investment_decision_options
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;
CREATE TRIGGER investment_decision_options_same_revision BEFORE UPDATE ON investment_decision_options
WHEN NEW.revision_id IS NOT OLD.revision_id
BEGIN SELECT RAISE(ABORT,'decision child cannot move to another revision'); END;

CREATE TRIGGER investment_decision_evidence_draft_insert BEFORE INSERT ON investment_decision_evidence
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=NEW.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;

CREATE TRIGGER investment_decision_evidence_draft_update BEFORE UPDATE ON investment_decision_evidence
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;

CREATE TRIGGER investment_decision_evidence_draft_delete BEFORE DELETE ON investment_decision_evidence
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;
CREATE TRIGGER investment_decision_evidence_same_revision BEFORE UPDATE ON investment_decision_evidence
WHEN NEW.revision_id IS NOT OLD.revision_id
BEGIN SELECT RAISE(ABORT,'decision child cannot move to another revision'); END;

CREATE TRIGGER investment_decision_funding_draft_insert BEFORE INSERT ON investment_decision_funding
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=NEW.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;

CREATE TRIGGER investment_decision_funding_draft_update BEFORE UPDATE ON investment_decision_funding
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;

CREATE TRIGGER investment_decision_funding_draft_delete BEFORE DELETE ON investment_decision_funding
WHEN NOT EXISTS(SELECT 1 FROM investment_decision_revisions WHERE id=OLD.revision_id AND status='DRAFT')
BEGIN SELECT RAISE(ABORT,'confirmed decision children are immutable'); END;
CREATE TRIGGER investment_decision_funding_same_revision BEFORE UPDATE ON investment_decision_funding
WHEN NEW.revision_id IS NOT OLD.revision_id
BEGIN SELECT RAISE(ABORT,'decision child cannot move to another revision'); END;

CREATE TRIGGER investment_decision_tags_insert BEFORE INSERT ON investment_decision_revisions
WHEN EXISTS(SELECT 1 FROM json_each(NEW.scenario_tags_json) WHERE type<>'text' OR length(trim(value))=0 OR value<>trim(value))
 OR (SELECT count(*) FROM json_each(NEW.scenario_tags_json))<>(SELECT count(DISTINCT value) FROM json_each(NEW.scenario_tags_json))
BEGIN SELECT RAISE(ABORT,'scenario tags must be unique nonempty strings'); END;

CREATE TRIGGER investment_decision_tags_update BEFORE UPDATE ON investment_decision_revisions
WHEN EXISTS(SELECT 1 FROM json_each(NEW.scenario_tags_json) WHERE type<>'text' OR length(trim(value))=0 OR value<>trim(value))
 OR (SELECT count(*) FROM json_each(NEW.scenario_tags_json))<>(SELECT count(DISTINCT value) FROM json_each(NEW.scenario_tags_json))
BEGIN SELECT RAISE(ABORT,'scenario tags must be unique nonempty strings'); END;

CREATE TRIGGER investment_revision_identity BEFORE UPDATE ON investment_decision_revisions
WHEN NEW.id IS NOT OLD.id OR NEW.decision_id IS NOT OLD.decision_id OR NEW.revision_number IS NOT OLD.revision_number OR NEW.created_at IS NOT OLD.created_at OR NEW.created_channel IS NOT OLD.created_channel OR NEW.created_actor_kind IS NOT OLD.created_actor_kind OR NEW.created_actor_id IS NOT OLD.created_actor_id OR NEW.created_credential_id IS NOT OLD.created_credential_id
BEGIN SELECT RAISE(ABORT,'decision identity and creation provenance are immutable'); END;

CREATE TRIGGER investment_revision_transition_body BEFORE UPDATE ON investment_decision_revisions
WHEN NEW.status<>'DRAFT' AND (NEW.action IS NOT OLD.action OR NEW.asset_id IS NOT OLD.asset_id OR NEW.planned_amount IS NOT OLD.planned_amount OR NEW.planned_quantity IS NOT OLD.planned_quantity OR NEW.currency IS NOT OLD.currency OR NEW.thesis IS NOT OLD.thesis OR NEW.accepted_disadvantages IS NOT OLD.accepted_disadvantages OR NEW.information_value IS NOT OLD.information_value OR NEW.waiting_cost IS NOT OLD.waiting_cost OR NEW.next_check_at IS NOT OLD.next_check_at OR NEW.declared_at IS NOT OLD.declared_at OR NEW.funding_status IS NOT OLD.funding_status OR NEW.scenario_tags_json IS NOT OLD.scenario_tags_json OR NEW.body_json IS NOT OLD.body_json OR NEW.content_origin IS NOT OLD.content_origin)
BEGIN SELECT RAISE(ABORT,'terminal decision transition cannot replace contents'); END;
