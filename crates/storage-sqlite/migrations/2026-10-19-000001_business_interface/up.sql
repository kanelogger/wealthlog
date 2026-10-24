-- Unified business interface, task two: activity versions, pending entries,
-- request records and the business audit trail.
--
-- No historical row is rewritten: activities gain version = 1 through the
-- column default, and every existing migration stays untouched.

-- ---------------------------------------------------------------------------
-- Activity version
-- ---------------------------------------------------------------------------
-- A monotonic per-activity counter used for optimistic concurrency on the new
-- business paths. It is deliberately OUTSIDE the shared write models: the
-- trigger below is the only writer, so old paths (grid save, import, broker
-- sync) never have to know about it.
ALTER TABLE activities ADD COLUMN version INTEGER NOT NULL DEFAULT 1;

-- Bump only when a business column changes, so audit/bookkeeping updates
-- (updated_at, is_user_modified, needs_review) do not fake a conflict.
CREATE TRIGGER activities_version_bump
AFTER UPDATE OF account_id, asset_id, activity_type, activity_type_override,
                subtype, status, activity_date, settlement_date, quantity,
                unit_price, amount, fee, tax, currency, fx_rate, notes,
                metadata, source_group_id
ON activities
FOR EACH ROW
WHEN NEW.version = OLD.version
BEGIN
    UPDATE activities SET version = OLD.version + 1 WHERE id = NEW.id;
END;

-- ---------------------------------------------------------------------------
-- Pending entries (待确认收支)
-- ---------------------------------------------------------------------------
-- Drafts that may be missing account, currency or date. Confirming one creates
-- a POSTED activity exactly once; the resulting activity id is recorded here.
CREATE TABLE pending_activities (
    id TEXT PRIMARY KEY NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'committed', 'rejected')),
    -- Nullable on purpose: an Agent may not know these yet.
    account_id TEXT REFERENCES accounts(id) ON DELETE SET NULL,
    activity_type TEXT NOT NULL,
    activity_date TEXT,
    currency TEXT,
    amount TEXT,
    fee TEXT,
    tax TEXT,
    notes TEXT,
    -- Asset hints are resolved when the entry is confirmed.
    asset_symbol TEXT,
    -- What created the entry: a PAT id (never a raw token) or 'manual'.
    created_by TEXT NOT NULL,
    -- Credential that confirms it, recorded separately from the creator.
    confirmed_by TEXT,
    -- Free-form extraction payload (message id, references, original text).
    source TEXT,
    version INTEGER NOT NULL DEFAULT 1,
    committed_activity_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_pending_activities_status ON pending_activities(status);
CREATE INDEX idx_pending_activities_created_at ON pending_activities(created_at);

-- ---------------------------------------------------------------------------
-- Request records (replay / idempotency)
-- ---------------------------------------------------------------------------
-- One row per (ledger, subject, operation, request key). The first submit
-- freezes the request snapshot; a retry with the same key and content returns
-- the stored result, while different content conflicts.
CREATE TABLE business_requests (
    id TEXT PRIMARY KEY NOT NULL,
    ledger_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    operation TEXT NOT NULL,
    request_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    request_snapshot TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('in_progress', 'completed', 'failed')),
    result_json TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (ledger_id, subject, operation, request_key)
);

CREATE INDEX idx_business_requests_state ON business_requests(state);

-- ---------------------------------------------------------------------------
-- Business audit (GUI, CLI and REST writes)
-- ---------------------------------------------------------------------------
-- The MCP audit log keeps protocol facts; this table records business
-- mutations from every entry point with a common shape.
CREATE TABLE business_audit (
    id TEXT PRIMARY KEY NOT NULL,
    ledger_id TEXT NOT NULL,
    -- 'gui' | 'cli' | 'rest' | 'mcp'
    channel TEXT NOT NULL,
    -- Token id / session id / 'anonymous' — never a raw credential.
    actor TEXT NOT NULL,
    -- Whether the actor identity was authenticated by middleware.
    authenticated INTEGER NOT NULL DEFAULT 0,
    operation TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT,
    request_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'error', 'denied')),
    detail_json TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_business_audit_entity ON business_audit(entity_type, entity_id);
CREATE INDEX idx_business_audit_created_at ON business_audit(created_at);
CREATE INDEX idx_business_audit_request ON business_audit(request_id);

-- Link MCP protocol rows to the business audit of the same call.
ALTER TABLE mcp_audit_log ADD COLUMN business_request_id TEXT;
