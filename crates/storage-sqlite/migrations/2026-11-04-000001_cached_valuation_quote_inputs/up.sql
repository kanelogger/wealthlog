-- Legacy cached valuations lack original quote evidence and remain readable.
ALTER TABLE daily_account_valuation ADD COLUMN quote_inputs TEXT;
