-- Reclaim disk space: drop a redundant index, then rewrite the file.
--
-- Both halves have to live in the same migration: DROP INDEX only moves its
-- pages to the freelist, so the VACUUM below is what actually returns them to
-- the filesystem. Splitting them would leave the freed pages stranded until
-- some future VACUUM.
--
-- 1. `idx_quotes_asset_day (asset_id, day)` is a strict left prefix of
--    `uq_quotes_asset_day_source (asset_id, day, source)`, so every lookup it
--    serves can be answered by the unique index instead. On a real instance it
--    costs ~8 MiB across 148k quotes and a fourth index update on every quote
--    write, for no plan the unique index cannot already produce (verified with
--    EXPLAIN QUERY PLAN over the date-range, latest-quote, and exact-key
--    shapes: all resolve to uq_quotes_asset_day_source unchanged).
--
--    `idx_quotes_asset_source_day (asset_id, source, day)` is deliberately
--    KEPT. It is not redundant: the latest-quote-per-asset batch query filters
--    `asset_id IN (...) AND source = ?`, and without it the planner degrades
--    from an (asset_id, source) seek to an asset_id-only seek that scans every
--    day for the asset.
DROP INDEX IF EXISTS idx_quotes_asset_day;

-- 2. Valuations are derived data. Rebuild them once so transfer economics use
--    the current asset contract multiplier instead of the multiplier copied
--    into an older holdings snapshot. Existing startup backfill detection
--    schedules the full recalculation when these rows are missing.
DELETE FROM daily_account_valuation;

-- 3. Reclaim the space freed above and by the derived-read-model reset
--    (2026-07-04-000001), which this migration sorts after.
--
-- Deleting the CALCULATED snapshots releases their pages to SQLite's freelist
-- but does not shrink the database file; on a large instance that is hundreds
-- of megabytes the file keeps. VACUUM rewrites the file so the space is
-- actually returned to the filesystem.
--
-- This lives in its own migration because VACUUM cannot run inside a
-- transaction (see metadata.toml), while 2026-07-04-000001 must stay
-- transactional: it contains ALTER TABLE ... ADD COLUMN statements that would
-- fail on a re-run if a crash left it recorded as unapplied.
--
-- Every mutation here is idempotent (guarded DROP INDEX, derived-data DELETE,
-- VACUUM), so this migration is safe to re-run after a crash — and safe to
-- renumber, which is why it carries a version above the last shipped migration
-- while the ADD COLUMN migrations that preceded it keep theirs.

-- `run_migrations` sets `synchronous = OFF` for the migration connection, which
-- is not safe to hold across a VACUUM: VACUUM rewrites the whole database file,
-- so an OS crash or power loss mid-rewrite can corrupt it rather than losing
-- just the in-flight migration. Restore full durability for the rewrite, then
-- hand back to the caller, which resets the connection pragmas (to
-- `synchronous = NORMAL`) once migrations finish.
PRAGMA synchronous = FULL;
VACUUM;
