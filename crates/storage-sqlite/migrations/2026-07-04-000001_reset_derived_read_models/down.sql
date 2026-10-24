-- Schema-only reversal. The derived read models dropped by the up migration
-- (lots, lot_disposals, daily_account_valuation rows, and CALCULATED
-- holdings_snapshots) are NOT restored here: they are rebuilt from the source
-- activities/imports by the portfolio calculation path, not by this migration.
-- Only the additive lot account-FX columns are reversed.
ALTER TABLE lots DROP COLUMN account_currency;
ALTER TABLE lots DROP COLUMN fx_rate_to_account;
