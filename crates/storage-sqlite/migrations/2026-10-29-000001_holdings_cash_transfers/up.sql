-- Revalue existing manual-account history under the live cash-transfer rule.
-- Source activities and observed snapshots stay intact. Transfer closure in
-- the coordinator also refreshes counterparties for correct scope netting.
INSERT INTO projection_state (scope, dirty_from, version)
SELECT id, '0001-01-01', 1
FROM accounts
WHERE tracking_mode = 'HOLDINGS'
ON CONFLICT (scope) DO UPDATE SET
    dirty_from = '0001-01-01',
    version = projection_state.version + 1;
