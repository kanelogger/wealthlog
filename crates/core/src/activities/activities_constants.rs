//! Activity types
//!
//! Each constant represents one of the supported activity categories.
//! The descriptions mirror `docs/activity-types.md` for quick reference.

/// Purchase of a security or other asset. Decreases cash and increases quantity.
pub const ACTIVITY_TYPE_BUY: &str = "BUY";

/// Disposal of a security or other asset. Increases cash and decreases quantity.
pub const ACTIVITY_TYPE_SELL: &str = "SELL";

/// Cash dividend paid into the account. Increases cash.
pub const ACTIVITY_TYPE_DIVIDEND: &str = "DIVIDEND";

/// Interest earned on cash or fixed-income positions. Increases cash.
pub const ACTIVITY_TYPE_INTEREST: &str = "INTEREST";

/// Incoming funds from outside Wealthfolio. Increases cash.
pub const ACTIVITY_TYPE_DEPOSIT: &str = "DEPOSIT";

/// Outgoing funds to an external account. Decreases cash.
pub const ACTIVITY_TYPE_WITHDRAWAL: &str = "WITHDRAWAL";

/// Move cash or assets into this account. Asset cost basis is preserved.
/// Default: internal move (no net contribution).
/// Can be external: when from untracked world (metadata.flow.is_external=true).
/// Increases cash or quantity.
pub const ACTIVITY_TYPE_TRANSFER_IN: &str = "TRANSFER_IN";

/// Move cash or assets out of this account. Asset cost basis is exported.
/// Default: internal move (no net contribution).
/// Can be external: when to untracked world (metadata.flow.is_external=true).
/// Decreases cash or quantity.
pub const ACTIVITY_TYPE_TRANSFER_OUT: &str = "TRANSFER_OUT";

/// Stand-alone brokerage or platform fee not tied to a trade. Decreases cash.
pub const ACTIVITY_TYPE_FEE: &str = "FEE";

/// Tax paid from the account (e.g., withholding or realised CGT). Decreases cash.
pub const ACTIVITY_TYPE_TAX: &str = "TAX";

/// Stock split or reverse split. Adjusts quantity and per-share cost without affecting total value.
pub const ACTIVITY_TYPE_SPLIT: &str = "SPLIT";

/// Cash-only credit: refunds, rebates, bonuses.
/// Default: does NOT affect net_contribution unless metadata.flow.is_external=true.
pub const ACTIVITY_TYPE_CREDIT: &str = "CREDIT";

/// Non-trade correction / transformation (usually no cash).
/// Examples: option expire worthless, RoC basis adjustment, merger/spinoff compiler input.
pub const ACTIVITY_TYPE_ADJUSTMENT: &str = "ADJUSTMENT";

/// Unknown or unmapped activity type. Requires user review.
pub const ACTIVITY_TYPE_UNKNOWN: &str = "UNKNOWN";

/// Trading activity types
pub const TRADING_ACTIVITY_TYPES: [&str; 3] =
    [ACTIVITY_TYPE_BUY, ACTIVITY_TYPE_SELL, ACTIVITY_TYPE_SPLIT];

/// Income activity types
pub const INCOME_ACTIVITY_TYPES: [&str; 2] = [ACTIVITY_TYPE_DIVIDEND, ACTIVITY_TYPE_INTEREST];

/// Activity types where `unit_price` represents the asset's per-unit market
/// price and can be used as a fallback quote. Income activities (DIVIDEND,
/// INTEREST) store payment amounts, not asset prices. Securities transfers
/// store book cost basis in `unit_price`; valuation derives transfer market
/// value from quotes instead.
pub const PRICE_BEARING_ACTIVITY_TYPES: [&str; 2] = [ACTIVITY_TYPE_BUY, ACTIVITY_TYPE_SELL];

/// Activity types that always require a symbol/asset.
/// Everything else: symbol is optional (cash-only or dual-use like TRANSFER_IN).
pub const SYMBOL_REQUIRED_TYPES: [&str; 5] = [
    ACTIVITY_TYPE_BUY,
    ACTIVITY_TYPE_SELL,
    ACTIVITY_TYPE_SPLIT,
    ACTIVITY_TYPE_DIVIDEND,
    ACTIVITY_TYPE_ADJUSTMENT,
];

/// Returns true when the activity type always requires a symbol/asset.
pub fn requires_symbol(activity_type: &str) -> bool {
    SYMBOL_REQUIRED_TYPES.contains(&activity_type)
}

/// Recognizes cash-placeholder symbols from broker exports (e.g. `$CASH-CAD`, `CASH:USD`).
pub fn is_cash_symbol(symbol: &str) -> bool {
    let s = symbol.trim();
    if s.is_empty() {
        return false;
    }
    // $CASH-XXX, $CASH_XXX, CASH-XXX, CASH_XXX, CASH:XXX (case-insensitive)
    let upper = s.to_uppercase();
    let stripped = upper.strip_prefix('$').unwrap_or(&upper);
    if let Some(rest) = stripped.strip_prefix("CASH") {
        if let Some(currency) = rest
            .strip_prefix('-')
            .or_else(|| rest.strip_prefix('_'))
            .or_else(|| rest.strip_prefix(':'))
        {
            return currency.len() == 3 && currency.chars().all(|c| c.is_ascii_alphabetic());
        }
    }
    false
}

/// Returns true for symbols that are clearly not real tickers (all dashes, `$`-prefixed junk, etc.).
pub fn is_garbage_symbol(symbol: &str) -> bool {
    let s = symbol.trim();
    if s.is_empty() {
        return false;
    }
    // All-dash: "----", "--"
    if s.chars().all(|c| c == '-') {
        return true;
    }
    // $-prefixed that isn't a recognized cash pattern: "$FOO", "$123"
    if s.starts_with('$') && !is_cash_symbol(s) {
        return true;
    }
    false
}

/// Activity types that are always pure cash — they never reference an asset.
pub const NEVER_ASSET_TYPES: [&str; 5] = [
    ACTIVITY_TYPE_DEPOSIT,
    ACTIVITY_TYPE_WITHDRAWAL,
    ACTIVITY_TYPE_FEE,
    ACTIVITY_TYPE_TAX,
    ACTIVITY_TYPE_CREDIT,
];

// ─────────────────────────────────────────────────────────────────────────────
// Activity Subtypes
// ─────────────────────────────────────────────────────────────────────────────
// Subtypes provide semantic variations of activity types without schema changes.
// The compiler expands these into canonical activity type postings.

/// DRIP (Dividend Reinvestment Plan): Dividend automatically reinvested in shares.
/// Expands to: DIVIDEND + BUY
pub const ACTIVITY_SUBTYPE_DRIP: &str = "DRIP";

/// Staking Reward: Crypto staking income received as tokens.
/// Expands to: INTEREST + BUY
pub const ACTIVITY_SUBTYPE_STAKING_REWARD: &str = "STAKING_REWARD";

/// Dividend in Kind: Dividend paid as additional units of the same asset.
/// Expands to: DIVIDEND + BUY
pub const ACTIVITY_SUBTYPE_DIVIDEND_IN_KIND: &str = "DIVIDEND_IN_KIND";

/// Bonus: External cash credit (new capital entering portfolio).
/// Affects net_contribution (like DEPOSIT) and is an external flow for TWR.
/// Examples: sign-up bonus, referral bonus, promotional credit.
pub const ACTIVITY_SUBTYPE_BONUS: &str = "BONUS";

/// Rebate: Trading rebate (negative fee, internal flow).
/// Does NOT affect net_contribution - represents reduced trading costs.
/// Examples: maker rebate, volume rebate.
pub const ACTIVITY_SUBTYPE_REBATE: &str = "REBATE";

/// Refund: Fee correction/reversal (internal flow).
/// Does NOT affect net_contribution - represents a fee that was reversed.
/// Examples: erroneous fee refund, service credit.
pub const ACTIVITY_SUBTYPE_REFUND: &str = "REFUND";

/// Reimbursement: External cash credit that offsets an expense category.
/// Used by spending/budgeting to reduce actuals without treating the money as income.
/// Examples: friend reimburses dinner, employer pays back a travel expense.
pub const ACTIVITY_SUBTYPE_REIMBURSEMENT: &str = "REIMBURSEMENT";

/// Option expiry: option contract expired worthless.
/// Removes lots via FIFO with no cash effect.
pub const ACTIVITY_SUBTYPE_OPTION_EXPIRY: &str = "OPTION_EXPIRY";

/// Return of capital: capital paid back, not income. It reduces the asset's
/// cost basis, and beyond it is a capital gain. On a DIVIDEND, the cash
/// distribution is capital; on an ADJUSTMENT (no cash), it reclassifies that
/// amount of dividends already recorded as capital (a T3's box 42).
pub const ACTIVITY_SUBTYPE_RETURN_OF_CAPITAL: &str = "RETURN_OF_CAPITAL";

/// Notional distribution: an ADJUSTMENT for a taxable distribution reinvested
/// without new units (also called phantom or non-cash). It is income and raises
/// the asset's cost basis by its amount, with no unit or cash effect.
pub const ACTIVITY_SUBTYPE_NOTIONAL_DISTRIBUTION: &str = "NOTIONAL_DISTRIBUTION";

/// Position-opening trade intent.
/// Examples: buy-to-open for long options, sell-to-open for short options.
pub const ACTIVITY_SUBTYPE_POSITION_OPEN: &str = "POSITION_OPEN";

/// Position-closing trade intent.
/// Examples: sell-to-close for long options, buy-to-close for short options.
pub const ACTIVITY_SUBTYPE_POSITION_CLOSE: &str = "POSITION_CLOSE";

/// A TRANSFER_IN/TRANSFER_OUT that moves a security rather than cash. The one
/// definition: `ActivityEconomicsResolver::is_security_transfer` and the
/// activity writer boundary both delegate here, so the rule cannot drift.
pub fn is_securities_transfer(activity_type: &str, resolved_asset_id: Option<&str>) -> bool {
    if activity_type != ACTIVITY_TYPE_TRANSFER_IN && activity_type != ACTIVITY_TYPE_TRANSFER_OUT {
        return false;
    }
    resolved_asset_id
        .map(str::trim)
        .is_some_and(|id| !id.is_empty() && !is_cash_symbol(id))
}

/// The final-cash contract's population: activity types whose `amount` is the
/// money that moved, and which therefore book zero cash when it is absent.
/// SPLIT and ADJUSTMENT carry no directed cash; a security transfer is
/// lot-only (its cash effect is the fee) and so is exempt too.
///
/// The single source of truth for "needs a final cash amount" — the activity
/// writer boundary, the legacy-cash migration, and the storage floor all call
/// this, so a new cash-bearing type cannot be added to one and missed by the
/// others.
pub fn requires_final_cash_amount(activity_type: &str, is_security_transfer: bool) -> bool {
    !is_security_transfer
        && matches!(
            activity_type,
            ACTIVITY_TYPE_BUY
                | ACTIVITY_TYPE_SELL
                | ACTIVITY_TYPE_DEPOSIT
                | ACTIVITY_TYPE_WITHDRAWAL
                | ACTIVITY_TYPE_DIVIDEND
                | ACTIVITY_TYPE_INTEREST
                | ACTIVITY_TYPE_CREDIT
                | ACTIVITY_TYPE_FEE
                | ACTIVITY_TYPE_TAX
                | ACTIVITY_TYPE_TRANSFER_IN
                | ACTIVITY_TYPE_TRANSFER_OUT
        )
}

/// Storage-floor backstop for the final-cash contract: a POSTED, cash-bearing
/// activity persisted with no stored amount books zero cash silently, so such
/// a row is only legal when it is flagged for review. The writer boundary is
/// supposed to make this state unreachable — this floor exists so any future
/// bypass (the CSV-import class) fails loudly at the repository instead of
/// silently mis-booking.
pub fn violates_final_cash_floor(
    effective_type: &str,
    asset_id: Option<&str>,
    status: &str,
    has_amount: bool,
    needs_review: bool,
) -> bool {
    if status != "POSTED" || has_amount || needs_review {
        return false;
    }
    requires_final_cash_amount(
        effective_type,
        is_securities_transfer(effective_type, asset_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── requires_symbol ─────────────────────────────────────────────────

    #[test]
    fn test_requires_symbol_true_for_symbol_types() {
        assert!(requires_symbol(ACTIVITY_TYPE_BUY));
        assert!(requires_symbol(ACTIVITY_TYPE_SELL));
        assert!(requires_symbol(ACTIVITY_TYPE_SPLIT));
        assert!(requires_symbol(ACTIVITY_TYPE_DIVIDEND));
        assert!(requires_symbol(ACTIVITY_TYPE_ADJUSTMENT));
    }

    #[test]
    fn test_requires_symbol_false_for_optional_types() {
        assert!(!requires_symbol(ACTIVITY_TYPE_DEPOSIT));
        assert!(!requires_symbol(ACTIVITY_TYPE_WITHDRAWAL));
        assert!(!requires_symbol(ACTIVITY_TYPE_INTEREST));
        assert!(!requires_symbol(ACTIVITY_TYPE_TRANSFER_IN));
        assert!(!requires_symbol(ACTIVITY_TYPE_TRANSFER_OUT));
        assert!(!requires_symbol(ACTIVITY_TYPE_TAX));
        assert!(!requires_symbol(ACTIVITY_TYPE_FEE));
        assert!(!requires_symbol(ACTIVITY_TYPE_CREDIT));
        assert!(!requires_symbol(ACTIVITY_TYPE_UNKNOWN));
        assert!(!requires_symbol("INVALID"));
        assert!(!requires_symbol(""));
    }

    #[test]
    fn test_price_bearing_types_exclude_transfers() {
        assert!(PRICE_BEARING_ACTIVITY_TYPES.contains(&ACTIVITY_TYPE_BUY));
        assert!(PRICE_BEARING_ACTIVITY_TYPES.contains(&ACTIVITY_TYPE_SELL));
        assert!(!PRICE_BEARING_ACTIVITY_TYPES.contains(&ACTIVITY_TYPE_TRANSFER_IN));
        assert!(!PRICE_BEARING_ACTIVITY_TYPES.contains(&ACTIVITY_TYPE_TRANSFER_OUT));
    }

    // ── is_cash_symbol ──────────────────────────────────────────────────

    #[test]
    fn test_is_cash_symbol_valid_patterns() {
        assert!(is_cash_symbol("$CASH-CAD"));
        assert!(is_cash_symbol("$CASH-USD"));
        assert!(is_cash_symbol("$CASH-EUR"));
        assert!(is_cash_symbol("CASH:USD"));
        assert!(is_cash_symbol("CASH:GBP"));
        assert!(is_cash_symbol("CASH-EUR"));
        assert!(is_cash_symbol("CASH_GBP"));
        assert!(is_cash_symbol("$cash-cad"));
        assert!(is_cash_symbol("Cash:usd"));
        assert!(is_cash_symbol("  $CASH-CAD  "));
    }

    #[test]
    fn test_is_cash_symbol_rejects_invalid() {
        assert!(!is_cash_symbol(""));
        assert!(!is_cash_symbol("   "));
        assert!(!is_cash_symbol("AAPL"));
        assert!(!is_cash_symbol("$CASH"));
        assert!(!is_cash_symbol("CASH"));
        assert!(!is_cash_symbol("$CASH-TOOLONG"));
        assert!(!is_cash_symbol("$CASH-12"));
        assert!(!is_cash_symbol("$CASH-A"));
        assert!(!is_cash_symbol("$CASH-AB"));
        assert!(!is_cash_symbol("$CASH-"));
        assert!(!is_cash_symbol("$CASHX-USD"));
        assert!(!is_cash_symbol("XCASH-USD"));
        assert!(!is_cash_symbol("----"));
        assert!(!is_cash_symbol("$FOO"));
        assert!(!is_cash_symbol("BTC-USD"));
    }

    // ── is_garbage_symbol ───────────────────────────────────────────────

    #[test]
    fn test_is_garbage_symbol() {
        assert!(is_garbage_symbol("----"));
        assert!(is_garbage_symbol("--"));
        assert!(is_garbage_symbol("-"));
        assert!(is_garbage_symbol("$FOO"));
        assert!(is_garbage_symbol("$123"));
        assert!(is_garbage_symbol("$"));
        assert!(is_garbage_symbol("$AAPL"));

        assert!(!is_garbage_symbol(""));
        assert!(!is_garbage_symbol("AAPL"));
        assert!(!is_garbage_symbol("BTC-USD"));
        assert!(!is_garbage_symbol("GOOG.TO"));
        assert!(!is_garbage_symbol("$CASH-CAD"));
        assert!(!is_garbage_symbol("$CASH-USD"));
        assert!(!is_garbage_symbol("CASH:EUR"));
    }
}
