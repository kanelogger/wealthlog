DROP INDEX IF EXISTS idx_business_audit_request;
DROP INDEX IF EXISTS idx_business_audit_created_at;
DROP INDEX IF EXISTS idx_business_audit_entity;
DROP TABLE IF EXISTS business_audit;
DROP INDEX IF EXISTS idx_business_requests_state;
DROP TABLE IF EXISTS business_requests;
DROP INDEX IF EXISTS idx_pending_activities_created_at;
DROP INDEX IF EXISTS idx_pending_activities_status;
DROP TABLE IF EXISTS pending_activities;
DROP TRIGGER IF EXISTS activities_version_bump;
-- SQLite cannot drop a column on every supported version; the migration is
-- forward-only for `activities.version` (safe to leave in place on rollback).
