//! # Trade-total ownership (the policy table)
//!
//! One table governs who owns a BUY/SELL `amount`, when the backend derives
//! it, when the confidence flag applies, and when an attestation
//! (`needs_review: Some(false)`) is honored. Every writer surface implements
//! exactly one row - adding a surface means filling in a row first, not
//! copying the nearest surface's code.
//!
//! | Source | `amount` supplied | Backend derives when | Mismatch flags review? | Attestation honored |
//! |---|---|---|---|---|
//! | Form create (desktop/mobile) | only if the user typed it | absent | no - every form save attests (the whole form is on screen) | yes - every submission |
//! | Form edit | stored total re-sent (custom) or `null` (calculated) | explicit `null` clear, or economics changed with no amount | no - form saves attest; Draft edits are stripped and keep flagging | yes - every submission except Draft edits |
//! | Grid | only when `_amountEdited` this session | economics cell edited (amount omitted) | yes; silently replacing a custom total always flags | yes (`_amountEdited`, non-Draft) |
//! | Manual batch / MCP commit | only if the user stated it | absent | no - attested by the user | yes |
//! | MCP update (as the grid) | only when the row states it | economics changed with no amount | yes; silently replacing a custom total always flags | yes (amount stated, non-Draft) |
//! | Migration (one-shot) | preserved, or derived under LEGACY semantics | reliable legacy inputs only | per legacy-shape rules in `activity_cash_migration` | n/a |
//!
//! Invariants every row obeys:
//! - An attested amount is never mutated: exact-final canonicalization aside,
//!   no rewrite and no gross-to-final conversion.
//! - A patch that changes neither the amount nor the economics never changes
//!   review state. A currency-only edit is deliberately not an economics
//!   change: it is a label correction on real data, and re-deriving from
//!   qty x price would replace an already-correct total.
//! - `unit_price`, `amount`, `fee`, and `tax` are all denominated in the
//!   activity currency. The asset quote currency belongs to market valuation,
//!   not transaction arithmetic; only the resolved asset multiplier affects
//!   qty x price. FX converts the resolved activity-currency cash effect into
//!   account currency later when required.
//! - The resolved asset owns the unit multiplier on every read path. Activity
//!   metadata can seed a newly created asset, but does not override it later.
//! - Drafts stay in the review queue until explicitly approved and posted.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use log::debug;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::accounts::{account_types, Account, AccountServiceTrait};
use crate::activities::activities_constants::{
    is_cash_symbol, is_garbage_symbol, is_securities_transfer, requires_final_cash_amount,
    requires_symbol, ACTIVITY_SUBTYPE_NOTIONAL_DISTRIBUTION, ACTIVITY_SUBTYPE_OPTION_EXPIRY,
    ACTIVITY_SUBTYPE_RETURN_OF_CAPITAL, ACTIVITY_TYPE_ADJUSTMENT, ACTIVITY_TYPE_BUY,
    ACTIVITY_TYPE_CREDIT, ACTIVITY_TYPE_FEE, ACTIVITY_TYPE_INTEREST, ACTIVITY_TYPE_SELL,
    ACTIVITY_TYPE_SPLIT, ACTIVITY_TYPE_TAX, ACTIVITY_TYPE_TRANSFER_IN, ACTIVITY_TYPE_TRANSFER_OUT,
    ACTIVITY_TYPE_WITHDRAWAL, PRICE_BEARING_ACTIVITY_TYPES,
};
use crate::activities::activities_errors::ActivityError;
use crate::activities::activities_model::*;

use crate::activities::idempotency::compute_idempotency_key;
use crate::activities::{
    ActivityRepositoryTrait, ActivityServiceTrait, TransferLinkState, TransferPair,
    TransferPairResolution,
};
use crate::assets::loan::LOAN_PAYMENT_TAG_KEY;
use crate::assets::{
    canonicalize_market_identity, normalize_quote_ccy_code, parse_crypto_pair_symbol,
    parse_symbol_with_known_exchange, resolve_bond_aliases, resolve_quote_ccy_precedence,
    AssetKind, AssetServiceTrait, AssetSpec, InstrumentType, QuoteCcyResolutionSource, QuoteMode,
};
use crate::errors::{DatabaseError, Error};
use crate::events::{DomainEvent, DomainEventSink, NoOpDomainEventSink};
use crate::fx::currency::{
    currency_minor_unit, get_normalization_rule, normalize_amount, resolve_currency,
};
use crate::fx::FxServiceTrait;
use crate::portfolio::economic_events::{ActivityCashInputs, ActivityEconomicsResolver};
use crate::quotes::constants::DATA_SOURCE_MANUAL;
use crate::quotes::{Quote, QuoteServiceTrait};
use crate::utils::time_utils::{calendar_day_instant, parse_user_timezone_or_default};
use crate::Result;
use log::warn;

use uuid::Uuid;
use wealthfolio_market_data::mic_to_currency;

/// What an update makes of a transfer leg when it turns it into a securities
/// transfer: its direction, the security and the quantity moved.
struct SecuritiesTransferLeg {
    activity_type: String,
    asset: String,
    quantity: Decimal,
}

impl SecuritiesTransferLeg {
    fn from_update(update: &ActivityUpdate) -> Option<Self> {
        let asset_input = update.asset.as_ref()?;
        let asset = asset_input
            .id
            .as_deref()
            .or(asset_input.symbol.as_deref())
            .map(str::trim)
            .filter(|asset| !asset.is_empty())?;
        if !is_securities_transfer(&update.activity_type, Some(asset)) {
            return None;
        }
        let quantity = update
            .quantity
            .flatten()
            .filter(|quantity| !quantity.is_zero())?;
        Some(Self {
            activity_type: update.activity_type.clone(),
            asset: asset.to_uppercase(),
            quantity: quantity.abs(),
        })
    }

    /// Whether the two legs are the opposite sides of one securities transfer.
    fn pairs_with(&self, other: &Self) -> bool {
        self.activity_type != other.activity_type
            && self.asset == other.asset
            && self.quantity == other.quantity
    }
}

struct InternalPairValues {
    source_amount: Decimal,
    destination_amount: Decimal,
    source_currency: String,
    destination_currency: String,
}

/// Service for managing activities
pub struct ActivityService {
    activity_repository: Arc<dyn ActivityRepositoryTrait>,
    account_service: Arc<dyn AccountServiceTrait>,
    asset_service: Arc<dyn AssetServiceTrait>,
    fx_service: Arc<dyn FxServiceTrait>,
    quote_service: Arc<dyn QuoteServiceTrait>,
    event_sink: Arc<dyn DomainEventSink>,
    timezone: Arc<RwLock<String>>,
}

/// Asset facts a trade update needs, fetched once per patch.
struct UpdateTradeFacts {
    /// Multiplier of the effective (possibly replacement) asset.
    unit_multiplier: Decimal,
    /// Multiplier of the row's current asset, for judging the stored amount.
    existing_unit_multiplier: Decimal,
    /// The patch swaps the instrument - by a different resolved id, or by a
    /// bare symbol that names something other than the current asset. An
    /// economics change even though `get_symbol_id()` may be empty (bare
    /// symbols resolve later, in prepare).
    replaces_asset: bool,
}

impl ActivityService {
    fn normalize_new_activity_economic_signs(activity: &mut NewActivity) {
        activity.quantity = activity.quantity.map(|v| v.abs());
        activity.unit_price = activity.unit_price.map(|v| v.abs());
        activity.amount = activity.amount.map(|v| v.abs());
        activity.fee = activity.fee.map(|v| v.abs());
        activity.tax = activity.tax.map(|v| v.abs());
    }

    fn is_trade_type(activity_type: &str) -> bool {
        matches!(activity_type, ACTIVITY_TYPE_BUY | ACTIVITY_TYPE_SELL)
    }

    // Thin constructor for `ActivityCashInputs`; the struct itself is the
    // named-argument form, so widening it past clippy's arity cap is fine.
    #[allow(clippy::too_many_arguments)]
    fn activity_cash_inputs<'a>(
        activity_type: &'a str,
        currency: &'a str,
        quantity: Option<Decimal>,
        unit_price: Option<Decimal>,
        amount: Option<Decimal>,
        fee: Option<Decimal>,
        tax: Option<Decimal>,
        unit_multiplier: Decimal,
    ) -> ActivityCashInputs<'a> {
        ActivityCashInputs {
            activity_type,
            currency,
            is_security_transfer: false,
            quantity,
            unit_price,
            amount,
            fee,
            tax,
            unit_multiplier,
        }
    }

    fn calculated_missing_final_amount(
        activity_type: &str,
        subtype: Option<&str>,
        quantity: Option<Decimal>,
        unit_price: Option<Decimal>,
        fee: Option<Decimal>,
        tax: Option<Decimal>,
        unit_multiplier: Decimal,
    ) -> Option<Decimal> {
        // Derivation only - the currency-scaled tolerance is never consulted.
        let inputs = Self::activity_cash_inputs(
            activity_type,
            "",
            quantity,
            unit_price,
            None,
            fee,
            tax,
            unit_multiplier,
        );
        if Self::is_trade_type(activity_type) {
            ActivityEconomicsResolver::calculate_trade_final_cash(inputs)
        } else if matches!(activity_type, ACTIVITY_TYPE_FEE | ACTIVITY_TYPE_TAX) {
            ActivityEconomicsResolver::calculate_standalone_charge_amount(inputs)
        } else {
            ActivityEconomicsResolver::calculate_composite_final_cash(
                activity_type,
                subtype,
                quantity,
                unit_price,
                unit_multiplier,
            )
        }
    }

    /// Canonicalizes a supplied trade total and reports whether it needs
    /// review. `amount` is passed separately from `inputs` so the caller's
    /// "an explicit amount exists" guarantee is carried by the type.
    /// `allow_gross_conversion` is false when the caller honors an explicit
    /// attestation: an attested total is the user's number and must never be
    /// rewritten, even when it happens to equal the derived gross (fee
    /// waived, or the fee booked as its own FEE activity).
    fn mark_trade_amount_confidence(
        currency: &str,
        amount: Decimal,
        inputs: ActivityCashInputs<'_>,
        allow_gross_conversion: bool,
    ) -> (Decimal, bool) {
        if let Some(expected) = ActivityEconomicsResolver::calculate_trade_final_cash(inputs) {
            // Half a minor unit: absorbs genuine rounding artifacts while a
            // deliberate one-cent difference stays the user's value and flags.
            let tolerance = currency_minor_unit(currency) / Decimal::TWO;
            if (amount.abs() - expected).abs() <= tolerance {
                return (expected, false);
            }
            // A total matching the derived gross is the pre-fee number from a
            // gross-reporting source; convert it to final exactly like the
            // migration does instead of storing it short by the charges.
            if allow_gross_conversion
                && ActivityEconomicsResolver::derived_positive_gross(inputs)
                    .is_some_and(|gross| (amount.abs() - gross).abs() <= tolerance)
            {
                return (expected, false);
            }
            return (amount.abs(), true);
        }

        let has_charges = inputs.fee.is_some_and(|value| !value.is_zero())
            || inputs.tax.is_some_and(|value| !value.is_zero());
        (amount.abs(), has_charges)
    }

    /// Canonical writer boundary. Ordinary cash requires an explicit amount;
    /// only trades, standalone charges, and recognized composites can fill a
    /// missing value. `honor_explicit_review_clear` is true for manual saves,
    /// where an incoming `needs_review: Some(false)` attests that the user
    /// confirmed the custom total against the calculated one.
    ///
    /// All monetary inputs are denominated in `activity.currency`; the asset
    /// quote currency is deliberately irrelevant here. The resolved asset is
    /// consulted only for its contract multiplier.
    fn ensure_new_activity_final_amount(
        activity: &mut NewActivity,
        resolved_asset_id: Option<&str>,
        unit_multiplier: Decimal,
        honor_explicit_review_clear: bool,
    ) {
        if activity.activity_type == ACTIVITY_TYPE_SPLIT {
            return;
        }

        if is_securities_transfer(&activity.activity_type, resolved_asset_id) {
            return;
        }

        if let Some(amount) = activity.amount {
            if Self::is_trade_type(&activity.activity_type) {
                let inputs = Self::activity_cash_inputs(
                    &activity.activity_type,
                    &activity.currency,
                    activity.quantity,
                    activity.unit_price,
                    Some(amount),
                    activity.fee,
                    activity.tax,
                    unit_multiplier,
                );
                let attested = honor_explicit_review_clear && activity.needs_review == Some(false);
                let (canonical, needs_review) = Self::mark_trade_amount_confidence(
                    &activity.currency,
                    amount,
                    inputs,
                    !attested,
                );
                activity.amount = Some(canonical);
                if needs_review
                    && !(honor_explicit_review_clear && activity.needs_review == Some(false))
                {
                    activity.needs_review = Some(true);
                }
            } else if matches!(
                activity.activity_type.as_str(),
                ACTIVITY_TYPE_FEE | ACTIVITY_TYPE_TAX
            ) && amount.is_zero()
            {
                // Legacy import shape: charges carried as amount=0 with the
                // value in fee/tax. The amount IS the charge; derive it.
                if let Some(derived) = Self::calculated_missing_final_amount(
                    &activity.activity_type,
                    activity.subtype.as_deref(),
                    activity.quantity,
                    activity.unit_price,
                    activity.fee,
                    activity.tax,
                    unit_multiplier,
                )
                .filter(|value| !value.is_zero())
                {
                    activity.amount = Some(derived);
                }
            }
            return;
        }

        activity.amount = Self::calculated_missing_final_amount(
            &activity.activity_type,
            activity.subtype.as_deref(),
            activity.quantity,
            activity.unit_price,
            activity.fee,
            activity.tax,
            unit_multiplier,
        );
    }

    /// Asset facts an update patch needs, fetched at most twice (existing
    /// asset plus a replacement, when one is named). A replacement asset that
    /// carries a symbol but no resolved id does not exist yet, so the
    /// multiplier must come from the patch's instrument facts - falling back
    /// to the existing asset would price an option or bond edit at the old
    /// asset's scale.
    fn update_trade_facts(
        &self,
        activity: &ActivityUpdate,
        existing: &Activity,
    ) -> UpdateTradeFacts {
        let existing_asset = existing
            .asset_id
            .as_deref()
            .and_then(|asset_id| self.asset_service.get_asset_by_id(asset_id).ok());
        // The asset owns the contract multiplier, so an edit reprices at the
        // asset's value and never at the row's metadata: that metadata is a
        // creation-time seed, not a standing override.
        let existing_unit_multiplier = existing_asset
            .as_ref()
            .map(|asset| asset.contract_multiplier())
            .unwrap_or(Decimal::ONE);

        if let Some(asset_id) = activity.get_symbol_id() {
            if Some(asset_id) == existing.asset_id.as_deref() {
                return UpdateTradeFacts {
                    unit_multiplier: existing_unit_multiplier,
                    existing_unit_multiplier,
                    replaces_asset: false,
                };
            }
            let asset = self.asset_service.get_asset_by_id(asset_id).ok();
            return UpdateTradeFacts {
                unit_multiplier: asset
                    .as_ref()
                    .map(|asset| asset.contract_multiplier())
                    .unwrap_or(Decimal::ONE),
                existing_unit_multiplier,
                replaces_asset: true,
            };
        }
        let patch_symbol = activity
            .get_asset_symbol()
            .map(str::trim)
            .filter(|symbol| !symbol.is_empty());
        if let Some(symbol) = patch_symbol {
            // A bare symbol resolves (or is created) later, in prepare;
            // whether it replaces the instrument is decided here by name so
            // the swap counts as an economics change even without an id.
            let names_current_asset = existing_asset.as_ref().is_some_and(|asset| {
                asset
                    .display_code
                    .as_deref()
                    .is_some_and(|code| code.eq_ignore_ascii_case(symbol))
                    || asset
                        .instrument_symbol
                        .as_deref()
                        .is_some_and(|code| code.eq_ignore_ascii_case(symbol))
            });
            if !names_current_asset {
                let instrument_type = activity.get_instrument_type().unwrap_or_default();
                let unit_multiplier = crate::assets::instrument_default_multiplier(
                    instrument_type.eq_ignore_ascii_case("OPTION"),
                    false,
                );
                return UpdateTradeFacts {
                    unit_multiplier,
                    existing_unit_multiplier,
                    replaces_asset: true,
                };
            }
        }
        UpdateTradeFacts {
            unit_multiplier: existing_unit_multiplier,
            existing_unit_multiplier,
            replaces_asset: false,
        }
    }

    fn missing_final_cash_error(activity_type: &str) -> crate::errors::Error {
        ActivityError::InvalidData(format!(
            "{} activities require a final cash amount",
            activity_type
        ))
        .into()
    }

    fn validate_new_activity_final_amount(
        activity: &NewActivity,
        resolved_asset_id: Option<&str>,
    ) -> Result<()> {
        let is_security_transfer =
            is_securities_transfer(&activity.activity_type, resolved_asset_id);
        if requires_final_cash_amount(&activity.activity_type, is_security_transfer)
            && activity.amount.is_none()
        {
            return Err(Self::missing_final_cash_error(&activity.activity_type));
        }
        Ok(())
    }

    fn hydrate_and_validate_update_against_existing(
        &self,
        activity: &mut ActivityUpdate,
        existing: &Activity,
    ) -> Result<()> {
        // PATCH semantics: omission preserves the current asset. Hydrate the
        // existing id before validation/resolution; an explicit empty object
        // remains empty and is handled as a clear by the repository.
        if activity.asset.is_none() {
            activity.asset = existing
                .asset_id
                .as_ref()
                .map(|asset_id| AssetResolutionInput {
                    id: Some(asset_id.clone()),
                    ..Default::default()
                });
        }

        // A metadata patch carries only the keys its writer owns (e.g. the
        // option form's contract multiplier); merge it over the stored blob
        // so unrelated keys - the migration's legacy_amount breadcrumb,
        // transfer flow flags - survive instead of being wiped by a wholesale
        // replace. Patch keys win.
        if let Some(patch) = activity.metadata.as_deref() {
            activity.metadata = Some(Self::merge_metadata_patch(
                existing.metadata.as_ref(),
                patch,
            ));
        }

        if activity
            .subtype
            .as_deref()
            .map(str::trim)
            .is_some_and(|subtype| !subtype.is_empty())
        {
            activity.subtype = NewActivity::canonicalize_subtype_for_activity(
                &activity.activity_type,
                activity.subtype.as_deref(),
            );
        }

        let effective_subtype = match activity.subtype.as_deref().map(str::trim) {
            Some("") => None,
            Some(subtype) => Some(subtype),
            None => existing.subtype.as_deref(),
        };
        // Normalize before classifying transfer economics: a cash repair must
        // preserve its final amount and pass cash validation. An omitted
        // subtype still carries the stored asset requirement.
        if let Some(currency) = Self::cash_symbol_currency(
            activity.asset.as_ref(),
            &activity.activity_type,
            effective_subtype,
        ) {
            if activity.currency.trim().is_empty() {
                activity.currency = currency;
            }
            // Omission preserves the stored asset; an empty patch clears it.
            activity.asset = Some(AssetResolutionInput::default());
        }
        let effective_asset_id = match activity.asset.as_ref() {
            Some(asset) if asset.is_empty() => None,
            _ => activity
                .get_symbol_id()
                .map(str::to_string)
                .or_else(|| existing.asset_id.clone()),
        };
        let is_security_transfer =
            is_securities_transfer(&activity.activity_type, effective_asset_id.as_deref());
        let quantity = activity
            .quantity
            .unwrap_or(existing.quantity)
            .map(|value| value.abs());
        let unit_price = activity
            .unit_price
            .unwrap_or(existing.unit_price)
            .map(|value| value.abs());
        let existing_amount = existing.amount.map(|value| value.abs());
        let mut amount = activity
            .amount
            .unwrap_or(existing_amount)
            .map(|value| value.abs());
        let fee = activity
            .fee
            .unwrap_or(existing.fee)
            .map(|value| value.abs());
        let tax = activity
            .tax
            .unwrap_or(existing.tax)
            .map(|value| value.abs());

        if is_security_transfer {
            if self.should_clear_stale_security_transfer_amount(activity, existing) {
                activity.amount = Some(None);
                amount = None;
            }
        } else if Self::is_trade_type(&activity.activity_type) {
            let trade_facts = self.update_trade_facts(activity, existing);
            let unit_multiplier = trade_facts.unit_multiplier;
            // The activity currency is deliberately absent from this list: a
            // currency-only edit is a label correction on real data, and
            // treating it as an economics change would re-derive a total
            // that is already correct.
            let economics_changed = activity.activity_type != existing.effective_type()
                || trade_facts.replaces_asset
                || Self::decimal_patch_changes(activity.quantity, existing.quantity)
                || Self::decimal_patch_changes(activity.unit_price, existing.unit_price)
                || Self::decimal_patch_changes(activity.fee, existing.fee)
                || Self::decimal_patch_changes(activity.tax, existing.tax);

            let explicit_amount = activity.amount.flatten();
            let amount_patch_changed =
                Self::decimal_patch_changes(activity.amount, existing.amount);
            let explicit_clear = matches!(activity.amount, Some(None));
            let should_recalculate =
                explicit_amount.is_none() && (economics_changed || explicit_clear);
            if should_recalculate {
                if let Some(calculated) = Self::calculated_missing_final_amount(
                    &activity.activity_type,
                    effective_subtype,
                    quantity,
                    unit_price,
                    fee,
                    tax,
                    unit_multiplier,
                ) {
                    // An economics-only patch that never mentioned the
                    // amount may be replacing a deliberate custom total;
                    // surface that instead of letting it vanish silently.
                    // An explicit clear is the user choosing the
                    // calculation and stays quiet.
                    let replaces_custom_total = !explicit_clear
                        && existing_amount.is_some_and(|stored| {
                            Self::calculated_missing_final_amount(
                                existing.effective_type(),
                                existing.subtype.as_deref(),
                                existing.quantity,
                                existing.unit_price,
                                existing.fee,
                                existing.tax,
                                trade_facts.existing_unit_multiplier,
                            )
                            .is_none_or(|expected| {
                                (stored - expected).abs()
                                    > currency_minor_unit(&activity.currency) / Decimal::TWO
                            })
                        });
                    if replaces_custom_total && activity.needs_review != Some(false) {
                        activity.needs_review = Some(true);
                    }
                    activity.amount = Some(Some(calculated));
                    amount = Some(calculated);
                }
            }

            if let Some(current_amount) = explicit_amount.or(amount) {
                let inputs = Self::activity_cash_inputs(
                    &activity.activity_type,
                    &activity.currency,
                    quantity,
                    unit_price,
                    Some(current_amount),
                    fee,
                    tax,
                    unit_multiplier,
                );
                let (canonical, needs_review) = Self::mark_trade_amount_confidence(
                    &activity.currency,
                    current_amount,
                    inputs,
                    activity.needs_review != Some(false),
                );
                if explicit_amount.is_some() || should_recalculate {
                    activity.amount = Some(Some(canonical));
                    amount = Some(canonical);
                }
                // An explicit clear is a human approval and outranks the
                // recomputed confidence flag. A patch that touched neither
                // the total nor its economics keeps the reviewed state
                // instead of re-entering the queue.
                if needs_review
                    && activity.needs_review != Some(false)
                    && (economics_changed || amount_patch_changed)
                {
                    activity.needs_review = Some(true);
                }
            }
        } else if amount.is_none() {
            // Non-trades read the asset facts only to fill a missing
            // composite amount; a patch that keeps its amount skips the
            // asset fetch entirely.
            let unit_multiplier = self.update_trade_facts(activity, existing).unit_multiplier;
            if let Some(calculated) = Self::calculated_missing_final_amount(
                &activity.activity_type,
                effective_subtype,
                quantity,
                unit_price,
                fee,
                tax,
                unit_multiplier,
            ) {
                activity.amount = Some(Some(calculated));
                amount = Some(calculated);
            }
        }

        let effective_status = activity
            .status
            .clone()
            .unwrap_or_else(|| existing.status.clone());
        let effective_review = activity.needs_review.unwrap_or(existing.needs_review);
        if existing.status == ActivityStatus::Draft
            && existing.needs_review
            && effective_status == ActivityStatus::Draft
            && !effective_review
        {
            return Err(Self::invalid_activity_data(
                "Draft activities must remain in review until explicitly approved and posted",
            ));
        }
        if existing.status == ActivityStatus::Draft
            && effective_status == ActivityStatus::Posted
            && effective_review
        {
            return Err(Self::invalid_activity_data(
                "Approving a Draft must clear its review flag",
            ));
        }
        if requires_final_cash_amount(&activity.activity_type, is_security_transfer)
            && amount.is_none()
            && (effective_status == ActivityStatus::Posted || !effective_review)
        {
            return Err(Self::missing_final_cash_error(&activity.activity_type));
        }

        NewActivity::validate_asset_backed_income_values(
            &activity.activity_type,
            effective_subtype,
            quantity,
            unit_price,
            amount,
        )?;

        Ok(())
    }

    fn should_clear_stale_security_transfer_amount(
        &self,
        activity: &ActivityUpdate,
        existing: &Activity,
    ) -> bool {
        if activity.amount.is_some() {
            return false;
        }
        let asset_id = match activity.asset.as_ref() {
            Some(asset) if asset.is_empty() => None,
            _ => activity.get_symbol_id().or(existing.asset_id.as_deref()),
        };
        if !is_securities_transfer(&activity.activity_type, asset_id)
            || self.is_bond_asset(asset_id)
        {
            return false;
        }
        let effective_quantity = activity.quantity.unwrap_or(existing.quantity);
        let effective_unit_price = activity.unit_price.unwrap_or(existing.unit_price);
        if effective_quantity.is_none_or(|quantity| quantity.is_zero())
            || effective_unit_price.is_none_or(|unit_price| unit_price.is_zero())
        {
            return false;
        }

        activity.account_id != existing.account_id
            || !activity.currency.eq_ignore_ascii_case(&existing.currency)
            || Self::decimal_patch_changes(activity.quantity, existing.quantity)
            || Self::decimal_patch_changes(activity.unit_price, existing.unit_price)
            || Self::decimal_patch_changes(activity.fee, existing.fee)
            || Self::decimal_patch_changes(activity.tax, existing.tax)
            || Self::decimal_patch_changes(activity.fx_rate, existing.fx_rate)
    }

    fn is_bond_asset(&self, asset_id: Option<&str>) -> bool {
        asset_id
            .and_then(|asset_id| self.asset_service.get_asset_by_id(asset_id).ok())
            .is_some_and(|asset| asset.instrument_type == Some(InstrumentType::Bond))
    }

    fn decimal_patch_changes(patch: Option<Option<Decimal>>, existing: Option<Decimal>) -> bool {
        match patch {
            None => false,
            Some(value) => {
                value.map(|decimal| decimal.abs()) != existing.map(|decimal| decimal.abs())
            }
        }
    }

    fn validate_new_activity_income_values(activity: &NewActivity) -> Result<()> {
        NewActivity::validate_asset_backed_income_values(
            &activity.activity_type,
            activity.subtype.as_deref(),
            activity.quantity,
            activity.unit_price,
            activity.amount,
        )?;

        Ok(())
    }

    /// A create whose asset only names the account's cash books cash: drop the
    /// asset, keeping the symbol's currency when the row states none.
    fn drop_cash_symbol_asset(activity: &mut NewActivity) {
        if let Some(currency) = Self::cash_symbol_currency(
            activity.asset.as_ref(),
            &activity.activity_type,
            activity.subtype.as_deref(),
        ) {
            if activity.currency.trim().is_empty() {
                activity.currency = currency;
            }
            activity.asset = None;
        }
    }

    fn normalize_activity_for_preparation(mut activity: NewActivity) -> NewActivity {
        activity.subtype = NewActivity::canonicalize_subtype_for_activity(
            &activity.activity_type,
            activity.subtype.as_deref(),
        );
        Self::drop_cash_symbol_asset(&mut activity);
        Self::normalize_new_activity_economic_signs(&mut activity);
        activity
    }

    /// Currency of a reserved cash placeholder on a symbol-optional activity.
    /// A concrete asset id or an asset-backed subtype keeps asset resolution.
    fn cash_symbol_currency(
        asset: Option<&AssetResolutionInput>,
        activity_type: &str,
        subtype: Option<&str>,
    ) -> Option<String> {
        fn non_empty(value: Option<&str>) -> Option<&str> {
            value.map(str::trim).filter(|v| !v.is_empty())
        }
        let asset = asset?;
        let symbol = non_empty(asset.symbol.as_deref())?;
        if !is_cash_symbol(symbol)
            || non_empty(asset.id.as_deref()).is_some_and(|id| !is_cash_symbol(id))
            || Self::requires_asset_identity(activity_type, subtype)
        {
            return None;
        }
        // Use the same uppercase spelling as is_cash_symbol and split at the
        // currency separator, without slicing the original UTF-8 input.
        symbol
            .to_uppercase()
            .rsplit(['-', '_', ':'])
            .next()
            .map(str::to_string)
    }

    fn requires_asset_identity(activity_type: &str, subtype: Option<&str>) -> bool {
        if activity_type.eq_ignore_ascii_case(ACTIVITY_TYPE_ADJUSTMENT) {
            let subtype = NewActivity::canonicalize_subtype_for_activity(activity_type, subtype);
            return subtype.as_deref().is_some_and(|subtype| {
                [
                    ACTIVITY_SUBTYPE_OPTION_EXPIRY,
                    ACTIVITY_SUBTYPE_RETURN_OF_CAPITAL,
                    ACTIVITY_SUBTYPE_NOTIONAL_DISTRIBUTION,
                ]
                .iter()
                .any(|known| subtype.eq_ignore_ascii_case(known))
            });
        }
        requires_symbol(activity_type)
            || NewActivity::is_asset_backed_income_subtype(activity_type, subtype)
    }

    fn has_valid_split_ratio(amount: Option<Decimal>) -> bool {
        amount.is_some_and(|amount| amount.is_sign_positive() && !amount.is_zero())
    }

    fn split_ratio_error() -> crate::errors::Error {
        ActivityError::InvalidData("Split activities require a positive amount ratio".to_string())
            .into()
    }

    fn validate_split_ratio(activity_type: &str, amount: Option<Decimal>) -> Result<()> {
        if activity_type == ACTIVITY_TYPE_SPLIT && !Self::has_valid_split_ratio(amount) {
            return Err(Self::split_ratio_error());
        }
        Ok(())
    }

    fn validate_split_ratio_update(
        &self,
        activity_id: &str,
        activity_type: &str,
        amount: Option<Option<Decimal>>,
    ) -> Result<()> {
        if activity_type != ACTIVITY_TYPE_SPLIT {
            return Ok(());
        }

        match amount {
            Some(Some(amount)) if Self::has_valid_split_ratio(Some(amount)) => Ok(()),
            Some(_) => Err(Self::split_ratio_error()),
            None => {
                let existing = self.activity_repository.get_activity(activity_id)?;
                if existing.activity_type == ACTIVITY_TYPE_SPLIT
                    && Self::has_valid_split_ratio(existing.amount)
                {
                    Ok(())
                } else {
                    Err(Self::split_ratio_error())
                }
            }
        }
    }

    /// `asset` is the request's asset id or symbol, before resolution.
    fn account_activity_validation_message(
        activity_type: &str,
        asset: Option<&str>,
        account: &Account,
    ) -> Option<String> {
        if account.account_type != account_types::CREDIT_CARD {
            return None;
        }

        // A card holds no positions, so a transfer on it moves cash only.
        if is_securities_transfer(activity_type, asset) {
            return Some("Securities transfers are not supported for credit card accounts".into());
        }

        // Charges, payments, refunds, fees and interest, plus TRANSFER_OUT for
        // money a card sends elsewhere: a balance transfer, a cash advance, a
        // wallet top-up. It books as more owed, like a charge, but is not one.
        match activity_type {
            ACTIVITY_TYPE_WITHDRAWAL
            | ACTIVITY_TYPE_TRANSFER_IN
            | ACTIVITY_TYPE_TRANSFER_OUT
            | ACTIVITY_TYPE_CREDIT
            | ACTIVITY_TYPE_FEE
            | ACTIVITY_TYPE_INTEREST => None,
            _ => Some(format!(
                "{} activities are not supported for credit card accounts",
                activity_type
            )),
        }
    }

    fn validate_activity_allowed_for_account(
        activity_type: &str,
        asset: Option<&str>,
        account: &Account,
    ) -> Result<()> {
        if let Some(message) =
            Self::account_activity_validation_message(activity_type, asset, account)
        {
            return Err(ActivityError::InvalidData(message).into());
        }
        Ok(())
    }

    fn duplicate_activity_error(existing_activity_id: Option<&str>) -> crate::errors::Error {
        // One typed variant carries the same legacy display text for both the
        // id and no-id cases; callers switch on the variant, never the text.
        ActivityError::duplicate(existing_activity_id).into()
    }

    fn map_duplicate_idempotency_violation(err: crate::errors::Error) -> crate::errors::Error {
        match err {
            crate::errors::Error::Database(crate::errors::DatabaseError::UniqueViolation(
                message,
            )) if message.contains("activities.idempotency_key") => {
                Self::duplicate_activity_error(None)
            }
            crate::errors::Error::Database(crate::errors::DatabaseError::Internal(message))
                if message.contains("activities.idempotency_key")
                    || message.contains("UNIQUE constraint failed: activities.idempotency_key") =>
            {
                Self::duplicate_activity_error(None)
            }
            other => other,
        }
    }

    fn parse_instrument_type(value: Option<&str>) -> Option<InstrumentType> {
        match value?.trim().to_uppercase().as_str() {
            "EQUITY" | "STOCK" | "ETF" | "MUTUALFUND" | "MUTUAL_FUND" | "INDEX" | "FUTURE"
            | "FUTURES" => Some(InstrumentType::Equity),
            "CRYPTO" | "CRYPTOCURRENCY" => Some(InstrumentType::Crypto),
            "FX" | "FOREX" | "CURRENCY" => Some(InstrumentType::Fx),
            "OPTION" => Some(InstrumentType::Option),
            "METAL" | "COMMODITY" => Some(InstrumentType::Metal),
            "BOND" | "FIXEDINCOME" | "FIXED_INCOME" | "DEBT" | "MONEYMARKET" => {
                Some(InstrumentType::Bond)
            }
            _ => None,
        }
    }

    fn normalize_quote_ccy(value: Option<&str>) -> Option<String> {
        let trimmed = value.map(str::trim).filter(|s| !s.is_empty())?;
        if trimmed.eq_ignore_ascii_case("GBP") {
            return Some("GBP".to_string());
        }
        if trimmed == "GBp" {
            return Some("GBp".to_string());
        }
        if trimmed.eq_ignore_ascii_case("GBX") {
            return Some("GBX".to_string());
        }
        if trimmed == "ZAc" || trimmed.eq_ignore_ascii_case("ZAC") {
            return Some("ZAc".to_string());
        }
        if !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        if !(3..=5).contains(&trimmed.len()) {
            return None;
        }
        Some(trimmed.to_uppercase())
    }

    fn kind_from_instrument_type(instrument_type: &InstrumentType) -> AssetKind {
        match instrument_type {
            InstrumentType::Fx => AssetKind::Fx,
            _ => AssetKind::Investment,
        }
    }

    fn existing_asset_quote_ccy_by_id(&self, asset_id: Option<&str>) -> Option<String> {
        let id = asset_id?.trim();
        if id.is_empty() {
            return None;
        }
        self.asset_service
            .get_asset_by_id(id)
            .ok()
            .and_then(|asset| normalize_quote_ccy_code(Some(asset.quote_ccy.as_str())))
    }

    #[allow(clippy::too_many_arguments)]
    async fn resolve_quote_ccy(
        &self,
        symbol: &str,
        exchange_mic: Option<&str>,
        instrument_type: Option<&InstrumentType>,
        explicit_quote_ccy: Option<&str>,
        existing_asset_quote_ccy: Option<&str>,
        terminal_fallback: &str,
        allow_provider_lookup: bool,
    ) -> (String, QuoteCcyResolutionSource) {
        let has_deterministic_precedence = normalize_quote_ccy_code(explicit_quote_ccy).is_some()
            || normalize_quote_ccy_code(existing_asset_quote_ccy).is_some();
        let provider_quote_ccy = if allow_provider_lookup && !has_deterministic_precedence {
            self.quote_service
                .resolve_symbol_quote(symbol, exchange_mic, instrument_type, None, None, None)
                .await
                .ok()
                .and_then(|q| q.currency)
        } else {
            None
        };

        resolve_quote_ccy_precedence(
            explicit_quote_ccy,
            existing_asset_quote_ccy,
            provider_quote_ccy.as_deref(),
            exchange_mic.and_then(mic_to_currency),
            Some(terminal_fallback),
        )
        .unwrap_or_else(|| {
            (
                terminal_fallback.to_string(),
                QuoteCcyResolutionSource::TerminalFallback,
            )
        })
    }

    /// Creates a new ActivityService instance with injected dependencies
    pub fn new(
        activity_repository: Arc<dyn ActivityRepositoryTrait>,
        account_service: Arc<dyn AccountServiceTrait>,
        asset_service: Arc<dyn AssetServiceTrait>,
        fx_service: Arc<dyn FxServiceTrait>,
        quote_service: Arc<dyn QuoteServiceTrait>,
    ) -> Self {
        Self {
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
            event_sink: Arc::new(NoOpDomainEventSink),
            timezone: Arc::new(RwLock::new("UTC".to_string())),
        }
    }

    fn parse_activity_timestamp_utc(activity_date: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(activity_date)
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDate::parse_from_str(activity_date, "%Y-%m-%d").map(|date| {
                    Utc.from_utc_datetime(
                        &date.and_hms_opt(0, 0, 0).expect("00:00:00 is a valid time"),
                    )
                })
            })
            .ok()
    }

    fn earliest_activity_at_utc<'a>(
        activities: impl IntoIterator<Item = &'a Activity>,
    ) -> Option<DateTime<Utc>> {
        activities
            .into_iter()
            .map(|activity| activity.activity_date)
            .min()
    }

    fn emit_activities_changed(
        &self,
        account_ids: Vec<String>,
        asset_ids: Vec<String>,
        currencies: Vec<String>,
        earliest_activity_at_utc: Option<DateTime<Utc>>,
    ) {
        self.event_sink.emit(DomainEvent::activities_changed(
            account_ids,
            asset_ids,
            currencies,
            earliest_activity_at_utc,
        ));
    }

    fn emit_asset_split_activities_changed<'a>(
        &self,
        activities: impl IntoIterator<Item = &'a Activity>,
    ) {
        let split_activities: Vec<&Activity> = activities
            .into_iter()
            .filter(|activity| activity.effective_type() == ACTIVITY_TYPE_SPLIT)
            .collect();
        let asset_ids: HashSet<String> = split_activities
            .iter()
            .filter_map(|activity| activity.asset_id.clone())
            .collect();
        if asset_ids.is_empty() {
            return;
        }

        self.event_sink
            .emit(DomainEvent::asset_split_activities_changed(
                asset_ids.into_iter().collect(),
                Self::earliest_activity_at_utc(split_activities),
            ));
    }

    /// Sets the domain event sink for this service.
    ///
    /// Events are emitted after successful mutations (create, update, delete).
    pub fn with_event_sink(mut self, event_sink: Arc<dyn DomainEventSink>) -> Self {
        self.event_sink = event_sink;
        self
    }

    pub fn with_timezone(mut self, timezone: Arc<RwLock<String>>) -> Self {
        self.timezone = timezone;
        self
    }

    fn configured_timezone(&self) -> Tz {
        let configured_timezone = self
            .timezone
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        parse_user_timezone_or_default(&configured_timezone)
    }

    /// Validates a submitted activity date and normalizes it for storage. A bare
    /// `YYYY-MM-DD` names a calendar day: it is stored at [`calendar_day_instant`]
    /// in the configured timezone, so the day it shows there is the day given.
    /// UTC midnight, which is where a bare date used to land, falls on the
    /// previous day west of UTC. Timestamps keep their instant.
    fn validate_and_normalize_activity_date(&self, activity_date: &str) -> Result<String> {
        let activity_date = activity_date.trim();
        let timezone = self.configured_timezone();
        if let Ok(day) = NaiveDate::parse_from_str(activity_date, "%Y-%m-%d") {
            validate_activity_date_in_timezone(activity_date, timezone)?;
            let instant = calendar_day_instant(day, timezone);
            // At or east of UTC that is UTC midnight: keep the date as given.
            return Ok(if instant.time() == chrono::NaiveTime::MIN {
                activity_date.to_string()
            } else {
                instant.with_timezone(&timezone).to_rfc3339()
            });
        }
        Self::validate_activity_timestamp(activity_date, timezone)
    }

    fn validate_activity_timestamp(activity_date: &str, timezone: Tz) -> Result<String> {
        validate_activity_date_in_timezone(activity_date, timezone)?;

        // Preserve the submitted timestamp whenever its own calendar date is
        // already supported. Re-encode only the boundary case where the
        // configured timezone makes the date valid but the submitted offset
        // does not; repository validation can then reach the same conclusion.
        if validate_activity_date(activity_date).is_ok() {
            return Ok(activity_date.to_string());
        }

        match DateTime::parse_from_rfc3339(activity_date) {
            Ok(date) => Ok(date.with_timezone(&timezone).to_rfc3339()),
            Err(_) => NaiveDate::parse_from_str(activity_date, "%Y-%m-%d")
                .map(|date| date.format("%Y-%m-%d").to_string())
                .map_err(|error| ActivityError::InvalidData(error.to_string()).into()),
        }
    }

    fn invalid_activity_data(message: impl Into<String>) -> Error {
        ActivityError::InvalidData(message.into()).into()
    }

    fn internal_transfer_metadata() -> Option<String> {
        Some(
            serde_json::json!({
                "flow": { "is_external": false },
                "transfer": {
                    "source": "wealthfolio",
                    "kind": "internal_cash"
                }
            })
            .to_string(),
        )
    }

    fn activity_asset_input(activity: &Activity) -> Option<AssetResolutionInput> {
        activity
            .asset_id
            .as_ref()
            .map(|asset_id| AssetResolutionInput {
                id: Some(asset_id.clone()),
                ..Default::default()
            })
    }

    fn add_activity_to_event_sets(
        activity: &Activity,
        account_ids: &mut HashSet<String>,
        asset_ids: &mut HashSet<String>,
        currencies: &mut HashSet<String>,
    ) {
        account_ids.insert(activity.account_id.clone());
        if let Some(asset_id) = activity.asset_id.as_ref() {
            asset_ids.insert(asset_id.clone());
        }
        currencies.insert(activity.currency.clone());
    }

    fn transfer_group_is_legacy_wealthfolio(group_id: &str) -> bool {
        group_id.starts_with("wf-transfer-")
    }

    fn activity_has_internal_transfer_marker(activity: &Activity, group_id: &str) -> bool {
        let explicit_internal = activity
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("flow"))
            .and_then(|flow| flow.get("is_external"))
            .and_then(|value| value.as_bool())
            == Some(false);

        let wealthfolio_internal = activity
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("transfer"))
            .and_then(|transfer| transfer.get("source"))
            .and_then(|value| value.as_str())
            == Some("wealthfolio");

        explicit_internal
            || wealthfolio_internal
            || Self::transfer_group_is_legacy_wealthfolio(group_id)
    }

    fn is_valid_internal_transfer_pair(pair: &TransferPair) -> bool {
        if pair.group_id.trim().is_empty() {
            return false;
        }

        Self::activity_has_internal_transfer_marker(&pair.transfer_in, &pair.group_id)
            && Self::activity_has_internal_transfer_marker(&pair.transfer_out, &pair.group_id)
    }

    fn is_cash_transfer_pair(pair: &TransferPair) -> bool {
        pair.transfer_in.asset_id.is_none() && pair.transfer_out.asset_id.is_none()
    }

    fn transfer_pair_response(pair: TransferPair) -> InternalTransferPairResponse {
        InternalTransferPairResponse {
            transfer_out: pair.transfer_out,
            transfer_in: pair.transfer_in,
        }
    }

    fn transfer_match_tolerance() -> Decimal {
        Decimal::new(1, 6)
    }

    fn opposite_transfer_type(activity_type: &str) -> Option<&'static str> {
        match activity_type {
            ACTIVITY_TYPE_TRANSFER_IN => Some(ACTIVITY_TYPE_TRANSFER_OUT),
            ACTIVITY_TYPE_TRANSFER_OUT => Some(ACTIVITY_TYPE_TRANSFER_IN),
            _ => None,
        }
    }

    fn non_cash_transfer_asset_key(activity: &Activity) -> Option<String> {
        activity
            .asset_id
            .as_deref()
            .map(str::trim)
            .filter(|asset_id| !asset_id.is_empty())
            .filter(|asset_id| !is_cash_symbol(asset_id))
            .map(str::to_uppercase)
    }

    fn is_security_transfer_activity(activity: &Activity) -> bool {
        matches!(
            activity.effective_type(),
            ACTIVITY_TYPE_TRANSFER_IN | ACTIVITY_TYPE_TRANSFER_OUT
        ) && Self::non_cash_transfer_asset_key(activity).is_some()
    }

    fn transfer_abs_value(activity: &Activity) -> Option<Decimal> {
        activity.amount.map(|value| value.abs())
    }

    fn decimals_match(left: Option<Decimal>, right: Option<Decimal>) -> bool {
        match (left, right) {
            (Some(left), Some(right)) => {
                (left.abs() - right.abs()).abs() <= Self::transfer_match_tolerance()
            }
            _ => false,
        }
    }

    fn transfer_date_diff_days(left: &Activity, right: &Activity) -> i64 {
        (left.activity_date.date_naive() - right.activity_date.date_naive())
            .num_days()
            .abs()
    }

    fn transfer_candidate_score(day_diff: i64) -> (i32, String) {
        let score = (100 - (day_diff as i32 * 10)).max(1);
        let confidence = if day_diff == 0 {
            "high"
        } else if day_diff <= 3 {
            "medium"
        } else {
            "low"
        };
        (score, confidence.to_string())
    }

    /// Counterparts `source` could be linked with, best first: posted transfers
    /// of the opposite direction, not already in a pair, within `window_days`.
    fn transfer_match_candidates<'a>(
        source: &Activity,
        opposite_type: &str,
        activities: impl IntoIterator<Item = &'a Activity>,
        transfer_resolution: &TransferPairResolution,
        window_days: i64,
        limit: usize,
    ) -> Vec<TransferMatchCandidate> {
        let mut candidates: Vec<TransferMatchCandidate> = activities
            .into_iter()
            .filter(|candidate| {
                candidate.id != source.id
                    && candidate.is_posted()
                    && transfer_resolution
                        .pair_for_activity(&candidate.id)
                        .is_none()
                    && candidate.effective_type() == opposite_type
            })
            .filter_map(|candidate| {
                let day_diff = Self::transfer_date_diff_days(source, candidate);
                if day_diff > window_days {
                    return None;
                }
                Self::build_transfer_match_candidate(source, candidate, day_diff)
            })
            .collect();

        candidates.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| {
                    left.activity
                        .activity_date
                        .cmp(&right.activity.activity_date)
                })
                .then_with(|| left.activity.id.cmp(&right.activity.id))
        });
        candidates.truncate(limit);
        candidates
    }

    /// The scan behind `find_unlinked_transfers`. A pair can cross into an
    /// archived account, so pairs are resolved over every account. Transfers
    /// come from the active accounts the Health Center checks, and candidates
    /// from the non-archived accounts the Link Transfer dialog offers.
    fn scan_unlinked_transfers(
        activity_repository: &dyn ActivityRepositoryTrait,
        account_service: &dyn AccountServiceTrait,
        request: UnlinkedTransfersRequest,
    ) -> Result<UnlinkedTransfers> {
        let every_activity = activity_repository.get_activities_including_archived_accounts()?;
        let transfer_resolution = TransferPairResolution::from_activities(&every_activity);
        let account_ids = |accounts: Vec<Account>| -> HashSet<String> {
            accounts.into_iter().map(|account| account.id).collect()
        };
        let active_account_ids = account_ids(account_service.get_active_non_archived_accounts()?);
        let open_account_ids = account_ids(account_service.get_non_archived_accounts()?);
        let window_days = request.window_days.unwrap_or(7).clamp(0, 90);
        let candidate_limit = request.candidate_limit.unwrap_or(3).clamp(1, 25);
        let limit = request.limit.unwrap_or(25).clamp(1, 100);

        let in_scope = |activity: &Activity, state: TransferLinkState| {
            let date = activity.activity_date.date_naive();
            state.is_unlinked()
                && request
                    .link_states
                    .as_deref()
                    .is_none_or(|states| states.contains(&state))
                && request
                    .activity_id
                    .as_deref()
                    .is_none_or(|id| activity.id == id)
                && request
                    .account_id
                    .as_deref()
                    .is_none_or(|id| activity.account_id == id)
                && request.start_date.is_none_or(|start| date >= start)
                && request.end_date.is_none_or(|end| date <= end)
        };
        let mut sources: Vec<(&Activity, TransferLinkState, &'static str)> = every_activity
            .iter()
            .filter(|activity| {
                activity.is_posted() && active_account_ids.contains(&activity.account_id)
            })
            .filter_map(|activity| {
                let state = transfer_resolution.link_state(activity)?;
                let opposite_type = Self::opposite_transfer_type(activity.effective_type())?;
                in_scope(activity, state).then_some((activity, state, opposite_type))
            })
            .collect();
        sources.sort_by(|(left, _, _), (right, _, _)| {
            right
                .activity_date
                .cmp(&left.activity_date)
                .then_with(|| left.id.cmp(&right.id))
        });

        let total = sources.len();
        let transfers = sources
            .into_iter()
            .skip(request.offset.unwrap_or(0))
            .take(limit)
            .map(|(source, link_state, opposite_type)| UnlinkedTransfer {
                activity: source.clone(),
                link_state,
                candidates: Self::transfer_match_candidates(
                    source,
                    opposite_type,
                    every_activity
                        .iter()
                        .filter(|candidate| open_account_ids.contains(&candidate.account_id)),
                    &transfer_resolution,
                    window_days,
                    candidate_limit,
                ),
            })
            .collect();
        let linked = request.activity_id.as_deref().and_then(|id| {
            let pair = transfer_resolution.pair_for_activity(id)?;
            let (activity, counterpart) = if pair.transfer_in.id == id {
                (&pair.transfer_in, &pair.transfer_out)
            } else {
                (&pair.transfer_out, &pair.transfer_in)
            };
            Some(LinkedTransfer {
                counterpart_id: counterpart.id.clone(),
                link_state: transfer_resolution.link_state(activity)?,
            })
        });
        Ok(UnlinkedTransfers {
            transfers,
            total,
            linked,
        })
    }

    fn build_transfer_match_candidate(
        source: &Activity,
        candidate: &Activity,
        day_diff: i64,
    ) -> Option<TransferMatchCandidate> {
        let source_is_security = Self::is_security_transfer_activity(source);
        let candidate_is_security = Self::is_security_transfer_activity(candidate);
        let (score, confidence) = Self::transfer_candidate_score(day_diff);
        let mut reasons = Vec::new();
        let mut warnings = Vec::new();
        let is_same_account_cash_fx =
            crate::activities::is_same_account_cash_fx_conversion(source, candidate)
                || crate::activities::is_same_account_cash_fx_conversion(candidate, source);

        if source.account_id == candidate.account_id && !is_same_account_cash_fx {
            return None;
        }

        if source_is_security || candidate_is_security {
            let source_asset = Self::non_cash_transfer_asset_key(source)?;
            let candidate_asset = Self::non_cash_transfer_asset_key(candidate)?;
            if source_asset != candidate_asset {
                return None;
            }
            if !Self::decimals_match(source.quantity, candidate.quantity) {
                return None;
            }
            reasons.push("Same asset".to_string());
            reasons.push("Same quantity".to_string());
            if source.currency != candidate.currency {
                warnings.push(format!(
                    "Currencies differ ({} vs {}).",
                    source.currency, candidate.currency
                ));
            }
            if let (Some(source_price), Some(candidate_price)) =
                (source.unit_price, candidate.unit_price)
            {
                let max = source_price.abs().max(candidate_price.abs());
                if !max.is_zero()
                    && (source_price.abs() - candidate_price.abs()).abs() / max > Decimal::new(1, 2)
                {
                    warnings.push("Prices differ by more than 1%.".to_string());
                }
            }

            if day_diff == 0 {
                reasons.push("Same date".to_string());
            } else {
                warnings.push(format!("Dates differ by {} day(s).", day_diff));
            }

            return Some(TransferMatchCandidate {
                activity: candidate.clone(),
                match_kind: "security".to_string(),
                confidence,
                score,
                reasons,
                warnings,
            });
        }

        if is_same_account_cash_fx {
            reasons.push("Same account".to_string());
            reasons.push("Cash FX conversion".to_string());
            if day_diff == 0 {
                reasons.push("Same date".to_string());
            } else {
                warnings.push(format!("Dates differ by {} day(s).", day_diff));
            }

            return Some(TransferMatchCandidate {
                activity: candidate.clone(),
                match_kind: "cash_fx_conversion".to_string(),
                confidence,
                score,
                reasons,
                warnings,
            });
        }

        if !source.currency.eq_ignore_ascii_case(&candidate.currency) {
            return None;
        }
        if !Self::decimals_match(
            Self::transfer_abs_value(source),
            Self::transfer_abs_value(candidate),
        ) {
            return None;
        }
        reasons.push("Same amount".to_string());
        reasons.push("Same currency".to_string());
        if day_diff == 0 {
            reasons.push("Same date".to_string());
        } else {
            warnings.push(format!("Dates differ by {} day(s).", day_diff));
        }

        Some(TransferMatchCandidate {
            activity: candidate.clone(),
            match_kind: "cash".to_string(),
            confidence,
            score,
            reasons,
            warnings,
        })
    }

    fn load_internal_transfer_pair_for_activity(
        &self,
        activity_id: &str,
    ) -> Result<Option<TransferPair>> {
        let activity = self.activity_repository.get_activity(activity_id)?;
        let Some(group_id) = activity
            .source_group_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };

        let group_activities = self
            .activity_repository
            .get_activities_by_source_group_id(group_id)?;

        if group_activities.len() != 2 {
            return Ok(None);
        }

        let resolution =
            crate::activities::TransferPairResolution::from_activities(&group_activities);
        let Some(pair) = resolution.pair_for_activity(activity_id).cloned() else {
            return Ok(None);
        };

        if Self::is_valid_internal_transfer_pair(&pair) {
            Ok(Some(pair))
        } else {
            Ok(None)
        }
    }

    fn require_internal_transfer_pair_for_activity(
        &self,
        activity_id: &str,
    ) -> Result<TransferPair> {
        self.load_internal_transfer_pair_for_activity(activity_id)?
            .ok_or_else(|| {
                Self::invalid_activity_data("Activity is not a valid internal transfer pair")
            })
    }

    fn validate_internal_pair_request(
        &self,
        request: &InternalTransferPairRequest,
    ) -> Result<InternalPairValues> {
        if request.from_account_id.trim().is_empty() {
            return Err(Self::invalid_activity_data("Source account is required"));
        }
        if request.to_account_id.trim().is_empty() {
            return Err(Self::invalid_activity_data(
                "Destination account is required",
            ));
        }
        if request.from_account_id == request.to_account_id {
            return Err(Self::invalid_activity_data(
                "Source and destination accounts must be different",
            ));
        }

        if request
            .transfer_mode
            .as_deref()
            .is_some_and(|mode| mode != "cash")
        {
            return Err(Self::invalid_activity_data(
                "Pair save currently supports internal cash transfers only",
            ));
        }

        let source_amount = request
            .source_amount
            .filter(|amount| amount.is_sign_positive() && !amount.is_zero())
            .ok_or_else(|| Self::invalid_activity_data("Source amount must be greater than 0"))?;

        let source_currency = request.source_currency.trim();
        let destination_currency = request.destination_currency.trim();
        if source_currency.is_empty() {
            return Err(Self::invalid_activity_data("Source currency is required"));
        }
        if destination_currency.is_empty() {
            return Err(Self::invalid_activity_data(
                "Destination currency is required",
            ));
        }

        self.account_service.get_account(&request.from_account_id)?;
        self.account_service.get_account(&request.to_account_id)?;

        // Normalize units together with amounts, before uppercasing (GBp != GBP).
        let (source_amount, source_currency) = normalize_amount(source_amount, source_currency);
        let (requested_destination_amount, destination_currency) = normalize_amount(
            request.destination_amount.unwrap_or_default(),
            destination_currency,
        );
        let source_currency = source_currency.to_uppercase();
        let destination_currency = destination_currency.to_uppercase();

        let destination_amount = if source_currency == destination_currency {
            source_amount
        } else {
            Some(requested_destination_amount)
                .filter(|amount| amount.is_sign_positive() && !amount.is_zero())
                .ok_or_else(|| {
                    Self::invalid_activity_data("Destination amount must be greater than 0")
                })?
        };

        // Legacy callers may send an execution-rate hint. The two cash amounts
        // are authoritative; this is NOT an activity-to-account valuation rate.
        if source_currency != destination_currency
            && request
                .fx_rate
                .is_some_and(|rate| !rate.is_sign_positive() || rate.is_zero())
        {
            return Err(Self::invalid_activity_data("FX rate must be positive"));
        }

        Ok(InternalPairValues {
            source_amount,
            destination_amount,
            source_currency,
            destination_currency,
        })
    }

    fn build_internal_pair_create_request(
        request: &InternalTransferPairRequest,
        values: &InternalPairValues,
    ) -> Vec<NewActivity> {
        let group_id = request
            .source_group_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("wf-transfer-{}", Uuid::new_v4()));
        let metadata = Self::internal_transfer_metadata();

        vec![
            NewActivity {
                id: None,
                account_id: request.from_account_id.clone(),
                asset: None,
                activity_type: ACTIVITY_TYPE_TRANSFER_OUT.to_string(),
                subtype: None,
                activity_date: request.activity_date.clone(),
                quantity: None,
                unit_price: None,
                currency: values.source_currency.clone(),
                fee: None,
                tax: None,
                amount: Some(values.source_amount),
                status: None,
                notes: request.notes.clone(),
                fx_rate: None,
                metadata: metadata.clone(),
                needs_review: None,
                source_system: Some("MANUAL".to_string()),
                source_record_id: None,
                source_group_id: Some(group_id.clone()),
                idempotency_key: None,
            },
            NewActivity {
                id: None,
                account_id: request.to_account_id.clone(),
                asset: None,
                activity_type: ACTIVITY_TYPE_TRANSFER_IN.to_string(),
                subtype: None,
                activity_date: request.activity_date.clone(),
                quantity: None,
                unit_price: None,
                currency: values.destination_currency.clone(),
                fee: None,
                tax: None,
                amount: Some(values.destination_amount),
                status: None,
                notes: request.notes.clone(),
                fx_rate: None,
                metadata,
                needs_review: None,
                source_system: Some("MANUAL".to_string()),
                source_record_id: None,
                source_group_id: Some(group_id),
                idempotency_key: None,
            },
        ]
    }

    fn build_internal_pair_updates(
        &self,
        request: &InternalTransferPairRequest,
        pair: &TransferPair,
        values: &InternalPairValues,
    ) -> Result<Vec<ActivityUpdate>> {
        let metadata = Self::internal_transfer_metadata();
        // Preserve user valuation overrides only while their currency pair is unchanged.
        let valuation_rate_patch = |existing: &Activity, account_id: &str, currency: &str| {
            let old_account = self.account_service.get_account(&existing.account_id)?;
            let new_account = self.account_service.get_account(account_id)?;
            Ok::<_, Error>(
                (existing.currency != currency || old_account.currency != new_account.currency)
                    .then_some(None),
            )
        };
        Ok(vec![
            ActivityUpdate {
                id: pair.transfer_out.id.clone(),
                account_id: request.from_account_id.clone(),
                asset: None,
                activity_type: ACTIVITY_TYPE_TRANSFER_OUT.to_string(),
                subtype: None,
                activity_date: request.activity_date.clone(),
                quantity: Some(None),
                unit_price: Some(None),
                currency: values.source_currency.clone(),
                fee: None,
                tax: None,
                amount: Some(Some(values.source_amount)),
                status: None,
                needs_review: None,
                notes: request.notes.clone(),
                fx_rate: valuation_rate_patch(
                    &pair.transfer_out,
                    &request.from_account_id,
                    &values.source_currency,
                )?,
                metadata: metadata.clone(),
            },
            ActivityUpdate {
                id: pair.transfer_in.id.clone(),
                account_id: request.to_account_id.clone(),
                asset: None,
                activity_type: ACTIVITY_TYPE_TRANSFER_IN.to_string(),
                subtype: None,
                activity_date: request.activity_date.clone(),
                quantity: Some(None),
                unit_price: Some(None),
                currency: values.destination_currency.clone(),
                fee: None,
                tax: None,
                amount: Some(Some(values.destination_amount)),
                status: None,
                needs_review: None,
                notes: request.notes.clone(),
                fx_rate: valuation_rate_patch(
                    &pair.transfer_in,
                    &request.to_account_id,
                    &values.destination_currency,
                )?,
                metadata,
            },
        ])
    }

    fn validate_internal_cash_pair_currency_update(
        update: &ActivityUpdate,
        existing: &Activity,
        pair: &TransferPair,
    ) -> Result<()> {
        if Self::is_cash_transfer_pair(pair)
            && pair.transfer_out.account_id != pair.transfer_in.account_id
            && (update.account_id != existing.account_id
                || (!update.currency.is_empty() && update.currency != existing.currency))
        {
            return Err(Self::invalid_activity_data(
                "Use the transfer pair editor to change transfer accounts or currencies",
            ));
        }
        Ok(())
    }

    fn build_counterpart_update(
        &self,
        update: &ActivityUpdate,
        existing: &Activity,
        pair: &TransferPair,
    ) -> Result<Option<ActivityUpdate>> {
        let counterpart = if existing.id == pair.transfer_in.id {
            &pair.transfer_out
        } else if existing.id == pair.transfer_out.id {
            &pair.transfer_in
        } else {
            return Ok(None);
        };

        // Lifecycle fields are pair-level actions, but only deliberate
        // transitions travel: the grid echoes the primary's current status on
        // every save, and an ordinary edit must not disturb the counterpart's
        // own lifecycle or review state.
        let status_transition = update
            .status
            .clone()
            .filter(|status| *status != existing.status);
        let review_transition = update
            .needs_review
            .filter(|review| *review != existing.needs_review);
        // Approving a pair must also post a Draft counterpart, or its
        // lifecycle guard would reject clearing the review flag alone.
        let counterpart_status = status_transition.or_else(|| {
            (review_transition == Some(false) && counterpart.status == ActivityStatus::Draft)
                .then_some(ActivityStatus::Posted)
        });

        let mut counterpart_update = ActivityUpdate {
            id: counterpart.id.clone(),
            account_id: counterpart.account_id.clone(),
            asset: Self::activity_asset_input(counterpart),
            activity_type: counterpart.activity_type.clone(),
            subtype: None,
            activity_date: update.activity_date.clone(),
            quantity: None,
            unit_price: None,
            currency: counterpart.currency.clone(),
            fee: None,
            tax: None,
            amount: None,
            status: counterpart_status,
            needs_review: review_transition,
            notes: update.notes.clone(),
            fx_rate: None,
            metadata: None,
        };

        let Some(Some(amount)) = update.amount else {
            return Ok(Some(counterpart_update));
        };

        if !Self::is_cash_transfer_pair(pair) {
            return Ok(Some(counterpart_update));
        }

        if pair.transfer_out.account_id != pair.transfer_in.account_id {
            // Grid saves can echo the original amount. A notes/rate-only edit
            // must not recalculate the other leg using a rounded ratio.
            if existing.amount.map(|value| value.abs()) == Some(amount.abs()) {
                return Ok(Some(counterpart_update));
            }
            let counterpart_amount = if existing.currency == counterpart.currency {
                Some(amount.abs())
            } else {
                existing
                    .amount
                    .zip(counterpart.amount)
                    .filter(|(source, destination)| !source.is_zero() && !destination.is_zero())
                    .and_then(|(source, destination)| {
                        amount
                            .abs()
                            .checked_mul(destination.abs())?
                            .checked_div(source.abs())
                    })
            }
            .ok_or_else(|| {
                Self::invalid_activity_data(
                    "Cross-currency transfer amount updates require valid existing amounts",
                )
            })?;
            counterpart_update.amount = Some(Some(counterpart_amount));
            return Ok(Some(counterpart_update));
        }

        if existing
            .currency
            .eq_ignore_ascii_case(&counterpart.currency)
        {
            counterpart_update.amount = Some(Some(amount.abs()));
            return Ok(Some(counterpart_update));
        }

        let rate = pair
            .transfer_in
            .fx_rate
            .or_else(|| update.fx_rate.flatten())
            .filter(|rate| rate.is_sign_positive() && !rate.is_zero())
            .ok_or_else(|| {
                Self::invalid_activity_data(
                    "Cross-currency transfer amount updates require a valid FX rate",
                )
            })?;

        let counterpart_amount = if existing.id == pair.transfer_out.id {
            amount.abs() * rate
        } else {
            amount.abs() / rate
        };
        counterpart_update.amount = Some(Some(counterpart_amount));

        Ok(Some(counterpart_update))
    }

    fn resolve_activity_currency(
        &self,
        activity_currency: &str,
        asset_currency: Option<&str>,
        account_currency: &str,
    ) -> String {
        resolve_currency(&[
            activity_currency,
            asset_currency.unwrap_or(""),
            account_currency,
            self.account_service
                .get_base_currency()
                .as_deref()
                .unwrap_or(""),
        ])
    }

    /// Creates a quote from activity data to serve as a price fallback.
    /// Uses `DataSource::Manual` for MANUAL-mode assets so provider sync cannot overwrite it.
    ///
    /// Only called for activity types where `unit_price` represents the asset's
    /// market price (BUY, SELL). Income activities (DIVIDEND,
    /// INTEREST) store payment amounts in `unit_price`, not asset prices.
    async fn create_quote_from_activity(
        &self,
        asset_id: &str,
        unit_price: Decimal,
        currency: &str,
        activity_date: &str,
        data_source: String,
    ) -> Result<()> {
        // Parse activity date
        let timestamp = if let Ok(dt) = DateTime::parse_from_rfc3339(activity_date) {
            dt.with_timezone(&Utc)
        } else if let Ok(date) = NaiveDate::parse_from_str(activity_date, "%Y-%m-%d") {
            Utc.from_utc_datetime(
                &date
                    .and_hms_opt(12, 0, 0)
                    .expect("12:00:00 is a valid time"),
            )
        } else {
            debug!(
                "Could not parse activity date '{}' for quote creation",
                activity_date
            );
            return Ok(());
        };

        let quote_id = if data_source == DATA_SOURCE_MANUAL {
            let date_part = timestamp.format("%Y%m%d").to_string();
            format!("{}_{}", date_part, asset_id.to_uppercase())
        } else {
            let date_str = timestamp.format("%Y-%m-%d").to_string();
            format!("{}_{}_{}", asset_id, date_str, data_source)
        };

        let quote = Quote {
            id: quote_id,
            asset_id: asset_id.to_string(),
            timestamp,
            open: unit_price,
            high: unit_price,
            low: unit_price,
            close: unit_price,
            adjclose: unit_price,
            volume: Decimal::ZERO,
            currency: currency.to_string(),
            data_source,
            created_at: Utc::now(),
            notes: None,
        };

        match self.quote_service.update_quote(quote).await {
            Ok(_) => {
                debug!(
                    "Created quote for asset {} on {} at price {}",
                    asset_id, activity_date, unit_price
                );
            }
            Err(e) => {
                // Log but don't fail the activity creation
                debug!("Failed to create quote for asset {}: {}", asset_id, e);
            }
        }

        Ok(())
    }
}

impl ActivityService {
    /// JSON metadata key carrying a non-standard contract multiplier on a new
    /// activity (e.g. mini options = 10).
    const METADATA_CONTRACT_MULTIPLIER: &'static str = "contract_multiplier";

    /// The creation-time multiplier seed an activity may carry. Read ONLY when
    /// this row creates its asset; the asset owns the multiplier from then on,
    /// so nothing downstream consults this.
    fn custom_option_multiplier(activity_metadata: Option<&str>) -> Option<Decimal> {
        activity_metadata
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get(Self::METADATA_CONTRACT_MULTIPLIER)?.as_f64())
            .and_then(Decimal::from_f64_retain)
            .filter(|d| d.is_sign_positive() && !d.is_zero())
    }

    /// Merges a metadata patch over the stored blob, top-level keys, patch
    /// keys winning. Non-object payloads (either side) fall back to plain
    /// replacement - there is nothing meaningful to merge into.
    fn merge_metadata_patch(existing: Option<&serde_json::Value>, patch: &str) -> String {
        let Ok(serde_json::Value::Object(mut patch_map)) =
            serde_json::from_str::<serde_json::Value>(patch)
        else {
            return patch.to_string();
        };
        // Only the loan payment actions write a withdrawal's loan tag; an edit
        // carrying a stale or new copy of it never changes the stored one.
        patch_map.remove(LOAN_PAYMENT_TAG_KEY);
        let mut merged = match existing {
            Some(serde_json::Value::Object(existing_map)) => existing_map.clone(),
            _ => serde_json::Map::new(),
        };
        merged.extend(patch_map);
        serde_json::Value::Object(merged).to_string()
    }

    /// New activities never start as loan payments; linking tags them afterwards.
    fn without_loan_payment_tag(metadata: Option<String>) -> Option<String> {
        let text = metadata?;
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(mut map)) if map.contains_key(LOAN_PAYMENT_TAG_KEY) => {
                map.remove(LOAN_PAYMENT_TAG_KEY);
                Some(serde_json::Value::Object(map).to_string())
            }
            _ => Some(text),
        }
    }

    /// Infers the asset kind and instrument type from symbol, exchange, and input values.
    /// Returns (AssetKind, Option<InstrumentType>).
    fn infer_asset_kind(
        &self,
        symbol: &str,
        exchange_mic: Option<&str>,
        asset_kind_input: Option<&str>,
    ) -> (AssetKind, Option<InstrumentType>) {
        // 1. If explicit input is provided, use it
        if let Some(asset_kind_value) = asset_kind_input {
            match asset_kind_value.to_uppercase().as_str() {
                "SECURITY" | "INVESTMENT" | "EQUITY" => {
                    return (AssetKind::Investment, Some(InstrumentType::Equity))
                }
                "CRYPTO" => return (AssetKind::Investment, Some(InstrumentType::Crypto)),
                "FX_RATE" | "FX" => return (AssetKind::Fx, Some(InstrumentType::Fx)),
                "OPTION" | "OPT" => return (AssetKind::Investment, Some(InstrumentType::Option)),
                "BOND" => return (AssetKind::Investment, Some(InstrumentType::Bond)),
                "COMMODITY" | "CMDTY" | "METAL" => {
                    return (AssetKind::Investment, Some(InstrumentType::Metal))
                }
                "PROPERTY" | "PROP" => return (AssetKind::Property, None),
                "VEHICLE" | "VEH" => return (AssetKind::Vehicle, None),
                "COLLECTIBLE" | "COLL" => return (AssetKind::Collectible, None),
                "PRECIOUS_METAL" | "PREC" => return (AssetKind::PreciousMetal, None),
                "PRIVATE_EQUITY" | "PEQ" => return (AssetKind::PrivateEquity, None),
                "LIABILITY" | "LIAB" => return (AssetKind::Liability, None),
                "OTHER" | "ALT" => return (AssetKind::Other, None),
                _ => {} // Fall through to inference
            }
        }

        // 2. Crypto pair pattern (e.g., BTC-USD, ETH-CAD) — checked before
        //    exchange_mic because brokers may attach their MIC to crypto pairs
        let upper_symbol = symbol.to_uppercase();
        if let Some((_base, quote)) = upper_symbol.rsplit_once('-') {
            let quote = quote.trim();
            let crypto_quotes = [
                "USD", "CAD", "EUR", "GBP", "JPY", "CHF", "AUD", "NZD", "HKD", "SGD", "CNY", "SEK",
                "NOK", "DKK", "PLN", "CZK", "HUF", "TRY", "MXN", "BRL", "KRW", "INR", "ZAR", "BTC",
                "ETH", "USDT", "USDC", "DAI", "BUSD", "USDP", "TUSD", "FDUSD",
            ];
            if crypto_quotes.contains(&quote) {
                return (AssetKind::Investment, Some(InstrumentType::Crypto));
            }
        }

        // 3. OCC option symbol heuristic (e.g. AAPL240119C00150000)
        // Must be checked before exchange MIC — search providers may attach an
        // exchange MIC (e.g. "OPRA") to option symbols, which would otherwise
        // cause them to be misclassified as equities.
        if crate::utils::occ_symbol::looks_like_occ_symbol(&upper_symbol) {
            return (AssetKind::Investment, Some(InstrumentType::Option));
        }

        // 4. If exchange MIC is provided, it's an equity
        if exchange_mic.is_some() {
            return (AssetKind::Investment, Some(InstrumentType::Equity));
        }

        // 5. Common crypto symbols heuristic (no MIC, bare symbol like BTC, ETH)
        let common_crypto = [
            "BTC", "ETH", "XRP", "LTC", "BCH", "ADA", "DOT", "LINK", "XLM", "DOGE", "UNI", "SOL",
            "AVAX", "MATIC", "ATOM", "ALGO", "VET", "FIL", "TRX", "ETC", "XMR", "AAVE", "MKR",
            "COMP", "SNX", "YFI", "SUSHI", "CRV",
        ];
        if common_crypto.contains(&upper_symbol.as_str()) {
            return (AssetKind::Investment, Some(InstrumentType::Crypto));
        }

        // 6. Default to equity (most common case)
        (AssetKind::Investment, Some(InstrumentType::Equity))
    }

    fn is_asset_not_found_error(err: &Error) -> bool {
        matches!(err, Error::Database(DatabaseError::NotFound(_)))
    }

    fn has_submitted_asset_identity(
        submitted_symbol: Option<&str>,
        submitted_exchange_mic: Option<&str>,
        submitted_instrument_type: Option<&InstrumentType>,
        submitted_quote_ccy: Option<&str>,
    ) -> bool {
        submitted_symbol
            .map(str::trim)
            .filter(|symbol| !symbol.is_empty())
            .is_some()
            || submitted_exchange_mic
                .map(str::trim)
                .filter(|mic| !mic.is_empty())
                .is_some()
            || submitted_instrument_type.is_some()
            || submitted_quote_ccy
                .map(str::trim)
                .filter(|ccy| !ccy.is_empty())
                .is_some()
    }

    fn asset_matches_submitted_identity(
        existing_asset: &crate::assets::Asset,
        submitted_symbol: Option<&str>,
        submitted_exchange_mic: Option<&str>,
        submitted_instrument_type: Option<&InstrumentType>,
        submitted_quote_ccy: Option<&str>,
    ) -> bool {
        let submitted_identity = canonicalize_market_identity(
            submitted_instrument_type.cloned(),
            submitted_symbol,
            submitted_exchange_mic,
            submitted_quote_ccy,
        );
        let existing_identity = canonicalize_market_identity(
            existing_asset.instrument_type.clone(),
            existing_asset
                .instrument_symbol
                .as_deref()
                .or(existing_asset.display_code.as_deref()),
            existing_asset.instrument_exchange_mic.as_deref(),
            Some(existing_asset.quote_ccy.as_str()),
        );

        if let Some(submitted_type) = submitted_instrument_type {
            if existing_asset.instrument_type.as_ref() != Some(submitted_type) {
                return false;
            }
        }

        if let Some(submitted_symbol) = submitted_identity.instrument_symbol.as_deref() {
            if Some(submitted_symbol) != existing_identity.instrument_symbol.as_deref() {
                return false;
            }
        }

        match submitted_instrument_type.or(existing_asset.instrument_type.as_ref()) {
            Some(InstrumentType::Crypto | InstrumentType::Fx) => {
                if let Some(submitted_quote_ccy) = submitted_identity.quote_ccy.as_deref() {
                    return Some(submitted_quote_ccy) == existing_identity.quote_ccy.as_deref();
                }
                true
            }
            Some(InstrumentType::Option) => true,
            _ => {
                if let Some(submitted_mic) = submitted_identity.instrument_exchange_mic.as_deref() {
                    return Some(submitted_mic)
                        == existing_identity.instrument_exchange_mic.as_deref();
                }
                true
            }
        }
    }

    fn resolved_submitted_asset_id(
        &self,
        submitted_asset_id: Option<&str>,
        submitted_symbol: Option<&str>,
        submitted_exchange_mic: Option<&str>,
        submitted_instrument_type: Option<&InstrumentType>,
        submitted_quote_ccy: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(asset_id) = submitted_asset_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return Ok(None);
        };

        let has_identity = Self::has_submitted_asset_identity(
            submitted_symbol,
            submitted_exchange_mic,
            submitted_instrument_type,
            submitted_quote_ccy,
        );

        match self.asset_service.get_asset_by_id(asset_id) {
            Ok(existing_asset) => {
                if has_identity
                    && !Self::asset_matches_submitted_identity(
                        &existing_asset,
                        submitted_symbol,
                        submitted_exchange_mic,
                        submitted_instrument_type,
                        submitted_quote_ccy,
                    )
                {
                    Ok(None)
                } else {
                    Ok(Some(asset_id.to_string()))
                }
            }
            Err(err) if has_identity && Self::is_asset_not_found_error(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Finds an existing asset by instrument fields, searching all assets.
    fn find_existing_asset_id(
        &self,
        symbol: &str,
        exchange_mic: Option<&str>,
        instrument_type: Option<&InstrumentType>,
        quote_ccy: Option<&str>,
    ) -> Option<String> {
        let assets = self.asset_service.get_assets().unwrap_or_default();
        let upper_symbol = symbol.to_uppercase();
        let expected_key = instrument_type.and_then(|itype| match itype {
            InstrumentType::Crypto | InstrumentType::Fx => quote_ccy.and_then(|ccy| {
                let normalized_ccy = ccy.trim().to_uppercase();
                if normalized_ccy.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{}:{}/{}",
                        itype.as_db_str(),
                        upper_symbol,
                        normalized_ccy
                    ))
                }
            }),
            _ => exchange_mic
                .filter(|mic| !mic.trim().is_empty())
                .map(|mic| {
                    format!(
                        "{}:{}@{}",
                        itype.as_db_str(),
                        upper_symbol,
                        mic.trim().to_uppercase()
                    )
                })
                .or_else(|| Some(format!("{}:{}", itype.as_db_str(), upper_symbol))),
        });

        // Fallback key for OCC option symbols that were previously misclassified
        // as EQUITY due to exchange MIC taking priority over OCC heuristic.
        // Must mirror the key format the old code would have produced (with MIC when present).
        let fallback_equity_key = if matches!(instrument_type, Some(InstrumentType::Option)) {
            exchange_mic
                .filter(|mic| !mic.trim().is_empty())
                .map(|mic| {
                    format!(
                        "{}:{}@{}",
                        InstrumentType::Equity.as_db_str(),
                        upper_symbol,
                        mic.trim().to_uppercase()
                    )
                })
                .or_else(|| {
                    Some(format!(
                        "{}:{}",
                        InstrumentType::Equity.as_db_str(),
                        upper_symbol,
                    ))
                })
        } else {
            None
        };

        if let Some(ref key) = expected_key {
            // Pass 1: exact instrument key match
            for asset in &assets {
                if asset.instrument_key.as_deref() == Some(key) {
                    return Some(asset.id.clone());
                }
            }
            // Pass 2: fallback for legacy misclassified options
            if let Some(ref fallback) = fallback_equity_key {
                for asset in &assets {
                    if asset.instrument_key.as_deref() == Some(fallback.as_str()) {
                        return Some(asset.id.clone());
                    }
                }
            }
        }

        if instrument_type == Some(&InstrumentType::Bond) {
            let specs = [AssetSpec::market_instrument(
                symbol.to_string(),
                symbol.to_string(),
                exchange_mic.map(str::to_string),
                InstrumentType::Bond,
                quote_ccy.unwrap_or("USD").to_string(),
            )];
            if let Some(id) = resolve_bond_aliases(&specs, &assets)[0].asset_id.clone() {
                return Some(id);
            }
        }

        for asset in &assets {
            if let (Some(ref a_symbol), Some(ref a_type)) =
                (&asset.instrument_symbol, &asset.instrument_type)
            {
                let type_matches = instrument_type.is_none_or(|t| t == a_type);
                let symbol_matches = a_symbol.to_uppercase() == upper_symbol;
                let mic_matches = if matches!(a_type, InstrumentType::Option) {
                    // OCC option ticker is globally unique; tolerate legacy MIC mismatch to avoid duplicates.
                    match (exchange_mic, &asset.instrument_exchange_mic) {
                        (Some(mic), Some(a_mic)) => mic.eq_ignore_ascii_case(a_mic),
                        _ => true,
                    }
                } else {
                    match (exchange_mic, &asset.instrument_exchange_mic) {
                        (Some(mic), Some(a_mic)) => mic.eq_ignore_ascii_case(a_mic),
                        (None, None) => true,
                        _ => false,
                    }
                };
                let ccy_matches = if matches!(a_type, InstrumentType::Crypto | InstrumentType::Fx) {
                    quote_ccy.is_none_or(|ccy| asset.quote_ccy.eq_ignore_ascii_case(ccy))
                } else {
                    true
                };
                if type_matches && symbol_matches && mic_matches && ccy_matches {
                    return Some(asset.id.clone());
                }
            }
        }
        None
    }

    async fn prepare_new_activity(&self, mut activity: NewActivity) -> Result<NewActivity> {
        activity.activity_date =
            self.validate_and_normalize_activity_date(&activity.activity_date)?;
        activity.metadata = Self::without_loan_payment_tag(activity.metadata.take());
        activity.subtype = NewActivity::canonicalize_subtype_for_activity(
            &activity.activity_type,
            activity.subtype.as_deref(),
        );
        Self::normalize_new_activity_economic_signs(&mut activity);
        let account: Account = self.account_service.get_account(&activity.account_id)?;
        Self::validate_activity_allowed_for_account(
            &activity.activity_type,
            activity.get_symbol_id().or(activity.get_symbol_code()),
            &account,
        )?;
        Self::drop_cash_symbol_asset(&mut activity);
        let base_ccy = self.account_service.get_base_currency().unwrap_or_default();
        let account_currency = resolve_currency(&[&account.currency, &base_ccy]);

        let currency = resolve_currency(&[&activity.currency, &account_currency, &base_ccy]);
        Self::validate_new_activity_income_values(&activity)?;

        if activity.activity_type == ACTIVITY_TYPE_SPLIT {
            activity.amount = activity.amount.map(|v| v.abs());
            Self::validate_split_ratio(&activity.activity_type, activity.amount)?;
        }

        // Extract asset fields from nested `asset` object
        let symbol = activity.get_symbol_code().map(|s| s.to_string());
        let exchange_mic = activity.get_exchange_mic().map(|s| s.to_string());
        let asset_kind_input = activity.get_kind().map(|s| s.to_string());
        let quote_ccy_input = Self::normalize_quote_ccy(activity.get_quote_ccy());
        let instrument_type_input = Self::parse_instrument_type(activity.get_instrument_type());
        let asset_name = activity.get_name().map(|s| s.to_string());
        let provider_id_input = activity.asset.as_ref().and_then(|a| a.provider_id.clone());
        let provider_symbol_input = activity
            .asset
            .as_ref()
            .and_then(|a| a.provider_symbol.clone());
        let quote_mode = activity.get_quote_mode().map(|s| s.to_string());
        let parsed_quote_mode =
            quote_mode
                .as_deref()
                .and_then(|mode| match mode.to_uppercase().as_str() {
                    "MANUAL" => Some(QuoteMode::Manual),
                    "MARKET" => Some(QuoteMode::Market),
                    _ => None,
                });

        let inferred = symbol.as_deref().map(|s| {
            self.infer_asset_kind(s, exchange_mic.as_deref(), asset_kind_input.as_deref())
        });
        let inferred_instrument_type = inferred.as_ref().and_then(|(_, it)| it.clone());
        let effective_instrument_type = instrument_type_input
            .clone()
            .or(inferred_instrument_type.clone());
        let effective_kind = instrument_type_input
            .as_ref()
            .map(Self::kind_from_instrument_type)
            .or_else(|| inferred.as_ref().map(|(kind, _)| kind.clone()));

        // Normalize symbol + MIC using payload/suffix only (no live lookup for user save paths).
        let is_crypto = effective_instrument_type.as_ref() == Some(&InstrumentType::Crypto);
        let is_option = effective_instrument_type.as_ref() == Some(&InstrumentType::Option);
        let is_non_security_instrument = matches!(
            effective_instrument_type.as_ref(),
            Some(InstrumentType::Crypto | InstrumentType::Fx)
        );
        // A supplied MIC decides whether a trailing `.X` is a venue or part of the
        // ticker: `ZAAA.F` on Cboe Canada is a hedged unit class, not a Frankfurt
        // listing. See `parse_symbol_with_known_exchange`.
        let (base_symbol, suffix_mic) = symbol
            .as_deref()
            .map(|sym| parse_symbol_with_known_exchange(sym, exchange_mic.as_deref()))
            .unwrap_or(("", None));
        let exchange_mic = if is_non_security_instrument {
            None
        } else {
            exchange_mic.or_else(|| suffix_mic.map(|mic| mic.to_string()))
        };
        let normalized_symbol_for_lookup = if base_symbol.is_empty() {
            None
        } else if is_crypto {
            Some(
                parse_crypto_pair_symbol(base_symbol)
                    .map(|(base, _)| base)
                    .unwrap_or_else(|| base_symbol.to_string()),
            )
        } else if is_option {
            // Normalize broker-specific option symbols (e.g. Fidelity's "-MU270115C600")
            // to standard OCC format before storing.
            Some(
                crate::utils::occ_symbol::normalize_option_symbol(base_symbol)
                    .unwrap_or_else(|| base_symbol.to_string()),
            )
        } else {
            Some(base_symbol.to_string())
        };
        let submitted_asset_id = self.resolved_submitted_asset_id(
            activity.get_symbol_id(),
            normalized_symbol_for_lookup.as_deref(),
            exchange_mic.as_deref(),
            effective_instrument_type.as_ref(),
            quote_ccy_input.as_deref(),
        )?;
        if let Some(asset_input) = activity.asset.as_mut() {
            asset_input.id = submitted_asset_id.clone();
        }

        match symbol.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw_symbol) => {
                if is_garbage_symbol(raw_symbol) {
                    return Err(ActivityError::InvalidData(format!(
                        "Invalid symbol '{}'. Please search for a valid ticker.",
                        raw_symbol
                    ))
                    .into());
                }

                self.asset_service.validate_persisted_symbol_metadata(
                    normalized_symbol_for_lookup
                        .as_deref()
                        .unwrap_or(raw_symbol),
                    submitted_asset_id.as_deref(),
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    parsed_quote_mode,
                    quote_ccy_input.as_deref(),
                )?;
            }
            None if activity
                .get_symbol_id()
                .filter(|id| !id.trim().is_empty())
                .is_none()
                && Self::requires_asset_identity(
                    &activity.activity_type,
                    activity.subtype.as_deref(),
                ) =>
            {
                return Err(ActivityError::InvalidData(
                    "Asset-backed activities need either asset_id or symbol".to_string(),
                )
                .into());
            }
            None => {}
        }

        let quote_lookup_symbol = normalized_symbol_for_lookup.clone().unwrap_or_default();

        // Use pair quote for crypto/FX; otherwise resolve from payload and existing data:
        // explicit input -> existing asset -> MIC fallback -> activity/account.
        let mut quote_ccy_for_asset = quote_ccy_input.clone();
        let asset_currency = if is_crypto {
            symbol
                .as_deref()
                .and_then(parse_crypto_pair_symbol)
                .map(|(_, quote)| quote)
                .or_else(|| quote_ccy_input.clone())
                .unwrap_or_else(|| currency.clone())
        } else if is_non_security_instrument {
            quote_ccy_input.clone().unwrap_or(currency.clone())
        } else {
            let existing_asset_quote_ccy = self
                .existing_asset_quote_ccy_by_id(submitted_asset_id.as_deref())
                .or_else(|| {
                    normalized_symbol_for_lookup
                        .as_deref()
                        .and_then(|resolved_symbol| {
                            self.asset_service.existing_quote_ccy_by_symbol(
                                resolved_symbol,
                                exchange_mic.as_deref(),
                                effective_instrument_type.as_ref(),
                            )
                        })
                });
            let (resolved_quote_ccy, resolution_source) = self
                .resolve_quote_ccy(
                    quote_lookup_symbol.as_str(),
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    quote_ccy_input.as_deref(),
                    existing_asset_quote_ccy.as_deref(),
                    currency.as_str(),
                    false,
                )
                .await;
            if matches!(
                resolution_source,
                QuoteCcyResolutionSource::ExplicitInput | QuoteCcyResolutionSource::ProviderQuote
            ) {
                quote_ccy_for_asset = Some(resolved_quote_ccy.clone());
            }
            resolved_quote_ccy
        };

        // Resolve asset_id:
        // 1. If symbol is provided, search existing assets or prepare for creation
        // 2. If only asset.id is provided (UUID), use it directly
        // 3. Cash activities: no asset
        let resolved_asset_id = if let Some(ref normalized_symbol) = normalized_symbol_for_lookup {
            // Look up existing asset by instrument fields
            let existing_id = self
                .find_existing_asset_id(
                    normalized_symbol,
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    Some(&asset_currency),
                )
                .or_else(|| submitted_asset_id.clone());

            if let Some(id) = existing_id {
                Some(id)
            } else {
                // Create new asset with generated UUID
                let new_id = Uuid::new_v4().to_string();

                // Build structured metadata for option/bond/metal assets
                let structured_metadata = if let Some(mult) =
                    Self::custom_option_multiplier(activity.metadata.as_deref())
                {
                    crate::assets::build_option_metadata(normalized_symbol, mult)
                } else {
                    crate::assets::build_asset_metadata(
                        effective_instrument_type.as_ref(),
                        normalized_symbol,
                    )
                };

                let metadata = crate::assets::AssetMetadata {
                    name: asset_name.clone(),
                    kind: effective_kind.clone(),
                    instrument_exchange_mic: exchange_mic.clone(),
                    instrument_symbol: Some(normalized_symbol.clone()),
                    instrument_type: effective_instrument_type.clone(),
                    display_code: Some(normalized_symbol.clone()),
                    requested_quote_ccy: quote_ccy_for_asset.clone(),
                    provider_config: None,
                    provider_id: provider_id_input.clone(),
                    provider_symbol: provider_symbol_input.clone(),
                    asset_metadata: structured_metadata,
                };
                self.asset_service
                    .get_or_create_minimal_asset(
                        &new_id,
                        Some(asset_currency.clone()),
                        Some(metadata),
                        quote_mode.clone(),
                    )
                    .await?;
                Some(new_id)
            }
        } else if let Some(asset_id) = submitted_asset_id.as_deref() {
            // Existing asset_id provided (UUID from frontend)
            Some(asset_id.to_string())
        } else if !Self::requires_asset_identity(
            &activity.activity_type,
            activity.subtype.as_deref(),
        ) {
            None // Symbol-optional types have no asset when symbol is absent
        } else {
            return Err(ActivityError::InvalidData(
                "Asset-backed activities need either asset_id or symbol".to_string(),
            )
            .into());
        };

        // Update activity's asset with resolved asset_id
        if let Some(ref resolved_id) = resolved_asset_id {
            match activity.asset.as_mut() {
                Some(asset) => asset.id = Some(resolved_id.clone()),
                None => {
                    activity.asset = Some(AssetResolutionInput {
                        id: Some(resolved_id.clone()),
                        ..Default::default()
                    });
                }
            }
        }

        // Facts the final-amount boundary needs from the resolved asset,
        // captured while it is in scope so it is fetched exactly once.
        let mut resolved_asset_multiplier: Option<Decimal> = None;

        // Process asset if asset_id is resolved
        if let Some(ref asset_id) = resolved_asset_id {
            let canonical_symbol = normalized_symbol_for_lookup.clone();
            let metadata = crate::assets::AssetMetadata {
                name: asset_name.clone(),
                kind: effective_kind,
                instrument_exchange_mic: exchange_mic.clone(),
                instrument_symbol: canonical_symbol.clone(),
                instrument_type: effective_instrument_type.clone(),
                display_code: canonical_symbol,
                requested_quote_ccy: quote_ccy_for_asset.clone(),
                provider_config: None,
                provider_id: provider_id_input,
                provider_symbol: provider_symbol_input,
                asset_metadata: None,
            };
            let mut asset = if normalized_symbol_for_lookup.is_none() {
                self.asset_service.get_asset_by_id(asset_id)?
            } else {
                self.asset_service
                    .get_or_create_minimal_asset(
                        asset_id,
                        Some(asset_currency.clone()),
                        Some(metadata),
                        quote_mode.clone(),
                    )
                    .await?
            };

            // Update asset quote mode if specified (for existing assets that need mode change)
            if let Some(ref mode) = quote_mode {
                let requested_mode = mode.to_uppercase();
                let current_mode = asset.quote_mode.as_db_str();
                if requested_mode != current_mode {
                    asset = self
                        .asset_service
                        .update_quote_mode_silent(&asset.id, &requested_mode)
                        .await?;
                }
            }

            resolved_asset_multiplier = Some(asset.contract_multiplier());

            // Create a quote from the activity price as a fallback, but only
            // for MANUAL-mode assets. For MARKET-mode assets the unit price is
            // a cost input, not a market price, and writing it here would
            // shadow provider quotes.
            let is_manual_mode = asset.quote_mode == QuoteMode::Manual
                || matches!(parsed_quote_mode, Some(QuoteMode::Manual));
            if is_manual_mode
                && PRICE_BEARING_ACTIVITY_TYPES.contains(&activity.activity_type.as_str())
            {
                if let Some(unit_price) = activity.unit_price {
                    let source = DATA_SOURCE_MANUAL.to_string();
                    self.create_quote_from_activity(
                        asset_id,
                        unit_price,
                        &currency,
                        &activity.activity_date,
                        source,
                    )
                    .await?;
                }
            }

            if activity.currency.is_empty() {
                activity.currency = asset.quote_ccy.clone();
            }

            // Register FX pair for activity currency if different from account currency
            if activity.currency != account_currency {
                self.fx_service
                    .register_currency_pair(activity.currency.as_str(), account_currency.as_str())
                    .await?;
            }

            // Register FX pair for asset currency if different from account currency
            if asset.quote_ccy != account_currency && asset.quote_ccy != activity.currency {
                self.fx_service
                    .register_currency_pair(asset.quote_ccy.as_str(), account_currency.as_str())
                    .await?;
            }
        } else {
            // For pure cash movements without asset, just register FX if needed
            if activity.currency.is_empty() {
                activity.currency = self.resolve_activity_currency("", None, &account_currency);
            }

            if activity.currency != account_currency {
                self.fx_service
                    .register_currency_pair(activity.currency.as_str(), account_currency.as_str())
                    .await?;
            }
        }

        // Normalize amounts to absolute values (direction is determined by activity type)
        activity.quantity = activity.quantity.map(|v| v.abs());
        activity.unit_price = activity.unit_price.map(|v| v.abs());
        activity.amount = activity.amount.map(|v| v.abs());
        activity.fee = activity.fee.map(|v| v.abs());

        // Normalize minor currency units (e.g., GBp -> GBP) and convert amounts
        if get_normalization_rule(&activity.currency).is_some() {
            let input_currency = activity.currency.clone();
            let mut normalized_currency = activity.currency.clone();
            if let Some(unit_price) = activity.unit_price {
                let (normalized_price, _) = normalize_amount(unit_price, &input_currency);
                activity.unit_price = Some(normalized_price);
            }
            if let Some(amount) = activity.amount {
                let (normalized_amount, _) = normalize_amount(amount, &input_currency);
                activity.amount = Some(normalized_amount);
            }
            if let Some(fee) = activity.fee {
                let (normalized_fee, currency) = normalize_amount(fee, &input_currency);
                activity.fee = Some(normalized_fee);
                normalized_currency = currency.to_string();
            }
            if let Some(tax) = activity.tax {
                let (normalized_tax, currency) = normalize_amount(tax, &input_currency);
                activity.tax = Some(normalized_tax);
                normalized_currency = currency.to_string();
            }
            if activity.fee.is_none() && activity.tax.is_none() {
                let (_, currency) = normalize_amount(Decimal::ZERO, &input_currency);
                normalized_currency = currency.to_string();
            }
            activity.currency = normalized_currency;
        }

        // The asset owns the contract multiplier. Activity metadata only
        // seeds a brand-new asset (see the ensure/create branch above), and
        // that seed is already reflected here because the asset is resolved
        // or created before these facts are read.
        let unit_multiplier = resolved_asset_multiplier.unwrap_or(Decimal::ONE);
        Self::ensure_new_activity_final_amount(
            &mut activity,
            resolved_asset_id.as_deref(),
            unit_multiplier,
            true,
        );
        Self::validate_new_activity_final_amount(&activity, resolved_asset_id.as_deref())?;

        // Preserve explicit idempotency key when provided (e.g., intentional manual duplicates).
        // Otherwise compute a stable content-based key for deduplication.
        let explicit_idempotency_key = activity
            .idempotency_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        if let Some(key) = explicit_idempotency_key {
            activity.idempotency_key = Some(key);
        } else if let Ok(date) = DateTime::parse_from_rfc3339(&activity.activity_date)
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDate::parse_from_str(&activity.activity_date, "%Y-%m-%d")
                    .map(|d| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default()))
            })
        {
            let key = compute_idempotency_key(
                &activity.account_id,
                &activity.activity_type,
                &date,
                activity.get_symbol_id(),
                activity.quantity,
                activity.unit_price,
                activity.amount,
                activity.fee,
                &activity.currency,
                activity.source_record_id.as_deref(),
                activity.notes.as_deref(),
            );
            activity.idempotency_key = Some(key);
        }

        if let Some(key) = activity.idempotency_key.as_ref() {
            let existing = self
                .activity_repository
                .check_existing_duplicates(std::slice::from_ref(key))?;
            if let Some(existing_activity_id) = existing.get(key) {
                return Err(Self::duplicate_activity_error(Some(existing_activity_id)));
            }
        }

        Ok(activity)
    }

    /// The checks an update passes before its asset is resolved: normalizes
    /// the date and split ratio, and returns the account currency.
    fn validate_update_request(&self, activity: &mut ActivityUpdate) -> Result<String> {
        activity.activity_date =
            self.validate_and_normalize_activity_date(&activity.activity_date)?;
        let account: Account = self.account_service.get_account(&activity.account_id)?;
        Self::validate_activity_allowed_for_account(
            &activity.activity_type,
            activity.get_symbol_id().or(activity.get_symbol_code()),
            &account,
        )?;
        let base_ccy = self.account_service.get_base_currency().unwrap_or_default();
        let account_currency = resolve_currency(&[&account.currency, &base_ccy]);

        if activity.asset.as_ref().is_some_and(|asset| {
            !asset.is_empty()
                && asset.id.as_deref().is_none_or(|id| id.trim().is_empty())
                && asset
                    .symbol
                    .as_deref()
                    .is_none_or(|symbol| symbol.trim().is_empty())
        }) {
            return Err(ActivityError::InvalidData(
                "Asset updates need either asset_id or symbol; use an empty asset object to clear"
                    .to_string(),
            )
            .into());
        }

        if activity.activity_type == ACTIVITY_TYPE_SPLIT {
            activity.amount = activity.amount.map(|v| v.map(|d| d.abs()));
            self.validate_split_ratio_update(
                &activity.id,
                &activity.activity_type,
                activity.amount,
            )?;
        }

        Ok(account_currency)
    }

    /// Stores values unsigned (the type sets the direction) and converts
    /// minor-unit currencies such as GBp to their major unit.
    fn normalize_update_values(activity: &mut ActivityUpdate) {
        activity.quantity = activity.quantity.map(|v| v.map(|d| d.abs()));
        activity.unit_price = activity.unit_price.map(|v| v.map(|d| d.abs()));
        activity.amount = activity.amount.map(|v| v.map(|d| d.abs()));
        activity.fee = activity.fee.map(|v| v.map(|d| d.abs()));
        activity.tax = activity.tax.map(|v| v.map(|d| d.abs()));

        if get_normalization_rule(&activity.currency).is_some() {
            let input_currency = activity.currency.clone();
            let mut normalized_currency = activity.currency.clone();
            if let Some(Some(unit_price)) = activity.unit_price {
                let (normalized_price, _) = normalize_amount(unit_price, &input_currency);
                activity.unit_price = Some(Some(normalized_price));
            }
            if let Some(Some(amount)) = activity.amount {
                let (normalized_amount, _) = normalize_amount(amount, &input_currency);
                activity.amount = Some(Some(normalized_amount));
            }
            if let Some(Some(fee)) = activity.fee {
                let (normalized_fee, currency) = normalize_amount(fee, &input_currency);
                activity.fee = Some(Some(normalized_fee));
                normalized_currency = currency.to_string();
            }
            if let Some(Some(tax)) = activity.tax {
                let (normalized_tax, currency) = normalize_amount(tax, &input_currency);
                activity.tax = Some(Some(normalized_tax));
                normalized_currency = currency.to_string();
            }
            if !matches!(activity.fee, Some(Some(_))) && !matches!(activity.tax, Some(Some(_))) {
                let (_, currency) = normalize_amount(rust_decimal::Decimal::ZERO, &input_currency);
                normalized_currency = currency.to_string();
            }
            activity.currency = normalized_currency;
        }
    }

    /// What `prepare_update_activity` makes of `activity`, without its writes
    /// (asset creation, quote-mode changes, manual quotes, FX pairs). The
    /// asset can only be named by id: resolving a symbol may create one.
    fn preview_prepared_update(
        &self,
        mut activity: ActivityUpdate,
        existing: Activity,
    ) -> Result<PreviewedActivityUpdate> {
        let account_currency = self.validate_update_request(&mut activity)?;
        let asset = match activity
            .get_symbol_id()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            Some(asset_id) => Some(self.asset_service.get_asset_by_id(asset_id)?),
            None if Self::requires_asset_identity(
                &activity.activity_type,
                activity.subtype.as_deref(),
            ) =>
            {
                return Err(Self::invalid_activity_data(
                    "Asset-backed activities need either asset_id or symbol",
                ));
            }
            None => None,
        };
        if activity.currency.is_empty() {
            activity.currency = match asset {
                Some(asset) => asset.quote_ccy,
                None => self.resolve_activity_currency("", None, &account_currency),
            };
        }
        Self::normalize_update_values(&mut activity);
        activity.validate()?;
        let activity_date = Self::parse_activity_timestamp_utc(&activity.activity_date)
            .ok_or_else(|| Self::invalid_activity_data("Invalid activity date"))?;

        Ok(PreviewedActivityUpdate {
            existing,
            update: activity,
            activity_date,
        })
    }

    async fn prepare_update_activity(
        &self,
        mut activity: ActivityUpdate,
    ) -> Result<ActivityUpdate> {
        let account_currency = self.validate_update_request(&mut activity)?;
        let currency = resolve_currency(&[&activity.currency, &account_currency]);

        // Extract asset fields
        let symbol = activity.get_symbol_code().map(|s| s.to_string());
        let exchange_mic = activity.get_exchange_mic().map(|s| s.to_string());
        let asset_kind_input = activity.get_kind().map(|s| s.to_string());
        let quote_ccy_input = Self::normalize_quote_ccy(activity.get_quote_ccy());
        let instrument_type_input = Self::parse_instrument_type(activity.get_instrument_type());
        let asset_name = activity.get_name().map(|s| s.to_string());
        let provider_id_input = activity.asset.as_ref().and_then(|a| a.provider_id.clone());
        let provider_symbol_input = activity
            .asset
            .as_ref()
            .and_then(|a| a.provider_symbol.clone());
        let quote_mode = activity.get_quote_mode().map(|s| s.to_string());
        let parsed_quote_mode =
            quote_mode
                .as_deref()
                .and_then(|mode| match mode.to_uppercase().as_str() {
                    "MANUAL" => Some(QuoteMode::Manual),
                    "MARKET" => Some(QuoteMode::Market),
                    _ => None,
                });

        let inferred = symbol.as_deref().map(|s| {
            self.infer_asset_kind(s, exchange_mic.as_deref(), asset_kind_input.as_deref())
        });
        let inferred_instrument_type = inferred.as_ref().and_then(|(_, it)| it.clone());
        let effective_instrument_type = instrument_type_input
            .clone()
            .or(inferred_instrument_type.clone());
        let effective_kind = instrument_type_input
            .as_ref()
            .map(Self::kind_from_instrument_type)
            .or_else(|| inferred.as_ref().map(|(kind, _)| kind.clone()));

        // Normalize symbol + MIC using payload/suffix only (no live lookup for user save paths).
        let is_crypto = effective_instrument_type.as_ref() == Some(&InstrumentType::Crypto);
        let is_non_security_instrument = matches!(
            effective_instrument_type.as_ref(),
            Some(InstrumentType::Crypto | InstrumentType::Fx)
        );
        // A supplied MIC decides whether a trailing `.X` is a venue or part of the
        // ticker: `ZAAA.F` on Cboe Canada is a hedged unit class, not a Frankfurt
        // listing. See `parse_symbol_with_known_exchange`.
        let (base_symbol, suffix_mic) = symbol
            .as_deref()
            .map(|sym| parse_symbol_with_known_exchange(sym, exchange_mic.as_deref()))
            .unwrap_or(("", None));
        let exchange_mic = if is_non_security_instrument {
            None
        } else {
            exchange_mic.or_else(|| suffix_mic.map(|mic| mic.to_string()))
        };
        let is_option = effective_instrument_type.as_ref() == Some(&InstrumentType::Option);
        let normalized_symbol_for_lookup = if base_symbol.is_empty() {
            None
        } else if is_crypto {
            Some(
                parse_crypto_pair_symbol(base_symbol)
                    .map(|(base, _)| base)
                    .unwrap_or_else(|| base_symbol.to_string()),
            )
        } else if is_option {
            Some(
                crate::utils::occ_symbol::normalize_option_symbol(base_symbol)
                    .unwrap_or_else(|| base_symbol.to_string()),
            )
        } else {
            Some(base_symbol.to_string())
        };
        let submitted_asset_id = self.resolved_submitted_asset_id(
            activity.get_symbol_id(),
            normalized_symbol_for_lookup.as_deref(),
            exchange_mic.as_deref(),
            effective_instrument_type.as_ref(),
            quote_ccy_input.as_deref(),
        )?;
        if let Some(asset_input) = activity.asset.as_mut() {
            asset_input.id = submitted_asset_id.clone();
        }

        match symbol.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw_symbol) => {
                if is_garbage_symbol(raw_symbol) {
                    return Err(ActivityError::InvalidData(format!(
                        "Invalid symbol '{}'. Please search for a valid ticker.",
                        raw_symbol
                    ))
                    .into());
                }

                self.asset_service.validate_persisted_symbol_metadata(
                    normalized_symbol_for_lookup
                        .as_deref()
                        .unwrap_or(raw_symbol),
                    submitted_asset_id.as_deref(),
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    parsed_quote_mode,
                    quote_ccy_input.as_deref(),
                )?;
            }
            None if activity
                .get_symbol_id()
                .filter(|id| !id.trim().is_empty())
                .is_none()
                && Self::requires_asset_identity(
                    &activity.activity_type,
                    activity.subtype.as_deref(),
                ) =>
            {
                return Err(ActivityError::InvalidData(
                    "Asset-backed activities need either asset_id or symbol".to_string(),
                )
                .into());
            }
            None => {}
        }

        let quote_lookup_symbol = normalized_symbol_for_lookup.clone().unwrap_or_default();
        let mut quote_ccy_for_asset = quote_ccy_input.clone();
        let asset_currency = if is_crypto {
            symbol
                .as_deref()
                .and_then(parse_crypto_pair_symbol)
                .map(|(_, quote)| quote)
                .or_else(|| quote_ccy_input.clone())
                .unwrap_or_else(|| currency.clone())
        } else if is_non_security_instrument {
            quote_ccy_input.clone().unwrap_or(currency.clone())
        } else {
            let existing_asset_quote_ccy = self
                .existing_asset_quote_ccy_by_id(submitted_asset_id.as_deref())
                .or_else(|| {
                    normalized_symbol_for_lookup
                        .as_deref()
                        .and_then(|resolved_symbol| {
                            self.asset_service.existing_quote_ccy_by_symbol(
                                resolved_symbol,
                                exchange_mic.as_deref(),
                                effective_instrument_type.as_ref(),
                            )
                        })
                });
            let (resolved_quote_ccy, resolution_source) = self
                .resolve_quote_ccy(
                    quote_lookup_symbol.as_str(),
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    quote_ccy_input.as_deref(),
                    existing_asset_quote_ccy.as_deref(),
                    currency.as_str(),
                    false,
                )
                .await;
            if matches!(
                resolution_source,
                QuoteCcyResolutionSource::ExplicitInput | QuoteCcyResolutionSource::ProviderQuote
            ) {
                quote_ccy_for_asset = Some(resolved_quote_ccy.clone());
            }
            resolved_quote_ccy
        };

        // Resolve asset_id (same logic as prepare_new_activity)
        let resolved_asset_id = if let Some(ref normalized_symbol) = normalized_symbol_for_lookup {
            let existing_id = self
                .find_existing_asset_id(
                    normalized_symbol,
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    Some(&asset_currency),
                )
                .or_else(|| submitted_asset_id.clone());

            if let Some(id) = existing_id {
                Some(id)
            } else {
                let new_id = Uuid::new_v4().to_string();
                let structured_metadata = if let Some(mult) =
                    Self::custom_option_multiplier(activity.metadata.as_deref())
                {
                    crate::assets::build_option_metadata(normalized_symbol, mult)
                } else {
                    crate::assets::build_asset_metadata(
                        effective_instrument_type.as_ref(),
                        normalized_symbol,
                    )
                };
                let metadata = crate::assets::AssetMetadata {
                    name: asset_name.clone(),
                    kind: effective_kind.clone(),
                    instrument_exchange_mic: exchange_mic.clone(),
                    instrument_symbol: Some(normalized_symbol.clone()),
                    instrument_type: effective_instrument_type.clone(),
                    display_code: Some(normalized_symbol.clone()),
                    requested_quote_ccy: quote_ccy_for_asset.clone(),
                    provider_config: None,
                    provider_id: provider_id_input.clone(),
                    provider_symbol: provider_symbol_input.clone(),
                    asset_metadata: structured_metadata,
                };
                self.asset_service
                    .get_or_create_minimal_asset(
                        &new_id,
                        Some(asset_currency.clone()),
                        Some(metadata),
                        quote_mode.clone(),
                    )
                    .await?;
                Some(new_id)
            }
        } else if let Some(asset_id) = submitted_asset_id.as_deref() {
            Some(asset_id.to_string())
        } else if !Self::requires_asset_identity(
            &activity.activity_type,
            activity.subtype.as_deref(),
        ) {
            None
        } else {
            return Err(ActivityError::InvalidData(
                "Asset-backed activities need either asset_id or symbol".to_string(),
            )
            .into());
        };

        // Update activity's asset with resolved asset_id
        if let Some(ref resolved_id) = resolved_asset_id {
            match activity.asset.as_mut() {
                Some(asset) => asset.id = Some(resolved_id.clone()),
                None => {
                    activity.asset = Some(AssetResolutionInput {
                        id: Some(resolved_id.clone()),
                        ..Default::default()
                    });
                }
            }
        }

        // Process asset if asset_id is resolved
        if let Some(ref asset_id) = resolved_asset_id {
            let canonical_symbol = normalized_symbol_for_lookup.clone();
            let metadata = crate::assets::AssetMetadata {
                name: asset_name.clone(),
                kind: effective_kind,
                instrument_exchange_mic: exchange_mic.clone(),
                instrument_symbol: canonical_symbol.clone(),
                instrument_type: effective_instrument_type.clone(),
                display_code: canonical_symbol,
                requested_quote_ccy: quote_ccy_for_asset.clone(),
                provider_config: None,
                provider_id: provider_id_input,
                provider_symbol: provider_symbol_input,
                asset_metadata: None,
            };
            let mut asset = if normalized_symbol_for_lookup.is_none() {
                self.asset_service.get_asset_by_id(asset_id)?
            } else {
                self.asset_service
                    .get_or_create_minimal_asset(
                        asset_id,
                        Some(asset_currency.clone()),
                        Some(metadata),
                        quote_mode.clone(),
                    )
                    .await?
            };

            // Update asset quote mode if specified
            if let Some(ref mode) = quote_mode {
                let requested_mode = mode.to_uppercase();
                let current_mode = asset.quote_mode.as_db_str();
                if requested_mode != current_mode {
                    asset = self
                        .asset_service
                        .update_quote_mode_silent(&asset.id, &requested_mode)
                        .await?;
                }
            }

            // Create a quote from the activity price as a fallback, but only
            // for MANUAL-mode assets. For MARKET-mode assets the unit price is
            // a cost input, not a market price, and writing it here would
            // shadow provider quotes.
            let is_manual_mode = asset.quote_mode == QuoteMode::Manual
                || matches!(parsed_quote_mode, Some(QuoteMode::Manual));
            if is_manual_mode
                && PRICE_BEARING_ACTIVITY_TYPES.contains(&activity.activity_type.as_str())
            {
                if let Some(Some(unit_price)) = activity.unit_price {
                    let source = DATA_SOURCE_MANUAL.to_string();
                    self.create_quote_from_activity(
                        asset_id,
                        unit_price,
                        &currency,
                        &activity.activity_date,
                        source,
                    )
                    .await?;
                }
            }

            if activity.currency.is_empty() {
                activity.currency = asset.quote_ccy.clone();
            }

            if activity.currency != account_currency {
                self.fx_service
                    .register_currency_pair(activity.currency.as_str(), account_currency.as_str())
                    .await?;
            }

            if asset.quote_ccy != account_currency && asset.quote_ccy != activity.currency {
                self.fx_service
                    .register_currency_pair(asset.quote_ccy.as_str(), account_currency.as_str())
                    .await?;
            }
        } else {
            if activity.currency.is_empty() {
                activity.currency = self.resolve_activity_currency("", None, &account_currency);
            }

            if activity.currency != account_currency {
                self.fx_service
                    .register_currency_pair(activity.currency.as_str(), account_currency.as_str())
                    .await?;
            }
        }

        Self::normalize_update_values(&mut activity);

        Ok(activity)
    }

    /// Builds an AssetSpec from a NewActivity.
    /// Returns None for cash activities that don't need an asset.
    async fn build_asset_spec(
        &self,
        activity: &NewActivity,
        account: &Account,
    ) -> Result<Option<crate::assets::AssetSpec>> {
        use crate::assets::{parse_crypto_pair_symbol, AssetSpec};

        let base_ccy = self.account_service.get_base_currency().unwrap_or_default();
        let account_currency = resolve_currency(&[&account.currency, &base_ccy]);
        let quote_ccy_input = Self::normalize_quote_ccy(activity.get_quote_ccy());

        let symbol = match activity.get_symbol_code() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                // No symbol provided - check if we have an asset_id directly (UUID)
                if let Some(asset_id) = activity.get_symbol_id() {
                    if !asset_id.is_empty() {
                        let asset_id = self
                            .resolved_submitted_asset_id(
                                Some(asset_id),
                                None,
                                None,
                                None,
                                quote_ccy_input.as_deref(),
                            )?
                            .ok_or_else(|| {
                                ActivityError::InvalidData(
                                    "Asset-backed activity needs symbol or asset_id".to_string(),
                                )
                            })?;
                        let existing_asset = self.asset_service.get_asset_by_id(&asset_id)?;

                        // asset_id is a UUID; use the existing asset to build the spec
                        let currency = Self::normalize_quote_ccy(activity.get_quote_ccy())
                            .or_else(|| {
                                if !activity.currency.is_empty() {
                                    Some(activity.currency.clone())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(existing_asset.quote_ccy);

                        let quote_mode = activity.get_quote_mode().and_then(|s| {
                            match s.to_uppercase().as_str() {
                                "MARKET" => Some(QuoteMode::Market),
                                "MANUAL" => Some(QuoteMode::Manual),
                                _ => None,
                            }
                        });

                        return Ok(Some(AssetSpec {
                            id: Some(asset_id),
                            display_code: None,
                            instrument_symbol: None,
                            instrument_exchange_mic: None,
                            instrument_type: None,
                            quote_ccy: currency,
                            requested_quote_ccy: quote_ccy_input.clone(),
                            kind: AssetKind::Investment,
                            quote_mode,
                            name: activity.get_name().map(|s| s.to_string()),
                            provider_config: None,
                            provider_id: activity
                                .asset
                                .as_ref()
                                .and_then(|asset| asset.provider_id.clone()),
                            provider_symbol: activity
                                .asset
                                .as_ref()
                                .and_then(|asset| asset.provider_symbol.clone()),
                            metadata: None,
                        }));
                    }
                }
                // Symbol-optional types with no symbol → no asset needed
                if !Self::requires_asset_identity(
                    &activity.activity_type,
                    activity.subtype.as_deref(),
                ) {
                    return Ok(None);
                }
                return Err(ActivityError::InvalidData(
                    "Asset-backed activity needs symbol or asset_id".to_string(),
                )
                .into());
            }
        };

        if is_garbage_symbol(symbol.as_str()) {
            return Err(ActivityError::InvalidData(format!(
                "Invalid symbol '{}'. Please search for a valid ticker.",
                symbol
            ))
            .into());
        }

        // Strip Yahoo suffix from symbol (e.g. GOOG.TO → GOOG + XTSE), unless the row
        // already names a venue the suffix disagrees with — then the suffix is part of
        // the ticker, as with a `.F` hedged class on Cboe Canada.
        let (base_symbol, suffix_mic) =
            parse_symbol_with_known_exchange(&symbol, activity.get_exchange_mic());

        // Preserve the venue supplied by the user or encoded in a recognized suffix.
        let exchange_mic = activity
            .get_exchange_mic()
            .map(|s| s.to_string())
            .or_else(|| suffix_mic.map(|s| s.to_string()));

        // Determine currency
        let currency = if !activity.currency.is_empty() {
            activity.currency.clone()
        } else {
            account_currency.clone()
        };

        let instrument_type_input = Self::parse_instrument_type(activity.get_instrument_type());

        // Infer asset kind and instrument type using base symbol
        let (inferred_kind, inferred_instrument_type) =
            self.infer_asset_kind(base_symbol, exchange_mic.as_deref(), activity.get_kind());
        let instrument_type = instrument_type_input.clone().or(inferred_instrument_type);
        let kind = instrument_type_input
            .as_ref()
            .map(Self::kind_from_instrument_type)
            .unwrap_or(inferred_kind);

        // Parse quote mode if provided
        let quote_mode = activity
            .get_quote_mode()
            .and_then(|s| match s.to_uppercase().as_str() {
                "MARKET" => Some(QuoteMode::Market),
                "MANUAL" => Some(QuoteMode::Manual),
                _ => None,
            });

        // Crypto/FX assets don't have exchange MICs — clear any that leaked from frontend/suffix
        let is_crypto = instrument_type.as_ref() == Some(&InstrumentType::Crypto);
        let is_non_security = matches!(
            instrument_type.as_ref(),
            Some(InstrumentType::Crypto | InstrumentType::Fx)
        );
        let is_option = instrument_type.as_ref() == Some(&InstrumentType::Option);
        // OCC option symbols are globally unique — exchange MIC would fragment identity
        let exchange_mic = if is_non_security || is_option {
            None
        } else {
            exchange_mic
        };
        let normalized_symbol = if is_crypto {
            parse_crypto_pair_symbol(base_symbol)
                .map(|(base, _)| base)
                .unwrap_or_else(|| base_symbol.to_string())
        } else if is_option {
            crate::utils::occ_symbol::normalize_option_symbol(base_symbol)
                .unwrap_or_else(|| base_symbol.to_string())
        } else {
            base_symbol.to_string()
        };
        let submitted_asset_id = self.resolved_submitted_asset_id(
            activity.get_symbol_id(),
            Some(normalized_symbol.as_str()),
            exchange_mic.as_deref(),
            instrument_type.as_ref(),
            quote_ccy_input.as_deref(),
        )?;

        self.asset_service.validate_persisted_symbol_metadata(
            normalized_symbol.as_str(),
            submitted_asset_id.as_deref(),
            exchange_mic.as_deref(),
            instrument_type.as_ref(),
            quote_mode,
            quote_ccy_input.as_deref(),
        )?;

        // For crypto, use the quote currency from the pair if available
        let mut quote_ccy_for_asset = quote_ccy_input.clone();
        let asset_currency = if is_crypto {
            parse_crypto_pair_symbol(base_symbol)
                .map(|(_, quote)| quote)
                .or_else(|| quote_ccy_input.clone())
                .unwrap_or_else(|| currency.clone())
        } else {
            let existing_asset_quote_ccy = self
                .existing_asset_quote_ccy_by_id(submitted_asset_id.as_deref())
                .or_else(|| {
                    self.asset_service.existing_quote_ccy_by_symbol(
                        normalized_symbol.as_str(),
                        exchange_mic.as_deref(),
                        instrument_type.as_ref(),
                    )
                });
            let (resolved_quote_ccy, resolution_source) = resolve_quote_ccy_precedence(
                quote_ccy_input.as_deref(),
                existing_asset_quote_ccy.as_deref(),
                None,
                exchange_mic.as_deref().and_then(mic_to_currency),
                Some(currency.as_str()),
            )
            .unwrap_or_else(|| (currency.clone(), QuoteCcyResolutionSource::TerminalFallback));
            if matches!(
                resolution_source,
                QuoteCcyResolutionSource::ExplicitInput | QuoteCcyResolutionSource::ProviderQuote
            ) {
                quote_ccy_for_asset = Some(resolved_quote_ccy.clone());
            }
            resolved_quote_ccy
        };

        // Normalize activity bonds the same way as holdings before asset lookup.
        let (asset_symbol, asset_exchange_mic) = if instrument_type == Some(InstrumentType::Bond) {
            let canonical = canonicalize_market_identity(
                instrument_type.clone(),
                Some(&normalized_symbol),
                exchange_mic.as_deref(),
                Some(&asset_currency),
            );
            (
                canonical
                    .instrument_symbol
                    .unwrap_or_else(|| normalized_symbol.clone()),
                canonical.instrument_exchange_mic,
            )
        } else {
            (normalized_symbol.clone(), exchange_mic)
        };

        // Look up existing asset by instrument fields to get its UUID
        let existing_id = if instrument_type == Some(InstrumentType::Bond) {
            submitted_asset_id
        } else {
            self.find_existing_asset_id(
                &asset_symbol,
                asset_exchange_mic.as_deref(),
                instrument_type.as_ref(),
                Some(&asset_currency),
            )
            .or(submitted_asset_id)
        };
        let asset_metadata = if is_option {
            Self::custom_option_multiplier(activity.metadata.as_deref()).and_then(|multiplier| {
                crate::assets::build_option_metadata(&normalized_symbol, multiplier)
            })
        } else {
            None
        };

        let spec = AssetSpec {
            id: existing_id,
            display_code: Some(asset_symbol.clone()),
            instrument_symbol: Some(asset_symbol),
            instrument_exchange_mic: asset_exchange_mic,
            instrument_type,
            quote_ccy: asset_currency,
            requested_quote_ccy: quote_ccy_for_asset,
            kind,
            quote_mode,
            name: activity.get_name().map(|s| s.to_string()),
            provider_config: None,
            provider_id: activity
                .asset
                .as_ref()
                .and_then(|asset| asset.provider_id.clone()),
            provider_symbol: activity
                .asset
                .as_ref()
                .and_then(|asset| asset.provider_symbol.clone()),
            metadata: asset_metadata,
        };
        Ok(Some(spec))
    }
}

#[async_trait::async_trait]
impl ActivityServiceTrait for ActivityService {
    fn get_activity(&self, activity_id: &str) -> Result<Activity> {
        self.activity_repository.get_activity(activity_id)
    }

    /// Retrieves all activities
    fn get_activities(&self) -> Result<Vec<Activity>> {
        self.activity_repository.get_activities()
    }

    fn get_activities_including_archived_accounts(&self) -> Result<Vec<Activity>> {
        self.activity_repository
            .get_activities_including_archived_accounts()
    }

    /// Retrieves activities by account ID
    fn get_activities_by_account_id(&self, account_id: &str) -> Result<Vec<Activity>> {
        self.activity_repository
            .get_activities_by_account_id(account_id)
    }

    /// Retrieves activities by account IDs
    fn get_activities_by_account_ids(&self, account_ids: &[String]) -> Result<Vec<Activity>> {
        self.activity_repository
            .get_activities_by_account_ids(account_ids)
    }

    /// Retrieves all trading activities
    fn get_trading_activities(&self) -> Result<Vec<Activity>> {
        self.activity_repository.get_trading_activities()
    }

    /// Retrieves all income activities
    fn get_income_activities(&self) -> Result<Vec<Activity>> {
        self.activity_repository.get_income_activities()
    }

    /// Searches activities with various filters and pagination
    fn search_activities(
        &self,
        page: i64,
        page_size: i64,
        account_id_filter: Option<Vec<String>>,
        activity_type_filter: Option<Vec<String>>,
        asset_id_keyword: Option<String>,
        sort: Option<Sort>,
        needs_review_filter: Option<bool>,
        date_from: Option<NaiveDate>,
        date_to: Option<NaiveDate>,
        instrument_type_filter: Option<Vec<String>>,
        activity_id_filter: Option<Vec<String>>,
    ) -> Result<ActivitySearchResponse> {
        self.activity_repository.search_activities(
            page,
            page_size,
            account_id_filter,
            activity_type_filter,
            asset_id_keyword,
            sort,
            needs_review_filter,
            date_from,
            date_to,
            instrument_type_filter,
            activity_id_filter,
        )
    }

    /// Searches activities using an exact UTC timestamp window for date filters.
    #[allow(clippy::too_many_arguments)]
    fn search_activities_in_utc_range(
        &self,
        page: i64,
        page_size: i64,
        account_id_filter: Option<Vec<String>>,
        activity_type_filter: Option<Vec<String>>,
        asset_id_keyword: Option<String>,
        sort: Option<Sort>,
        needs_review_filter: Option<bool>,
        date_from_utc: Option<DateTime<Utc>>,
        date_to_utc_exclusive: Option<DateTime<Utc>>,
        instrument_type_filter: Option<Vec<String>>,
        activity_id_filter: Option<Vec<String>>,
    ) -> Result<ActivitySearchResponse> {
        self.activity_repository.search_activities_in_utc_range(
            page,
            page_size,
            account_id_filter,
            activity_type_filter,
            asset_id_keyword,
            sort,
            needs_review_filter,
            date_from_utc,
            date_to_utc_exclusive,
            instrument_type_filter,
            activity_id_filter,
        )
    }

    /// Creates a new activity
    async fn prepare_activity_for_commit(&self, activity: NewActivity) -> Result<NewActivity> {
        self.prepare_new_activity(activity).await
    }

    async fn prepare_activity_update_for_commit(
        &self,
        mut activity: ActivityUpdate,
    ) -> Result<(ActivityUpdate, Activity)> {
        let existing = self.activity_repository.get_activity(&activity.id)?;
        self.hydrate_and_validate_update_against_existing(&mut activity, &existing)?;
        let prepared = self.prepare_update_activity(activity).await?;
        Ok((prepared, existing))
    }

    fn notify_activity_committed(&self, previous: Option<&Activity>, current: &Activity) {
        let mut account_ids = HashSet::new();
        let mut asset_ids = HashSet::new();
        let mut currencies = HashSet::new();
        if let Some(previous) = previous {
            Self::add_activity_to_event_sets(
                previous,
                &mut account_ids,
                &mut asset_ids,
                &mut currencies,
            );
        }
        Self::add_activity_to_event_sets(
            current,
            &mut account_ids,
            &mut asset_ids,
            &mut currencies,
        );
        self.emit_activities_changed(
            account_ids.into_iter().collect(),
            asset_ids.into_iter().collect(),
            currencies.into_iter().collect(),
            Some(
                current.activity_date.min(
                    previous
                        .map(|previous| previous.activity_date)
                        .unwrap_or(current.activity_date),
                ),
            ),
        );
        self.emit_asset_split_activities_changed(std::iter::once(current));
    }

    async fn create_activity(&self, activity: NewActivity) -> Result<Activity> {
        let prepared = self.prepare_new_activity(activity).await?;
        let created = self
            .activity_repository
            .create_activity(prepared)
            .await
            .map_err(Self::map_duplicate_idempotency_violation)?;

        // Emit domain event after successful creation
        let account_ids = vec![created.account_id.clone()];
        let asset_ids = created.asset_id.clone().into_iter().collect();
        let currencies = vec![created.currency.clone()];
        self.emit_activities_changed(
            account_ids,
            asset_ids,
            currencies,
            Some(created.activity_date),
        );
        self.emit_asset_split_activities_changed(std::iter::once(&created));

        Ok(created)
    }

    /// Updates an existing activity
    async fn update_activity(&self, mut activity: ActivityUpdate) -> Result<Activity> {
        // Get the existing activity BEFORE the update to capture old account_id and asset_id
        // This ensures we emit events for both old and new locations if they changed
        let existing = self.activity_repository.get_activity(&activity.id)?;
        self.hydrate_and_validate_update_against_existing(&mut activity, &existing)?;

        let pair = self.load_internal_transfer_pair_for_activity(&activity.id)?;
        let counterpart_update = match pair.as_ref() {
            Some(pair) => {
                Self::validate_internal_cash_pair_currency_update(&activity, &existing, pair)?;
                self.build_counterpart_update(&activity, &existing, pair)?
            }
            None => None,
        };

        let prepared = self.prepare_update_activity(activity).await?;

        if let Some(mut counterpart_update) = counterpart_update {
            let counterpart_existing = self
                .activity_repository
                .get_activity(&counterpart_update.id)?;
            self.hydrate_and_validate_update_against_existing(
                &mut counterpart_update,
                &counterpart_existing,
            )?;
            let prepared_counterpart = self.prepare_update_activity(counterpart_update).await?;
            let persisted = self
                .activity_repository
                .bulk_mutate_activities(
                    Vec::new(),
                    vec![prepared.clone(), prepared_counterpart],
                    Vec::new(),
                )
                .await?;

            let mut account_ids_set: HashSet<String> = HashSet::new();
            let mut asset_ids_set: HashSet<String> = HashSet::new();
            let mut currencies_set: HashSet<String> = HashSet::new();
            Self::add_activity_to_event_sets(
                &existing,
                &mut account_ids_set,
                &mut asset_ids_set,
                &mut currencies_set,
            );
            Self::add_activity_to_event_sets(
                &counterpart_existing,
                &mut account_ids_set,
                &mut asset_ids_set,
                &mut currencies_set,
            );
            for updated in &persisted.updated {
                Self::add_activity_to_event_sets(
                    updated,
                    &mut account_ids_set,
                    &mut asset_ids_set,
                    &mut currencies_set,
                );
            }

            let updated = persisted
                .updated
                .into_iter()
                .find(|updated| updated.id == prepared.id)
                .ok_or_else(|| {
                    Self::invalid_activity_data("Updated transfer leg was not returned")
                })?;
            let earliest_activity_at_utc = Self::earliest_activity_at_utc(
                [&existing, &counterpart_existing]
                    .into_iter()
                    .chain(std::iter::once(&updated)),
            );
            self.emit_activities_changed(
                account_ids_set.into_iter().collect(),
                asset_ids_set.into_iter().collect(),
                currencies_set.into_iter().collect(),
                earliest_activity_at_utc,
            );

            return Ok(updated);
        }

        let updated = self.activity_repository.update_activity(prepared).await?;

        // Emit domain event after successful update
        // Include BOTH old and new account_ids and asset_ids (if they differ)
        let mut account_ids_set: HashSet<String> = HashSet::new();
        let mut asset_ids_set: HashSet<String> = HashSet::new();
        let mut currencies_set: HashSet<String> = HashSet::new();

        // Add old values
        account_ids_set.insert(existing.account_id.clone());
        if let Some(ref old_asset_id) = existing.asset_id {
            asset_ids_set.insert(old_asset_id.clone());
        }
        currencies_set.insert(existing.currency.clone());

        // Add new values
        account_ids_set.insert(updated.account_id.clone());
        if let Some(ref new_asset_id) = updated.asset_id {
            asset_ids_set.insert(new_asset_id.clone());
        }
        currencies_set.insert(updated.currency.clone());

        // Only a linked pair's other leg is kept in step (above). Other rows
        // sharing the source group, such as a broker's fee row, stay as they
        // are, as in `bulk_mutate_activities`.

        let account_ids: Vec<String> = account_ids_set.into_iter().collect();
        let asset_ids: Vec<String> = asset_ids_set.into_iter().collect();
        let currencies: Vec<String> = currencies_set.into_iter().collect();
        let earliest_activity_at_utc = existing.activity_date.min(updated.activity_date);
        self.emit_activities_changed(
            account_ids,
            asset_ids,
            currencies,
            Some(earliest_activity_at_utc),
        );
        self.emit_asset_split_activities_changed([&existing, &updated]);

        Ok(updated)
    }

    fn preview_activity_update(
        &self,
        mut activity: ActivityUpdate,
    ) -> Result<ActivityUpdatePreview> {
        // The same steps as `update_activity`, up to its writes.
        let existing = self.activity_repository.get_activity(&activity.id)?;
        if activity
            .get_symbol_code()
            .is_some_and(|symbol| !symbol.trim().is_empty())
        {
            return Err(Self::invalid_activity_data(
                "An update preview names the asset by its id, not by symbol",
            ));
        }
        self.hydrate_and_validate_update_against_existing(&mut activity, &existing)?;

        let pair = self.load_internal_transfer_pair_for_activity(&activity.id)?;
        let counterpart_update = match pair.as_ref() {
            Some(pair) => self.build_counterpart_update(&activity, &existing, pair)?,
            None => None,
        };
        let activity = self.preview_prepared_update(activity, existing)?;
        let linked = match counterpart_update {
            Some(mut counterpart_update) => {
                let counterpart_existing = self
                    .activity_repository
                    .get_activity(&counterpart_update.id)?;
                self.hydrate_and_validate_update_against_existing(
                    &mut counterpart_update,
                    &counterpart_existing,
                )?;
                Some(self.preview_prepared_update(counterpart_update, counterpart_existing)?)
            }
            None => None,
        };

        Ok(ActivityUpdatePreview { activity, linked })
    }

    /// Deletes an activity
    async fn delete_activity(&self, activity_id: String) -> Result<Activity> {
        if let Some(pair) = self.load_internal_transfer_pair_for_activity(&activity_id)? {
            let delete_ids = vec![pair.transfer_out.id.clone(), pair.transfer_in.id.clone()];
            let persisted = self
                .activity_repository
                .bulk_mutate_activities(Vec::new(), Vec::new(), delete_ids)
                .await?;

            let deleted = persisted
                .deleted
                .iter()
                .find(|activity| activity.id == activity_id)
                .cloned()
                .ok_or_else(|| {
                    Self::invalid_activity_data("Deleted transfer leg was not returned")
                })?;

            let mut account_ids_set: HashSet<String> = HashSet::new();
            let mut asset_ids_set: HashSet<String> = HashSet::new();
            let mut currencies_set: HashSet<String> = HashSet::new();
            for activity in &persisted.deleted {
                Self::add_activity_to_event_sets(
                    activity,
                    &mut account_ids_set,
                    &mut asset_ids_set,
                    &mut currencies_set,
                );
            }
            self.emit_activities_changed(
                account_ids_set.into_iter().collect(),
                asset_ids_set.into_iter().collect(),
                currencies_set.into_iter().collect(),
                Self::earliest_activity_at_utc(persisted.deleted.iter()),
            );

            return Ok(deleted);
        }

        let deleted = self
            .activity_repository
            .delete_activity(activity_id)
            .await?;

        // Emit domain event after successful deletion
        let account_ids = vec![deleted.account_id.clone()];
        let asset_ids = deleted.asset_id.clone().into_iter().collect();
        let currencies = vec![deleted.currency.clone()];
        self.emit_activities_changed(
            account_ids,
            asset_ids,
            currencies,
            Some(deleted.activity_date),
        );
        self.emit_asset_split_activities_changed(std::iter::once(&deleted));

        Ok(deleted)
    }

    fn get_transfer_pair_for_activity(
        &self,
        activity_id: String,
    ) -> Result<Option<InternalTransferPairResponse>> {
        Ok(self
            .load_internal_transfer_pair_for_activity(&activity_id)?
            .map(Self::transfer_pair_response))
    }

    fn find_transfer_match_candidates(
        &self,
        request: TransferMatchCandidateRequest,
    ) -> Result<Vec<TransferMatchCandidate>> {
        let source = self
            .activity_repository
            .get_activity(&request.activity_id)?;
        let Some(opposite_type) = Self::opposite_transfer_type(source.effective_type()) else {
            return Err(Self::invalid_activity_data(
                "Transfer match candidates require a transfer activity",
            ));
        };
        let all_activities = self.activity_repository.get_activities()?;
        let transfer_resolution = TransferPairResolution::from_activities(&all_activities);
        if transfer_resolution.pair_for_activity(&source.id).is_some() {
            return Ok(Vec::new());
        }

        Ok(Self::transfer_match_candidates(
            &source,
            opposite_type,
            &all_activities,
            &transfer_resolution,
            request.window_days.unwrap_or(7).clamp(0, 90),
            request.limit.unwrap_or(25).clamp(1, 100),
        ))
    }

    async fn find_unlinked_transfers(
        &self,
        request: UnlinkedTransfersRequest,
    ) -> Result<UnlinkedTransfers> {
        let activity_repository = Arc::clone(&self.activity_repository);
        let account_service = Arc::clone(&self.account_service);
        crate::portfolio::coordinator::blocking(move || {
            Self::scan_unlinked_transfers(
                activity_repository.as_ref(),
                account_service.as_ref(),
                request,
            )
        })
        .await
    }

    async fn prepare_internal_cash_transfer(
        &self,
        request: InternalTransferPairRequest,
    ) -> Result<Vec<NewActivity>> {
        if request.transfer_out_id.is_some() || request.transfer_in_id.is_some() {
            return Err(Self::invalid_activity_data(
                "creation does not accept existing leg ids",
            ));
        }
        let values = self.validate_internal_pair_request(&request)?;
        let mut prepared = Vec::new();
        for activity in Self::build_internal_pair_create_request(&request, &values) {
            prepared.push(self.prepare_new_activity(activity).await?);
        }
        Ok(prepared)
    }

    async fn save_internal_transfer_pair(
        &self,
        request: InternalTransferPairRequest,
    ) -> Result<InternalTransferPairResponse> {
        let pair_values = self.validate_internal_pair_request(&request)?;

        let is_update = request.transfer_out_id.is_some() || request.transfer_in_id.is_some();
        let mut old_account_ids: HashSet<String> = HashSet::new();
        let mut old_asset_ids: HashSet<String> = HashSet::new();
        let mut old_currencies: HashSet<String> = HashSet::new();
        let mut old_activities: Vec<Activity> = Vec::new();

        let persisted = if is_update {
            let transfer_out_id = request
                .transfer_out_id
                .clone()
                .ok_or_else(|| Self::invalid_activity_data("Transfer out id is required"))?;
            let transfer_in_id = request
                .transfer_in_id
                .clone()
                .ok_or_else(|| Self::invalid_activity_data("Transfer in id is required"))?;

            let pair = self.require_internal_transfer_pair_for_activity(&transfer_out_id)?;
            if pair.transfer_in.id != transfer_in_id {
                return Err(Self::invalid_activity_data(
                    "Transfer legs do not belong to the same pair",
                ));
            }
            if !Self::is_cash_transfer_pair(&pair) {
                return Err(Self::invalid_activity_data(
                    "Pair save currently supports internal cash transfers only",
                ));
            }

            for activity in [&pair.transfer_out, &pair.transfer_in] {
                Self::add_activity_to_event_sets(
                    activity,
                    &mut old_account_ids,
                    &mut old_asset_ids,
                    &mut old_currencies,
                );
                old_activities.push(activity.clone());
            }

            let mut updates = self.build_internal_pair_updates(&request, &pair, &pair_values)?;
            for update in &mut updates {
                let existing = self.activity_repository.get_activity(&update.id)?;
                self.hydrate_and_validate_update_against_existing(update, &existing)?;
            }
            let mut prepared_updates = Vec::new();
            for update in updates {
                prepared_updates.push(self.prepare_update_activity(update).await?);
            }
            self.activity_repository
                .bulk_mutate_activities(Vec::new(), prepared_updates, Vec::new())
                .await?
        } else {
            let creates = Self::build_internal_pair_create_request(&request, &pair_values);
            let mut prepared_creates = Vec::new();
            for create in creates {
                prepared_creates.push(self.prepare_new_activity(create).await?);
            }
            self.activity_repository
                .bulk_mutate_activities(prepared_creates, Vec::new(), Vec::new())
                .await
                .map_err(Self::map_duplicate_idempotency_violation)?
        };

        let transfer_out = persisted
            .created
            .iter()
            .chain(persisted.updated.iter())
            .find(|activity| activity.activity_type == ACTIVITY_TYPE_TRANSFER_OUT)
            .cloned()
            .ok_or_else(|| Self::invalid_activity_data("Transfer out leg was not returned"))?;
        let transfer_in = persisted
            .created
            .iter()
            .chain(persisted.updated.iter())
            .find(|activity| activity.activity_type == ACTIVITY_TYPE_TRANSFER_IN)
            .cloned()
            .ok_or_else(|| Self::invalid_activity_data("Transfer in leg was not returned"))?;

        let mut account_ids_set = old_account_ids;
        let mut asset_ids_set = old_asset_ids;
        let mut currencies_set = old_currencies;
        for activity in [&transfer_out, &transfer_in] {
            Self::add_activity_to_event_sets(
                activity,
                &mut account_ids_set,
                &mut asset_ids_set,
                &mut currencies_set,
            );
        }
        self.emit_activities_changed(
            account_ids_set.into_iter().collect(),
            asset_ids_set.into_iter().collect(),
            currencies_set.into_iter().collect(),
            Self::earliest_activity_at_utc(
                old_activities
                    .iter()
                    .chain(std::iter::once(&transfer_out))
                    .chain(std::iter::once(&transfer_in)),
            ),
        );

        Ok(InternalTransferPairResponse {
            transfer_out,
            transfer_in,
        })
    }

    async fn link_transfer_activities(
        &self,
        activity_a_id: String,
        activity_b_id: String,
    ) -> Result<(Activity, Activity)> {
        let (transfer_in, transfer_out) = self
            .activity_repository
            .link_transfer_activities(activity_a_id, activity_b_id)
            .await?;

        let mut account_ids: HashSet<String> = HashSet::new();
        let mut asset_ids: HashSet<String> = HashSet::new();
        let mut currencies: HashSet<String> = HashSet::new();
        for activity in [&transfer_in, &transfer_out] {
            account_ids.insert(activity.account_id.clone());
            if let Some(ref asset_id) = activity.asset_id {
                asset_ids.insert(asset_id.clone());
            }
            currencies.insert(activity.currency.clone());
        }
        let earliest_at = transfer_in.activity_date.min(transfer_out.activity_date);
        self.emit_activities_changed(
            account_ids.into_iter().collect(),
            asset_ids.into_iter().collect(),
            currencies.into_iter().collect(),
            Some(earliest_at),
        );

        Ok((transfer_in, transfer_out))
    }

    async fn unlink_transfer_activities(
        &self,
        activity_a_id: String,
        activity_b_id: String,
    ) -> Result<(Activity, Activity)> {
        let (transfer_in, transfer_out) = self
            .activity_repository
            .unlink_transfer_activities(activity_a_id, activity_b_id)
            .await?;

        let mut account_ids: HashSet<String> = HashSet::new();
        let mut asset_ids: HashSet<String> = HashSet::new();
        let mut currencies: HashSet<String> = HashSet::new();
        for activity in [&transfer_in, &transfer_out] {
            account_ids.insert(activity.account_id.clone());
            if let Some(ref asset_id) = activity.asset_id {
                asset_ids.insert(asset_id.clone());
            }
            currencies.insert(activity.currency.clone());
        }
        let earliest_at = transfer_in.activity_date.min(transfer_out.activity_date);
        self.emit_activities_changed(
            account_ids.into_iter().collect(),
            asset_ids.into_iter().collect(),
            currencies.into_iter().collect(),
            Some(earliest_at),
        );

        Ok((transfer_in, transfer_out))
    }

    async fn bulk_mutate_activities(
        &self,
        request: ActivityBulkMutationRequest,
    ) -> Result<ActivityBulkMutationResult> {
        let mut errors: Vec<ActivityBulkMutationError> = Vec::new();
        let mut prepared_creates: Vec<NewActivity> = Vec::new();
        let mut prepared_updates: Vec<ActivityUpdate> = Vec::new();
        let mut valid_delete_ids: Vec<String> = Vec::new();

        // Capture OLD account_ids and asset_ids BEFORE updates/deletes for proper event emission
        // This ensures that when an activity moves accounts or changes assets, both old and new locations get recalculated
        let mut old_account_ids: HashSet<String> = HashSet::new();
        let mut old_asset_ids: HashSet<String> = HashSet::new();
        let mut old_currencies: HashSet<String> = HashSet::new();
        let mut old_activity_dates: Vec<DateTime<Utc>> = Vec::new();
        let mut old_activities: Vec<Activity> = Vec::new();

        let explicit_update_ids: HashSet<String> = request
            .updates
            .iter()
            .map(|update| update.id.clone())
            .collect();
        let securities_legs: HashMap<String, SecuritiesTransferLeg> = request
            .updates
            .iter()
            .filter_map(|update| {
                SecuritiesTransferLeg::from_update(update).map(|leg| (update.id.clone(), leg))
            })
            .collect();
        let mut update_requests: Vec<ActivityUpdate> = Vec::new();
        for update_request in request.updates {
            match self.activity_repository.get_activity(&update_request.id) {
                Ok(existing) => {
                    if let Some(pair) =
                        self.load_internal_transfer_pair_for_activity(&update_request.id)?
                    {
                        let counterpart_id = if existing.id == pair.transfer_in.id {
                            pair.transfer_out.id.clone()
                        } else {
                            pair.transfer_in.id.clone()
                        };
                        // The pair editor converts a cash pair to a securities
                        // transfer by updating both legs together; that is a
                        // complete pair edit, not a single-leg one.
                        let converts_pair_to_securities = securities_legs
                            .get(&update_request.id)
                            .zip(securities_legs.get(&counterpart_id))
                            .is_some_and(|(leg, counterpart)| leg.pairs_with(counterpart));
                        if !converts_pair_to_securities {
                            if let Err(err) = Self::validate_internal_cash_pair_currency_update(
                                &update_request,
                                &existing,
                                &pair,
                            ) {
                                errors.push(ActivityBulkMutationError {
                                    id: Some(update_request.id.clone()),
                                    action: "update".to_string(),
                                    message: err.to_string(),
                                });
                                continue;
                            }
                        }

                        if !explicit_update_ids.contains(&counterpart_id) {
                            match self.build_counterpart_update(&update_request, &existing, &pair) {
                                Ok(Some(counterpart_update)) => {
                                    update_requests.push(counterpart_update);
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    errors.push(ActivityBulkMutationError {
                                        id: Some(update_request.id.clone()),
                                        action: "update".to_string(),
                                        message: err.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    // The normal update preparation path below will report the not-found error.
                }
            }
            update_requests.push(update_request);
        }

        let mut valid_delete_ids_seen: HashSet<String> = HashSet::new();
        let mut delete_requests: Vec<String> = Vec::new();
        for delete_id in request.delete_ids {
            if self.activity_repository.get_activity(&delete_id).is_ok() {
                if let Some(pair) = self.load_internal_transfer_pair_for_activity(&delete_id)? {
                    for pair_delete_id in [pair.transfer_out.id, pair.transfer_in.id] {
                        if valid_delete_ids_seen.insert(pair_delete_id.clone()) {
                            delete_requests.push(pair_delete_id);
                        }
                    }
                } else if valid_delete_ids_seen.insert(delete_id.clone()) {
                    delete_requests.push(delete_id);
                }
            } else if valid_delete_ids_seen.insert(delete_id.clone()) {
                delete_requests.push(delete_id);
            }
        }

        if !errors.is_empty() {
            return Ok(ActivityBulkMutationResult {
                errors,
                ..Default::default()
            });
        }

        // Use save preparation for the creates, one account at a time.
        //
        // Save preparation is account-scoped: it enforces the per-account-type
        // activity rules and resolves symbols and currencies against the
        // account currency. A bulk request may span accounts, so preparing
        // every create against a single account would judge rows by an account
        // they do not belong to (e.g. rejecting a bank DEPOSIT because an
        // unrelated credit-card row happened to be listed first).
        if !request.creates.is_empty() {
            // Store temp_ids for error reporting (prepare result uses indices)
            let temp_ids: Vec<Option<String>> =
                request.creates.iter().map(|a| a.id.clone()).collect();

            // Group creates by account, keeping each row's index in the
            // original request so errors and prepared rows can be reported
            // back in request order.
            let mut grouped: Vec<(String, Vec<(usize, NewActivity)>)> = Vec::new();
            let mut group_positions: HashMap<String, usize> = HashMap::new();
            for (idx, activity) in request.creates.into_iter().enumerate() {
                let account_id = activity.account_id.clone();
                let position = match group_positions.get(&account_id) {
                    Some(position) => *position,
                    None => {
                        group_positions.insert(account_id.clone(), grouped.len());
                        grouped.push((account_id, Vec::new()));
                        grouped.len() - 1
                    }
                };
                grouped[position].1.push((idx, activity));
            }

            let mut create_errors: Vec<(usize, ActivityBulkMutationError)> = Vec::new();
            let mut ordered_creates: Vec<Option<NewActivity>> = vec![None; temp_ids.len()];

            for (account_id, entries) in grouped {
                let account = self.account_service.get_account(&account_id)?;
                let indexes: Vec<usize> = entries.iter().map(|(idx, _)| *idx).collect();
                let account_creates: Vec<NewActivity> =
                    entries.into_iter().map(|(_, activity)| activity).collect();

                let prepare_result = self
                    .prepare_activities_for_save(account_creates, &account)
                    .await?;

                // Convert preparation errors to bulk mutation errors. Preparation
                // reports indices within the group it was handed, so map each one
                // back to its position in the original request.
                let failed_offsets: HashSet<usize> = prepare_result
                    .errors
                    .iter()
                    .map(|(offset, _)| *offset)
                    .collect();
                for (offset, error) in prepare_result.errors {
                    let idx = indexes.get(offset).copied().unwrap_or(offset);
                    create_errors.push((
                        idx,
                        ActivityBulkMutationError {
                            id: temp_ids.get(idx).cloned().flatten(),
                            action: "create".to_string(),
                            message: error,
                        },
                    ));
                }

                // Preparation yields one prepared row per input row that did not
                // error, in input order, so walking the surviving offsets puts
                // each row back at its original request position.
                let mut prepared = prepare_result.prepared.into_iter();
                for (offset, idx) in indexes.into_iter().enumerate() {
                    if failed_offsets.contains(&offset) {
                        continue;
                    }
                    match prepared.next() {
                        Some(prepared_activity) => {
                            ordered_creates[idx] = Some(prepared_activity.activity);
                        }
                        None => break,
                    }
                }
            }

            // Report errors and prepared activities in request order, so the
            // result does not depend on how the creates grouped by account.
            create_errors.sort_by_key(|(idx, _)| *idx);
            errors.extend(create_errors.into_iter().map(|(_, error)| error));

            prepared_creates = ordered_creates.into_iter().flatten().collect();
        }

        // For updates: capture OLD values before preparing the update
        for update_request in update_requests {
            let mut update_request = update_request;
            let target_id = update_request.id.clone();
            // Get the existing activity to capture old account_id and asset_id
            match self.activity_repository.get_activity(&target_id) {
                Ok(existing) => {
                    old_account_ids.insert(existing.account_id.clone());
                    if let Some(ref asset_id) = existing.asset_id {
                        old_asset_ids.insert(asset_id.clone());
                    }
                    old_currencies.insert(existing.currency.clone());
                    old_activity_dates.push(existing.activity_date);
                    old_activities.push(existing.clone());
                    if let Err(err) = self.hydrate_and_validate_update_against_existing(
                        &mut update_request,
                        &existing,
                    ) {
                        errors.push(ActivityBulkMutationError {
                            id: Some(target_id),
                            action: "update".to_string(),
                            message: err.to_string(),
                        });
                        continue;
                    }
                }
                Err(_) => {
                    // Activity doesn't exist - will fail during prepare_update_activity
                }
            }
            match self.prepare_update_activity(update_request).await {
                Ok(prepared) => prepared_updates.push(prepared),
                Err(err) => {
                    errors.push(ActivityBulkMutationError {
                        id: Some(target_id),
                        action: "update".to_string(),
                        message: err.to_string(),
                    });
                }
            }
        }

        // For deletes: capture OLD values before deletion
        for delete_id in delete_requests {
            match self.activity_repository.get_activity(&delete_id) {
                Ok(existing) => {
                    // Capture old values for event emission
                    old_account_ids.insert(existing.account_id.clone());
                    if let Some(ref asset_id) = existing.asset_id {
                        old_asset_ids.insert(asset_id.clone());
                    }
                    old_currencies.insert(existing.currency.clone());
                    old_activity_dates.push(existing.activity_date);
                    old_activities.push(existing.clone());
                    valid_delete_ids.push(delete_id.clone());
                }
                Err(err) => {
                    errors.push(ActivityBulkMutationError {
                        id: Some(delete_id),
                        action: "delete".to_string(),
                        message: err.to_string(),
                    });
                }
            }
        }

        if !errors.is_empty() {
            let outcome = ActivityBulkMutationResult {
                errors,
                ..Default::default()
            };
            return Ok(outcome);
        }

        let mut persisted = self
            .activity_repository
            .bulk_mutate_activities(prepared_creates, prepared_updates, valid_delete_ids)
            .await
            .map_err(Self::map_duplicate_idempotency_violation)?;

        persisted.errors = errors;

        // Emit ONE aggregated domain event for all mutations
        // Start with OLD values captured before updates/deletes (to recalculate old locations)
        let mut account_ids_set: HashSet<String> = old_account_ids;
        let mut asset_ids_set: HashSet<String> = old_asset_ids;
        let mut currencies_set: HashSet<String> = old_currencies;

        // Add NEW values from created and updated activities
        for activity in &persisted.created {
            account_ids_set.insert(activity.account_id.clone());
            if let Some(ref asset_id) = activity.asset_id {
                asset_ids_set.insert(asset_id.clone());
            }
            currencies_set.insert(activity.currency.clone());
        }
        for activity in &persisted.updated {
            account_ids_set.insert(activity.account_id.clone());
            if let Some(ref asset_id) = activity.asset_id {
                asset_ids_set.insert(asset_id.clone());
            }
            currencies_set.insert(activity.currency.clone());
        }
        for activity in &persisted.deleted {
            account_ids_set.insert(activity.account_id.clone());
            if let Some(ref asset_id) = activity.asset_id {
                asset_ids_set.insert(asset_id.clone());
            }
            currencies_set.insert(activity.currency.clone());
        }

        // Only emit if there were actual changes
        if !account_ids_set.is_empty() {
            let account_ids: Vec<String> = account_ids_set.into_iter().collect();
            let asset_ids: Vec<String> = asset_ids_set.into_iter().collect();
            let currencies: Vec<String> = currencies_set.into_iter().collect();
            let earliest_activity_at_utc = old_activity_dates
                .into_iter()
                .chain(Self::earliest_activity_at_utc(
                    persisted
                        .created
                        .iter()
                        .chain(persisted.updated.iter())
                        .chain(persisted.deleted.iter()),
                ))
                .min();
            self.emit_activities_changed(
                account_ids,
                asset_ids,
                currencies,
                earliest_activity_at_utc,
            );
        }
        self.emit_asset_split_activities_changed(
            old_activities
                .iter()
                .chain(persisted.created.iter())
                .chain(persisted.updated.iter())
                .chain(persisted.deleted.iter()),
        );

        Ok(persisted)
    }

    /// Gets the first activity date for given account IDs
    fn get_first_activity_date(
        &self,
        account_ids: Option<&[String]>,
    ) -> Result<Option<chrono::DateTime<Utc>>> {
        self.activity_repository
            .get_first_activity_date(account_ids)
    }

    /// Checks for existing activities with the given idempotency keys.
    ///
    /// Returns a map of {idempotency_key: existing_activity_id} for keys that already exist.
    fn check_existing_duplicates(
        &self,
        idempotency_keys: Vec<String>,
    ) -> Result<HashMap<String, String>> {
        self.activity_repository
            .check_existing_duplicates(&idempotency_keys)
    }

    async fn prepare_activities_for_save(
        &self,
        activities: Vec<NewActivity>,
        account: &Account,
    ) -> Result<PrepareActivitiesResult> {
        self.prepare_activities_internal(activities, account).await
    }
}

// Private helper methods for ActivityService
impl ActivityService {
    async fn prepare_activities_internal(
        &self,
        activities: Vec<NewActivity>,
        account: &Account,
    ) -> Result<PrepareActivitiesResult> {
        use crate::assets::AssetSpec;

        if activities.is_empty() {
            return Ok(PrepareActivitiesResult::default());
        }

        let activities: Vec<NewActivity> = activities
            .into_iter()
            .map(|activity| {
                let mut activity = Self::normalize_activity_for_preparation(activity);
                let date = self.validate_and_normalize_activity_date(&activity.activity_date);
                if let Ok(date) = date {
                    activity.activity_date = date;
                }
                activity
            })
            .collect();

        let mut result = PrepareActivitiesResult::default();
        let base_ccy = self.account_service.get_base_currency().unwrap_or_default();
        let account_currency = resolve_currency(&[&account.currency, &base_ccy]);

        // 2. Build AssetSpecs for each activity
        let mut asset_specs: Vec<AssetSpec> = Vec::new();
        let mut activity_asset_map: Vec<Option<String>> = Vec::with_capacity(activities.len());

        for (idx, activity) in activities.iter().enumerate() {
            if let Err(e) = Self::validate_activity_allowed_for_account(
                &activity.activity_type,
                activity.get_symbol_id().or(activity.get_symbol_code()),
                account,
            ) {
                result.errors.push((idx, e.to_string()));
                activity_asset_map.push(None);
                continue;
            }

            if let Err(e) = activity.validate() {
                result.errors.push((idx, e.to_string()));
                activity_asset_map.push(None);
                continue;
            }

            match self.build_asset_spec(activity, account).await {
                Ok(Some(spec)) => {
                    // Use spec.id if available, or instrument_key for mapping
                    let map_key = spec.id.clone().or_else(|| spec.instrument_key());
                    activity_asset_map.push(map_key);
                    asset_specs.push(spec);
                }
                Ok(None) => {
                    // Cash activities have no asset
                    activity_asset_map.push(None);
                }
                Err(e) => {
                    result.errors.push((idx, e.to_string()));
                    activity_asset_map.push(None);
                }
            }
        }

        // 3. Deduplicate specs and call ensure_assets()
        let unique_specs: Vec<AssetSpec> = asset_specs
            .into_iter()
            .fold(HashMap::new(), |mut map, spec| {
                let key = spec
                    .id
                    .clone()
                    .unwrap_or_else(|| spec.instrument_key().unwrap_or_default());
                map.entry(key).or_insert(spec);
                map
            })
            .into_values()
            .collect();

        let ensure_result = self
            .asset_service
            .ensure_assets(unique_specs, self.activity_repository.as_ref())
            .await?;

        result.assets_created = ensure_result.created_ids.len() as u32;
        result.created_asset_ids = ensure_result.created_ids.clone();

        // Resolve activity_asset_map entries: replace instrument_key refs with actual asset IDs
        for entry in &mut activity_asset_map {
            if let Some(ref map_key) = entry {
                if let Some(asset_id) = ensure_result.input_to_asset_id.get(map_key) {
                    *entry = Some(asset_id.clone());
                } else {
                    warn!(
                        "Could not resolve asset for key '{}'; activity will have no linked asset",
                        map_key
                    );
                    *entry = None;
                }
            }
        }

        // 4. Collect FX pairs and call ensure_fx_pairs()
        // Include both activity currency and asset currency pairs
        let mut fx_pairs: Vec<(String, String)> = Vec::new();

        for (idx, a) in activities.iter().enumerate() {
            let activity_currency = if !a.currency.is_empty() {
                a.currency.clone()
            } else if let Some(asset_id) = activity_asset_map.get(idx).and_then(|id| id.as_ref()) {
                ensure_result
                    .assets
                    .get(asset_id)
                    .map(|asset| asset.quote_ccy.clone())
                    .unwrap_or_else(|| account_currency.clone())
            } else {
                account_currency.clone()
            };

            // Activity currency vs account currency
            if activity_currency != account_currency {
                fx_pairs.push((activity_currency.clone(), account_currency.clone()));
            }

            // Asset currency vs account currency (when asset currency differs from both)
            if let Some(asset_id) = activity_asset_map.get(idx).and_then(|id| id.as_ref()) {
                if let Some(asset) = ensure_result.assets.get(asset_id) {
                    if asset.quote_ccy != account_currency && asset.quote_ccy != activity_currency {
                        fx_pairs.push((asset.quote_ccy.clone(), account_currency.clone()));
                    }
                }
            }
        }

        self.fx_service.ensure_fx_pairs(fx_pairs).await?;

        // 5. Build PreparedActivity for each valid activity
        for (idx, mut activity) in activities.into_iter().enumerate() {
            // Skip if we already recorded an error for this index
            if result.errors.iter().any(|(i, _)| *i == idx) {
                continue;
            }

            let resolved_asset_id = activity_asset_map.get(idx).cloned().flatten();

            // Determine FX pair needed
            let activity_currency = if !activity.currency.is_empty() {
                activity.currency.clone()
            } else if let Some(asset_id) = resolved_asset_id.as_ref() {
                ensure_result
                    .assets
                    .get(asset_id)
                    .map(|asset| asset.quote_ccy.clone())
                    .unwrap_or_else(|| account_currency.clone())
            } else {
                account_currency.clone()
            };
            let fx_pair = if activity_currency != account_currency {
                Some((activity_currency.clone(), account_currency.clone()))
            } else {
                None
            };

            // Validate the activity
            if let Err(e) = activity.validate() {
                result.errors.push((idx, e.to_string()));
                continue;
            }

            // Update activity's asset with resolved asset_id
            if let Some(ref asset_id) = resolved_asset_id {
                match activity.asset.as_mut() {
                    Some(asset) => asset.id = Some(asset_id.clone()),
                    None => {
                        activity.asset = Some(AssetResolutionInput {
                            id: Some(asset_id.clone()),
                            ..Default::default()
                        });
                    }
                }
            }

            // 6. Create a quote from the activity price as a fallback, but only
            // for MANUAL-mode assets. For MARKET-mode assets the unit price is
            // a cost input, not a market price; writing it as BROKER would
            // misattribute user input as broker-sourced (BROKER is reserved
            // for connected-account activity rows) and can shadow provider quotes.
            if PRICE_BEARING_ACTIVITY_TYPES.contains(&activity.activity_type.as_str()) {
                if let Some(ref asset_id) = resolved_asset_id {
                    if let Some(unit_price) = activity.unit_price {
                        let is_manual_mode = ensure_result
                            .assets
                            .get(asset_id)
                            .is_some_and(|a| a.quote_mode == QuoteMode::Manual);
                        if is_manual_mode {
                            let currency = if !activity.currency.is_empty() {
                                &activity.currency
                            } else {
                                &account_currency
                            };
                            self.create_quote_from_activity(
                                asset_id,
                                unit_price,
                                currency,
                                &activity.activity_date,
                                DATA_SOURCE_MANUAL.to_string(),
                            )
                            .await?;
                        }
                    }
                }
            }

            // Ensure currency is set for cash activities or missing currency
            if activity.currency.is_empty() {
                if let Some(asset_id) = resolved_asset_id.as_ref() {
                    if let Some(asset) = ensure_result.assets.get(asset_id) {
                        activity.currency = asset.quote_ccy.clone();
                    } else {
                        activity.currency = account_currency.clone();
                    }
                } else {
                    activity.currency = account_currency.clone();
                }
            }

            // Normalize amounts to absolute values (direction is determined by activity type)
            activity.quantity = activity.quantity.map(|v| v.abs());
            activity.unit_price = activity.unit_price.map(|v| v.abs());
            activity.amount = activity.amount.map(|v| v.abs());
            activity.fee = activity.fee.map(|v| v.abs());
            activity.tax = activity.tax.map(|v| v.abs());

            Self::validate_split_ratio(&activity.activity_type, activity.amount)?;

            // Normalize minor currency units (e.g., GBp -> GBP) and convert amounts
            if get_normalization_rule(&activity.currency).is_some() {
                let input_currency = activity.currency.clone();
                if let Some(unit_price) = activity.unit_price {
                    let (normalized_price, _) = normalize_amount(unit_price, &input_currency);
                    activity.unit_price = Some(normalized_price);
                }
                if let Some(amount) = activity.amount {
                    let (normalized_amount, _) = normalize_amount(amount, &input_currency);
                    activity.amount = Some(normalized_amount);
                }
                if let Some(fee) = activity.fee {
                    let (normalized_fee, normalized_currency) =
                        normalize_amount(fee, &input_currency);
                    activity.fee = Some(normalized_fee);
                    activity.currency = normalized_currency.to_string();
                }
                if let Some(tax) = activity.tax {
                    let (normalized_tax, normalized_currency) =
                        normalize_amount(tax, &input_currency);
                    activity.tax = Some(normalized_tax);
                    activity.currency = normalized_currency.to_string();
                }
                if activity.fee.is_none() && activity.tax.is_none() {
                    let (_, normalized_currency) = normalize_amount(Decimal::ZERO, &input_currency);
                    activity.currency = normalized_currency.to_string();
                }
            }

            let prepared_asset = resolved_asset_id
                .as_ref()
                .and_then(|asset_id| ensure_result.assets.get(asset_id));
            // The asset owns the contract multiplier; `ensure_assets` above
            // has already applied any creation-time seed from this row.
            let unit_multiplier = prepared_asset
                .map(|asset| asset.contract_multiplier())
                .unwrap_or(Decimal::ONE);
            Self::ensure_new_activity_final_amount(
                &mut activity,
                resolved_asset_id.as_deref(),
                unit_multiplier,
                true,
            );
            if let Err(error) =
                Self::validate_new_activity_final_amount(&activity, resolved_asset_id.as_deref())
            {
                result.errors.push((idx, error.to_string()));
                continue;
            }

            let explicit_idempotency_key = activity
                .idempotency_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);

            if let Some(key) = explicit_idempotency_key {
                activity.idempotency_key = Some(key);
            } else if let Ok(date) = DateTime::parse_from_rfc3339(&activity.activity_date)
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|_| {
                    NaiveDate::parse_from_str(&activity.activity_date, "%Y-%m-%d")
                        .map(|d| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default()))
                })
            {
                let key = compute_idempotency_key(
                    &activity.account_id,
                    &activity.activity_type,
                    &date,
                    activity.get_symbol_id(),
                    activity.quantity,
                    activity.unit_price,
                    activity.amount,
                    activity.fee,
                    &activity.currency,
                    activity.source_record_id.as_deref(),
                    activity.notes.as_deref(),
                );
                activity.idempotency_key = Some(key);
            }

            result.prepared.push(PreparedActivity {
                activity,
                resolved_asset_id,
                fx_pair,
            });
        }

        Ok(result)
    }
}

#[cfg(test)]
mod securities_transfer_tests {
    use super::is_securities_transfer;

    #[test]
    fn transfer_with_security_asset_is_securities() {
        assert!(is_securities_transfer("TRANSFER_IN", Some("AAPL")));
        assert!(is_securities_transfer("TRANSFER_OUT", Some("FWIA")));
    }

    #[test]
    fn transfer_with_cash_asset_is_not_securities() {
        assert!(!is_securities_transfer("TRANSFER_IN", Some("CASH:USD")));
        assert!(!is_securities_transfer("TRANSFER_OUT", Some("$CASH-EUR")));
        assert!(!is_securities_transfer("TRANSFER_IN", Some("CASH-GBP")));
    }

    #[test]
    fn transfer_without_resolved_asset_is_not_securities() {
        assert!(!is_securities_transfer("TRANSFER_IN", None));
    }

    #[test]
    fn non_transfer_types_are_not_securities_transfers() {
        assert!(!is_securities_transfer("BUY", Some("AAPL")));
        assert!(!is_securities_transfer("DEPOSIT", Some("CASH:USD")));
    }
}
