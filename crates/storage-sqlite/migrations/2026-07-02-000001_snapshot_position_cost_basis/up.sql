-- Additive: precomputed per-position cost basis at acquisition-date FX.
--
-- Sibling scalars to the embedded position lots. Written alongside the legacy
-- positions JSON so valuation can read a scalar instead of walking lots. Both
-- nullable: NULL means "not precomputed" (older rows / positions without
-- materialized lots), and consumers fall back to walking lots. Stored at full
-- precision so the scalar stays byte-identical to the lot-walked result.
ALTER TABLE snapshot_positions ADD COLUMN cost_basis_base TEXT;
ALTER TABLE snapshot_positions ADD COLUMN cost_basis_account TEXT;
