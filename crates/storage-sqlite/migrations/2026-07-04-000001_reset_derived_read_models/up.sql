-- Reset the derived read models so they rebuild from source data.
--
-- Lots, lot disposals, daily account valuations, and CALCULATED holdings
-- snapshots are all generated from the source activity/import data. This
-- migration replaces the previous in-app "strip embedded lots" startup
-- migration with a plain drop-and-rebuild: clear the generated read models and
-- let the existing portfolio calculation path rebuild them from activities with
-- one consistent version. Source snapshots (MANUAL_ENTRY, CSV_IMPORT,
-- BROKER_IMPORTED, SYNTHETIC) are NOT touched.

-- 1. Persist account FX on the lot book.
--    Additive, nullable columns so a persisted open lot carries the account-FX
--    it was acquired at (rate + account currency). This makes the current lot
--    book complete on its own: an append-only rebuild reconstructs the same
--    account-currency cost basis as a full rebuild for multi-currency accounts,
--    without falling back to market FX. NULL means same-currency, or a lot
--    written before this column existed.
ALTER TABLE lots ADD COLUMN fx_rate_to_account TEXT;
ALTER TABLE lots ADD COLUMN account_currency TEXT;

-- 2. Preserve HOLDINGS-mode source snapshots.
--    Older HOLDINGS snapshots can carry source='CALCULATED' because the source
--    column defaulted there, but HOLDINGS accounts are not replayed from
--    activities, so those rows are source data. Normalize them to MANUAL_ENTRY
--    before the calculated rows are dropped so they survive the reset.
UPDATE holdings_snapshots
SET source = 'MANUAL_ENTRY'
WHERE source = 'CALCULATED'
  AND account_id IN (
      SELECT id
      FROM accounts
      WHERE tracking_mode = 'HOLDINGS'
  );

-- 3. Drop the rebuildable derived read models. The portfolio calculation path
--    regenerates calculated snapshots, valuations, lots, and disposals from the
--    source activities on the next recalculation.
DELETE FROM holdings_snapshots WHERE source = 'CALCULATED';
DELETE FROM daily_account_valuation;
DELETE FROM lot_disposals;
DELETE FROM lots;

-- 4. Drop the relational position rows orphaned by step 3. Migrations run with
--    `PRAGMA foreign_keys = OFF`, so the ON DELETE CASCADE from
--    holdings_snapshots does not fire here. Keyframes are sparse, so a rebuilt
--    snapshot does not necessarily reuse the deleted row's snapshot_id and the
--    delete-then-insert on write would not reclaim these.
DELETE FROM snapshot_positions
WHERE snapshot_id NOT IN (SELECT id FROM holdings_snapshots);
