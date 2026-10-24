use log::{debug, error, info, warn};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::events::{DomainEvent, DomainEventSink, NoOpDomainEventSink};
use crate::fx::normalize_amount;
use crate::quotes::QuoteServiceTrait;
use crate::taxonomies::TaxonomyServiceTrait;
use crate::utils::isin::looks_like_isin;
use futures::stream::{self, StreamExt};

use super::assets_model::{
    canonicalize_market_identity, normalize_quote_ccy_code, resolve_quote_ccy_precedence, Asset,
    AssetKind, AssetProfile, AssetSpec, EnsureAssetsResult, InstrumentType, NewAsset,
    QuoteCcyResolutionSource, QuoteMode, UpdateAssetProfile,
};
use super::assets_traits::{AssetRepositoryTrait, AssetServiceTrait};
use super::auto_classification::{
    AutoClassificationService, ClassificationInput, ProviderProfileClassification,
};
use super::bond_identity::{resolve_bond_aliases, BondAliasResolution, BondIdentityClaims};
use crate::errors::{DatabaseError, Error, Result};

// Import mic_to_currency for resolving exchange trading currencies
use wealthfolio_market_data::{
    mic_to_currency, InstrumentId as MarketInstrumentId, ProviderId, ProviderInstrument,
    QuoteContext, ResolutionSource, ResolverChain, SymbolResolver,
};

/// Converts a provider's asset_type string to our InstrumentType enum.
/// Provider data uses various naming conventions (e.g., "CRYPTOCURRENCY", "ETF", "Equity").
/// Returns None if the string doesn't map to a known type (caller decides fallback).
fn parse_instrument_type_from_provider(asset_type: &str) -> Option<InstrumentType> {
    match asset_type.to_uppercase().as_str() {
        "CRYPTOCURRENCY" | "CRYPTO" => Some(InstrumentType::Crypto),
        "EQUITY" | "STOCK" | "ETF" | "MUTUALFUND" | "MUTUAL FUND" | "INDEX" => {
            Some(InstrumentType::Equity)
        }
        "CURRENCY" | "FOREX" | "FX" => Some(InstrumentType::Fx),
        "OPTION" => Some(InstrumentType::Option),
        "COMMODITY" => Some(InstrumentType::Metal),
        _ => None,
    }
}

fn normalized_lookup_key(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_uppercase())
}

/// Fill missing bond fields without overwriting saved terms. A conflicting set
/// must not acquire a Treasury type that would certify it for calculated pricing.
fn merge_bond_profile(
    symbol: Option<&str>,
    metadata: &mut Option<serde_json::Value>,
    profile: &super::assets_model::BondSpec,
) -> Result<()> {
    let saved_claims = BondIdentityClaims::new(symbol, metadata.as_ref());
    let profile_claims = BondIdentityClaims::new(profile.isin.as_deref(), None);
    if profile_claims.isins.is_empty()
        || saved_claims.unique_group().is_none()
        || saved_claims.unique_group() != profile_claims.unique_group()
        || (!saved_claims.isins.is_empty() && saved_claims.isins != profile_claims.isins)
    {
        return Err(Error::Asset(
            "Bond profile identity does not match the asset".into(),
        ));
    }
    let root = metadata.get_or_insert_with(|| serde_json::json!({}));
    if root.is_null() {
        *root = serde_json::json!({});
    }
    let root = root
        .as_object_mut()
        .ok_or_else(|| Error::Asset("Invalid asset metadata object".into()))?;
    let bond = root.entry("bond").or_insert_with(|| serde_json::json!({}));
    if bond.is_null() {
        *bond = serde_json::json!({});
    }
    let saved: super::assets_model::BondSpec = serde_json::from_value(bond.clone())
        .map_err(|_| Error::Asset("Invalid bond metadata object".into()))?;
    let compatible = saved
        .coupon_rate
        .is_none_or(|v| Some(v) == profile.coupon_rate)
        && saved
            .maturity_date
            .is_none_or(|v| Some(v) == profile.maturity_date)
        && saved
            .face_value
            .is_none_or(|v| Some(v) == profile.face_value)
        && saved
            .coupon_frequency
            .as_ref()
            .is_none_or(|v| Some(v) == profile.coupon_frequency.as_ref());
    let values = serde_json::to_value(profile).map_err(|e| Error::Asset(e.to_string()))?;
    let bond = bond
        .as_object_mut()
        .ok_or_else(|| Error::Asset("Invalid bond metadata object".into()))?;
    let values = values
        .as_object()
        .ok_or_else(|| Error::Asset("Invalid bond profile object".into()))?;
    for (key, value) in values {
        if key == "treasuryType" && !compatible {
            continue;
        }
        if !value.is_null() && bond.get(key).is_none_or(|v| v.is_null()) {
            bond.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

/// Service for managing assets
pub struct AssetService {
    quote_service: Arc<dyn QuoteServiceTrait>,
    asset_repository: Arc<dyn AssetRepositoryTrait>,
    taxonomy_service: Option<Arc<dyn TaxonomyServiceTrait>>,
    event_sink: Arc<dyn DomainEventSink>,
}

struct AssetEnrichmentOutcome {
    asset: Asset,
    multiplier_changed: bool,
}

impl AssetService {
    fn normalize_exchange_mic(value: Option<&str>) -> Option<String> {
        value
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_uppercase())
    }

    fn default_quote_mode_for_kind(kind: &AssetKind) -> QuoteMode {
        match kind {
            AssetKind::Investment | AssetKind::Fx => QuoteMode::Market,
            _ => QuoteMode::Manual,
        }
    }

    fn inferred_provider_config(
        quote_mode: QuoteMode,
        instrument_type: Option<&InstrumentType>,
        instrument_symbol: Option<&str>,
        exchange_mic: Option<&str>,
    ) -> Option<serde_json::Value> {
        if quote_mode != QuoteMode::Market {
            return None;
        }

        if matches!(instrument_type, Some(InstrumentType::Equity))
            && exchange_mic
                .is_some_and(|mic| matches!(mic.trim().to_uppercase().as_str(), "XETR" | "XFRA"))
            && instrument_symbol
                .map(str::trim)
                .is_some_and(looks_like_isin)
        {
            return Some(serde_json::json!({ "preferred_provider": "BOERSE_FRANKFURT" }));
        }

        None
    }

    fn provider_instrument_from_symbol(
        instrument_type: Option<&InstrumentType>,
        provider_symbol: &str,
        quote_ccy: Option<&str>,
    ) -> Option<ProviderInstrument> {
        let symbol = Arc::from(provider_symbol);
        match instrument_type {
            Some(InstrumentType::Equity | InstrumentType::Option) | None => {
                Some(ProviderInstrument::EquitySymbol { symbol })
            }
            Some(InstrumentType::Crypto) => Some(ProviderInstrument::CryptoSymbol { symbol }),
            Some(InstrumentType::Fx) => Some(ProviderInstrument::FxSymbol { symbol }),
            Some(InstrumentType::Metal) => Some(ProviderInstrument::MetalSymbol {
                symbol,
                quote: Cow::Owned(quote_ccy.unwrap_or("USD").to_string()),
            }),
            Some(InstrumentType::Bond) => Some(ProviderInstrument::BondIsin { isin: symbol }),
        }
    }

    fn instrument_id_from_identity(
        instrument_type: &InstrumentType,
        symbol: &str,
        exchange_mic: Option<&str>,
        quote_ccy: Option<&str>,
    ) -> Option<MarketInstrumentId> {
        match instrument_type {
            InstrumentType::Equity => Some(MarketInstrumentId::Equity {
                ticker: Arc::from(symbol),
                mic: exchange_mic.map(|mic| Cow::Owned(mic.to_string())),
            }),
            InstrumentType::Crypto => Some(MarketInstrumentId::Crypto {
                base: Arc::from(symbol),
                quote: Cow::Owned(quote_ccy?.to_string()),
            }),
            InstrumentType::Fx => Some(MarketInstrumentId::Fx {
                base: Cow::Owned(symbol.to_string()),
                quote: Cow::Owned(quote_ccy?.to_string()),
            }),
            InstrumentType::Metal => Some(MarketInstrumentId::Metal {
                code: Arc::from(symbol),
                quote: Cow::Owned(quote_ccy.unwrap_or("USD").to_string()),
            }),
            InstrumentType::Option => Some(MarketInstrumentId::Option {
                occ_symbol: Arc::from(symbol),
            }),
            InstrumentType::Bond => Some(MarketInstrumentId::Bond {
                isin: Arc::from(symbol),
            }),
        }
    }

    fn provider_config_for_resolution(
        provider_id: Option<&str>,
        provider_symbol: Option<&str>,
        instrument_type: Option<&InstrumentType>,
        canonical_symbol: Option<&str>,
        exchange_mic: Option<&str>,
        quote_ccy: Option<&str>,
    ) -> Option<serde_json::Value> {
        let provider = provider_id
            .map(str::trim)
            .filter(|provider| !provider.is_empty())?;
        let mut config = serde_json::json!({ "preferred_provider": provider });
        let default_yahoo = provider.eq_ignore_ascii_case("YAHOO");

        let (Some(provider_symbol), Some(instrument_type), Some(canonical_symbol)) =
            (provider_symbol, instrument_type, canonical_symbol)
        else {
            return (!default_yahoo).then_some(config);
        };

        let Some(instrument) = Self::instrument_id_from_identity(
            instrument_type,
            canonical_symbol,
            exchange_mic,
            quote_ccy,
        ) else {
            return (!default_yahoo).then_some(config);
        };

        let context = QuoteContext {
            instrument,
            identifiers: Default::default(),
            overrides: None,
            currency_hint: quote_ccy.map(|ccy| Cow::Owned(ccy.to_string())),
            preferred_provider: Some(Cow::Owned(provider.to_string())),
            bond_metadata: None,
            custom_provider_code: None,
        };
        let provider_id: ProviderId = Cow::Owned(provider.to_string());
        let deterministic = ResolverChain::new()
            .resolve(&provider_id, &context)
            .ok()
            .filter(|resolved| resolved.source != ResolutionSource::RulesFallback)
            .map(|resolved| resolved.instrument.to_symbol_string());

        if deterministic
            .as_deref()
            .is_some_and(|symbol| symbol.eq_ignore_ascii_case(provider_symbol))
        {
            return (!default_yahoo).then_some(config);
        }

        if let Some(provider_instrument) =
            Self::provider_instrument_from_symbol(Some(instrument_type), provider_symbol, quote_ccy)
        {
            if let Some(obj) = config.as_object_mut() {
                obj.insert(
                    "overrides".to_string(),
                    serde_json::json!({ provider: provider_instrument }),
                );
            }
            return Some(config);
        }

        (!default_yahoo).then_some(config)
    }

    #[allow(clippy::too_many_arguments)]
    async fn resolve_quote_ccy(
        &self,
        symbol: Option<&str>,
        exchange_mic: Option<&str>,
        instrument_type: Option<&InstrumentType>,
        explicit_quote_ccy: Option<&str>,
        existing_asset_quote_ccy: Option<&str>,
        terminal_fallback: Option<&str>,
        allow_provider_lookup: bool,
    ) -> (String, QuoteCcyResolutionSource) {
        let has_deterministic_precedence = normalize_quote_ccy_code(explicit_quote_ccy).is_some()
            || normalize_quote_ccy_code(existing_asset_quote_ccy).is_some();
        let provider_quote_ccy = if allow_provider_lookup && !has_deterministic_precedence {
            if let Some(sym) = symbol.map(str::trim).filter(|s| !s.is_empty()) {
                self.quote_service
                    .resolve_symbol_quote(sym, exchange_mic, instrument_type, None, None, None)
                    .await
                    .ok()
                    .and_then(|q| q.currency)
            } else {
                None
            }
        } else {
            None
        };

        resolve_quote_ccy_precedence(
            explicit_quote_ccy,
            existing_asset_quote_ccy,
            provider_quote_ccy.as_deref(),
            exchange_mic.and_then(mic_to_currency),
            terminal_fallback,
        )
        .unwrap_or_else(|| {
            (
                terminal_fallback.unwrap_or("USD").to_string(),
                QuoteCcyResolutionSource::TerminalFallback,
            )
        })
    }

    fn should_refresh_market_quote_ccy_on_mic_change(
        quote_mode: QuoteMode,
        payload_quote_ccy: Option<&str>,
        payload_exchange_mic: Option<&str>,
        existing_exchange_mic: Option<&str>,
    ) -> bool {
        quote_mode == QuoteMode::Market
            && payload_quote_ccy.is_none()
            && Self::normalize_exchange_mic(payload_exchange_mic)
                != Self::normalize_exchange_mic(existing_exchange_mic)
    }

    fn expected_market_quote_ccy(
        instrument_type: Option<&InstrumentType>,
        quote_mode: QuoteMode,
        exchange_mic: Option<&str>,
    ) -> Option<String> {
        if quote_mode != QuoteMode::Market {
            return None;
        }

        match instrument_type {
            Some(InstrumentType::Equity | InstrumentType::Option | InstrumentType::Metal) => {
                exchange_mic
                    .and_then(mic_to_currency)
                    .map(|ccy| ccy.to_string())
            }
            _ => None,
        }
    }

    fn metadata_identifier<'a>(
        metadata: Option<&'a serde_json::Value>,
        key: &str,
    ) -> Option<&'a str> {
        metadata
            .and_then(|m| m.get("identifiers"))
            .and_then(|v| v.as_object())
            .and_then(|ids| ids.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Pricing inputs a user can edit on a bond spec. A change here alters every
    /// price the calculated provider would derive, so the provider binding has
    /// to be re-resolved. Note this only re-resolves and refetches the recent
    /// window: older calculated quotes keep the previous specification.
    fn bond_pricing_inputs(
        metadata: Option<&serde_json::Value>,
    ) -> (
        Option<&serde_json::Value>,
        Option<&serde_json::Value>,
        Option<&serde_json::Value>,
    ) {
        let bond = metadata.and_then(|m| m.get("bond"));
        (
            bond.and_then(|b| b.get("maturityDate")),
            bond.and_then(|b| b.get("couponRate")),
            bond.and_then(|b| b.get("couponFrequency")),
        )
    }

    fn should_reset_sync_state_after_profile_change(before: &Asset, after: &Asset) -> bool {
        let is_bond = before.is_bond() || after.is_bond();
        before.quote_mode != after.quote_mode
            || before.quote_ccy != after.quote_ccy
            || before.instrument_type != after.instrument_type
            || before.instrument_symbol != after.instrument_symbol
            || before.instrument_exchange_mic != after.instrument_exchange_mic
            || before.provider_config != after.provider_config
            || (is_bond
                && Self::metadata_identifier(before.metadata.as_ref(), "isin")
                    != Self::metadata_identifier(after.metadata.as_ref(), "isin"))
            || (is_bond
                && Self::bond_pricing_inputs(before.metadata.as_ref())
                    != Self::bond_pricing_inputs(after.metadata.as_ref()))
    }

    /// Creates a new AssetService instance
    pub fn new(
        asset_repository: Arc<dyn AssetRepositoryTrait>,
        quote_service: Arc<dyn QuoteServiceTrait>,
    ) -> Result<Self> {
        Ok(Self {
            quote_service,
            asset_repository,
            taxonomy_service: None,
            event_sink: Arc::new(NoOpDomainEventSink),
        })
    }

    /// Creates a new AssetService instance with taxonomy service for auto-classification
    pub fn with_taxonomy_service(
        asset_repository: Arc<dyn AssetRepositoryTrait>,
        quote_service: Arc<dyn QuoteServiceTrait>,
        taxonomy_service: Arc<dyn TaxonomyServiceTrait>,
    ) -> Result<Self> {
        Ok(Self {
            quote_service,
            asset_repository,
            taxonomy_service: Some(taxonomy_service),
            event_sink: Arc::new(NoOpDomainEventSink),
        })
    }

    /// Sets the domain event sink for this service.
    ///
    /// Events are emitted after successful asset mutations.
    pub fn with_event_sink(mut self, event_sink: Arc<dyn DomainEventSink>) -> Self {
        self.event_sink = event_sink;
        self
    }

    /// Auto-classify a single newly created asset (instrument_type + asset_class).
    async fn classify_new_asset(
        &self,
        asset_id: &str,
        instrument_type: Option<&InstrumentType>,
        kind: &AssetKind,
    ) {
        if let Some(taxonomy_service) = &self.taxonomy_service {
            let classifier = AutoClassificationService::new(Arc::clone(taxonomy_service));
            classifier
                .classify_from_spec(asset_id, instrument_type, kind)
                .await;
        }
    }

    /// Builds a NewAsset from an AssetSpec without any I/O.
    fn new_asset_from_spec(&self, spec: &AssetSpec) -> NewAsset {
        let canonical = canonicalize_market_identity(
            spec.instrument_type.clone(),
            spec.instrument_symbol
                .as_deref()
                .or(spec.display_code.as_deref()),
            spec.instrument_exchange_mic.as_deref(),
            Some(spec.quote_ccy.as_str()),
        );

        let quote_mode = spec
            .quote_mode
            .unwrap_or_else(|| Self::default_quote_mode_for_kind(&spec.kind));

        let resolved_mic = canonical
            .instrument_exchange_mic
            .clone()
            .or(spec.instrument_exchange_mic.clone());
        let fallback_quote_ccy = canonical
            .quote_ccy
            .clone()
            .unwrap_or_else(|| spec.quote_ccy.clone());
        let resolved_quote_ccy = if fallback_quote_ccy.trim().is_empty() {
            Self::expected_market_quote_ccy(
                spec.instrument_type.as_ref(),
                quote_mode,
                resolved_mic.as_deref(),
            )
            .unwrap_or(fallback_quote_ccy)
        } else {
            fallback_quote_ccy
        };

        let provider_config = spec.provider_config.clone().or_else(|| match quote_mode {
            QuoteMode::Market => Self::provider_config_for_resolution(
                spec.provider_id.as_deref(),
                spec.provider_symbol.as_deref(),
                spec.instrument_type.as_ref(),
                canonical
                    .instrument_symbol
                    .as_deref()
                    .or(spec.instrument_symbol.as_deref()),
                resolved_mic.as_deref(),
                Some(resolved_quote_ccy.as_str()),
            )
            .or_else(|| {
                Self::inferred_provider_config(
                    quote_mode,
                    spec.instrument_type.as_ref(),
                    canonical
                        .instrument_symbol
                        .as_deref()
                        .or(spec.instrument_symbol.as_deref()),
                    resolved_mic.as_deref(),
                )
            }),
            QuoteMode::Manual => None,
        });

        let resolved_symbol = canonical
            .instrument_symbol
            .clone()
            .or(spec.instrument_symbol.clone());
        let metadata = spec.metadata.clone().or_else(|| {
            super::build_asset_metadata(
                spec.instrument_type.as_ref(),
                resolved_symbol.as_deref().unwrap_or(""),
            )
        });

        NewAsset {
            id: spec.id.clone(),
            kind: spec.kind.clone(),
            name: spec.name.clone(),
            display_code: canonical.display_code.or(spec.display_code.clone()),
            quote_mode,
            quote_ccy: resolved_quote_ccy,
            instrument_type: spec.instrument_type.clone(),
            instrument_symbol: resolved_symbol,
            instrument_exchange_mic: resolved_mic,
            provider_config,
            provider_id: spec.provider_id.clone(),
            provider_symbol: spec.provider_symbol.clone(),
            is_active: true,
            metadata,
            ..Default::default()
        }
    }
}

impl AssetService {
    async fn retain_bond_isin(&self, asset_id: &str, isin: &str) -> Result<Asset> {
        let asset = self.asset_repository.get_by_id(asset_id)?;
        let stored =
            BondIdentityClaims::new(asset.instrument_symbol.as_deref(), asset.metadata.as_ref());
        let incoming = BondIdentityClaims::new(Some(isin), None);
        if !asset.is_bond()
            || stored.unique_group().is_none()
            || stored.unique_group() != incoming.unique_group()
            || stored.isins.iter().any(|old| old != isin)
            || Self::metadata_identifier(asset.metadata.as_ref(), "isin")
                .is_some_and(|stored_isin| crate::utils::isin::parse_isin(stored_isin).is_ok())
        {
            return Ok(asset);
        }
        let mut metadata = asset
            .metadata
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        let Some(root) = metadata.as_object_mut() else {
            return Ok(asset);
        };
        let identifiers = root
            .entry("identifiers")
            .or_insert_with(|| serde_json::json!({}));
        if identifiers.is_null() {
            *identifiers = serde_json::json!({});
        }
        let Some(identifiers) = identifiers.as_object_mut() else {
            return Ok(asset);
        };
        identifiers.insert("isin".into(), serde_json::json!(isin));
        self.update_asset_profile(
            asset_id,
            UpdateAssetProfile::metadata_only(&asset, metadata),
        )
        .await
    }

    async fn enrich_asset_profile_silent(&self, asset_id: &str) -> Result<AssetEnrichmentOutcome> {
        // Get the existing asset
        let existing_asset = self.asset_repository.get_by_id(asset_id)?;

        // Skip enrichment for assets that don't need market data
        if existing_asset.quote_mode != QuoteMode::Market {
            debug!(
                "Skipping enrichment for asset {} - quote mode is {:?}",
                asset_id, existing_asset.quote_mode
            );
            return Ok(AssetEnrichmentOutcome {
                asset: existing_asset,
                multiplier_changed: false,
            });
        }

        // Fetch profile from provider using the asset (resolver handles exchange suffix)
        debug!(
            "Fetching profile for asset {} (display_code: {:?}, exchange: {:?})",
            asset_id, existing_asset.display_code, existing_asset.instrument_exchange_mic
        );

        let provider_profile = self
            .quote_service
            .get_asset_profile(&existing_asset)
            .await?;
        // User edits can happen during the network request. Merge missing terms into
        // the latest asset, and discard results if its identity or mode changed.
        let latest_asset = self.asset_repository.get_by_id(asset_id)?;
        if latest_asset.instrument_type != existing_asset.instrument_type
            || latest_asset.instrument_symbol != existing_asset.instrument_symbol
            || latest_asset.quote_mode != existing_asset.quote_mode
            || latest_asset.to_instrument_id() != existing_asset.to_instrument_id()
            || latest_asset.provider_config != existing_asset.provider_config
        {
            return Err(Error::Asset(
                "Asset changed during enrichment; retry with its current identity".into(),
            ));
        }
        let existing_asset = latest_asset;
        let old_multiplier = existing_asset.contract_multiplier();

        // Derive instrument_type from provider's asset_type if not already set
        let updated_instrument_type = if existing_asset.instrument_type.is_none() {
            provider_profile
                .asset_type
                .as_ref()
                .and_then(|t| parse_instrument_type_from_provider(t))
        } else {
            None
        };

        // Build provider profile metadata for storage
        let mut profile_metadata = serde_json::Map::new();
        if let Some(ref sectors) = provider_profile.sectors {
            profile_metadata.insert(
                "sectors".to_string(),
                serde_json::Value::String(sectors.clone()),
            );
        }
        if let Some(ref industry) = provider_profile.industry {
            profile_metadata.insert(
                "industry".to_string(),
                serde_json::Value::String(industry.clone()),
            );
        }
        if let Some(ref countries) = provider_profile.countries {
            profile_metadata.insert(
                "countries".to_string(),
                serde_json::Value::String(countries.clone()),
            );
        }
        if let Some(ref asset_type) = provider_profile.asset_type {
            profile_metadata.insert(
                "quoteType".to_string(),
                serde_json::Value::String(asset_type.clone()),
            );
        }
        if let Some(ref url) = provider_profile.url {
            profile_metadata.insert(
                "website".to_string(),
                serde_json::Value::String(url.clone()),
            );
        }
        if let Some(market_cap) = provider_profile.market_cap {
            profile_metadata.insert("marketCap".to_string(), serde_json::json!(market_cap));
        }
        if let Some(pe_ratio) = provider_profile.pe_ratio {
            profile_metadata.insert("peRatio".to_string(), serde_json::json!(pe_ratio));
        }
        if let Some(dividend_yield) = provider_profile.dividend_yield {
            profile_metadata.insert(
                "dividendYield".to_string(),
                serde_json::json!(dividend_yield),
            );
        }
        if let Some(week_52_high) = provider_profile.week_52_high {
            profile_metadata.insert("week52High".to_string(), serde_json::json!(week_52_high));
        }
        if let Some(week_52_low) = provider_profile.week_52_low {
            profile_metadata.insert("week52Low".to_string(), serde_json::json!(week_52_low));
        }

        // Merge with existing metadata (preserving any non-profile fields like OptionSpec)
        let mut updated_metadata = if profile_metadata.is_empty() {
            existing_asset.metadata.clone()
        } else {
            let mut merged = match &existing_asset.metadata {
                Some(existing) => match existing.as_object() {
                    Some(obj) => obj.clone(),
                    None => serde_json::Map::new(),
                },
                None => serde_json::Map::new(),
            };
            merged.insert(
                "profile".to_string(),
                serde_json::Value::Object(profile_metadata),
            );
            Some(serde_json::Value::Object(merged))
        };

        if existing_asset.is_bond() {
            if let Some(bond) = &provider_profile.bond {
                merge_bond_profile(
                    existing_asset.instrument_symbol.as_deref(),
                    &mut updated_metadata,
                    bond,
                )?;
            }
        }

        let effective_instrument_type = updated_instrument_type
            .clone()
            .or(existing_asset.instrument_type.clone());
        let canonical = canonicalize_market_identity(
            effective_instrument_type,
            existing_asset
                .instrument_symbol
                .as_deref()
                .or(existing_asset.display_code.as_deref()),
            existing_asset.instrument_exchange_mic.as_deref(),
            Some(provider_profile.currency.as_str()),
        );
        let resolved_quote_ccy = canonical
            .quote_ccy
            .unwrap_or_else(|| existing_asset.quote_ccy.clone());

        // Build profile update from provider data
        let mut update = UpdateAssetProfile {
            display_code: existing_asset.display_code.clone(),
            name: provider_profile.name.or(existing_asset.name.clone()),
            notes: existing_asset.notes.clone().unwrap_or_default(),
            kind: None,
            quote_mode: Some(existing_asset.quote_mode),
            quote_ccy: Some(resolved_quote_ccy),
            instrument_type: updated_instrument_type,
            instrument_symbol: None,
            instrument_exchange_mic: None,
            provider_config: existing_asset.provider_config.clone(),
            metadata: updated_metadata,
        };

        // Update notes with description if notes is empty and provider has notes
        if update.notes.is_empty() {
            if let Some(ref notes) = provider_profile.notes {
                update.notes = notes.clone();
            }
        }

        debug!(
            "Enriching asset {} with provider profile: instrument_type={:?}, name={:?}, sectors={:?}, industry={:?}, countries={:?}, asset_type={:?}",
            asset_id, update.instrument_type, update.name, provider_profile.sectors, provider_profile.industry, provider_profile.countries, provider_profile.asset_type
        );

        let updated_asset = self
            .asset_repository
            .update_profile(asset_id, update)
            .await?;

        // Auto-classify asset based on provider profile data
        if let Some(taxonomy_service) = &self.taxonomy_service {
            let classification_input =
                ClassificationInput::from_provider_profile(ProviderProfileClassification {
                    quote_type: provider_profile.asset_type.as_deref(),
                    name: updated_asset.name.as_deref(),
                    sectors_json: provider_profile.sectors.as_deref(),
                    classes_json: provider_profile.classes.as_deref(),
                    countries_json: provider_profile.countries.as_deref(),
                    exchange_mic: existing_asset.instrument_exchange_mic.as_deref(),
                    ..Default::default()
                });

            let auto_classifier = AutoClassificationService::new(Arc::clone(taxonomy_service));
            match auto_classifier
                .classify_asset(asset_id, &classification_input)
                .await
            {
                Ok(result) => {
                    info!(
                        "Auto-classified asset {}: type={:?}, sectors={:?}, region={:?}",
                        asset_id, result.security_type, result.sectors, result.region
                    );
                }
                Err(e) => {
                    debug!("Auto-classification failed for {}: {}", asset_id, e);
                }
            }
        }

        let multiplier_changed = old_multiplier != updated_asset.contract_multiplier();
        Ok(AssetEnrichmentOutcome {
            asset: updated_asset,
            multiplier_changed,
        })
    }
}

// Implement the service trait
#[async_trait::async_trait]
impl AssetServiceTrait for AssetService {
    /// Lists all assets with enriched fields (e.g., exchange_name)
    fn get_assets(&self) -> Result<Vec<Asset>> {
        let assets = self.asset_repository.list()?;
        Ok(assets.into_iter().map(|a| a.enrich()).collect())
    }

    /// Retrieves an asset by its ID with enriched fields
    fn get_asset_by_id(&self, asset_id: &str) -> Result<Asset> {
        self.asset_repository
            .get_by_id(asset_id)
            .map(|a| a.enrich())
    }

    fn get_asset_profile(&self, asset_id: &str) -> Result<AssetProfile> {
        let asset = self.get_asset_by_id(asset_id)?;
        let (valuation_market_price, valuation_market_currency) = self
            .quote_service
            .get_latest_quote(asset_id)
            .ok()
            .map(|quote| {
                let (amount, currency) = normalize_amount(quote.close, &quote.currency);
                (Some(amount), Some(currency.to_string()))
            })
            .unwrap_or((None, None));

        Ok(AssetProfile::new(
            asset,
            valuation_market_price,
            valuation_market_currency,
        ))
    }

    async fn delete_asset(&self, asset_id: &str) -> Result<()> {
        // Clean up sync state before deleting the asset to avoid orphaned records
        if let Err(e) = self.quote_service.delete_sync_state(asset_id).await {
            warn!("Failed to delete sync state for {}: {}", asset_id, e);
        }

        self.asset_repository.delete(asset_id).await
    }

    /// Updates an asset profile
    async fn update_asset_profile(
        &self,
        asset_id: &str,
        mut payload: UpdateAssetProfile,
    ) -> Result<Asset> {
        let existing_asset = self.asset_repository.get_by_id(asset_id)?;
        let effective_quote_mode = payload.quote_mode.unwrap_or(existing_asset.quote_mode);

        payload.instrument_exchange_mic = payload
            .instrument_exchange_mic
            .as_deref()
            .and_then(normalized_lookup_key);

        let effective_instrument_type = payload
            .instrument_type
            .clone()
            .or(existing_asset.instrument_type.clone());

        if effective_instrument_type.is_some() {
            let should_refresh_quote_ccy = Self::should_refresh_market_quote_ccy_on_mic_change(
                effective_quote_mode,
                payload.quote_ccy.as_deref(),
                payload.instrument_exchange_mic.as_deref(),
                existing_asset.instrument_exchange_mic.as_deref(),
            );

            let mut canonical = canonicalize_market_identity(
                effective_instrument_type.clone(),
                payload
                    .instrument_symbol
                    .as_deref()
                    .or(payload.display_code.as_deref())
                    .or(existing_asset.instrument_symbol.as_deref())
                    .or(existing_asset.display_code.as_deref()),
                payload
                    .instrument_exchange_mic
                    .as_deref()
                    .or(existing_asset.instrument_exchange_mic.as_deref()),
                if should_refresh_quote_ccy {
                    None
                } else {
                    payload
                        .quote_ccy
                        .as_deref()
                        .or(Some(existing_asset.quote_ccy.as_str()))
                },
            );

            let requested_symbol = payload.instrument_symbol.as_deref().or_else(|| {
                payload.display_code.as_deref().filter(|code| {
                    normalized_lookup_key(code)
                        != existing_asset
                            .display_code
                            .as_deref()
                            .and_then(normalized_lookup_key)
                })
            });
            let symbol_changed = requested_symbol.is_some_and(|symbol| {
                normalized_lookup_key(symbol)
                    != existing_asset
                        .instrument_symbol
                        .as_deref()
                        .or(existing_asset.display_code.as_deref())
                        .and_then(normalized_lookup_key)
            });
            let exchange_changed = payload
                .instrument_exchange_mic
                .as_deref()
                .is_some_and(|mic| {
                    normalized_lookup_key(mic)
                        != existing_asset
                            .instrument_exchange_mic
                            .as_deref()
                            .and_then(normalized_lookup_key)
                });
            let identity_changed = symbol_changed
                || exchange_changed
                || effective_instrument_type != existing_asset.instrument_type
                || (matches!(
                    effective_instrument_type,
                    Some(InstrumentType::Crypto | InstrumentType::Fx)
                ) && payload.quote_ccy.as_deref().is_some_and(|ccy| {
                    normalized_lookup_key(ccy) != normalized_lookup_key(&existing_asset.quote_ccy)
                }));

            if !identity_changed {
                // Name, notes, and provider edits must not silently rekey a
                // legacy asset, including rows the migration skipped on collision.
                canonical.instrument_symbol = existing_asset.instrument_symbol.clone();
                canonical.display_code = payload
                    .display_code
                    .clone()
                    .or(existing_asset.display_code.clone());
                canonical.instrument_exchange_mic = existing_asset.instrument_exchange_mic.clone();
                if matches!(
                    effective_instrument_type,
                    Some(InstrumentType::Crypto | InstrumentType::Fx)
                ) {
                    canonical.quote_ccy = Some(existing_asset.quote_ccy.clone());
                    payload.quote_ccy = Some(existing_asset.quote_ccy.clone());
                }
            }

            payload.instrument_symbol = canonical
                .instrument_symbol
                .or(payload.instrument_symbol.clone());
            payload.display_code = canonical.display_code.or(payload.display_code.clone());
            payload.instrument_exchange_mic = canonical
                .instrument_exchange_mic
                .or(payload.instrument_exchange_mic.clone());
            if effective_quote_mode == QuoteMode::Market {
                payload.quote_ccy = canonical.quote_ccy.or(payload.quote_ccy.clone());
            }

            if identity_changed {
                if let (Some(symbol), Some(instrument_type)) = (
                    payload.instrument_symbol.as_ref(),
                    effective_instrument_type.as_ref(),
                ) {
                    // Match SQLite's generated key from the exact values that
                    // update_profile will store; manual quote currencies are not normalized.
                    let prefix = format!("{}:{}", instrument_type.as_db_str(), symbol);
                    let key = match instrument_type {
                        InstrumentType::Crypto | InstrumentType::Fx => format!(
                            "{}/{}",
                            prefix,
                            payload
                                .quote_ccy
                                .as_deref()
                                .unwrap_or(&existing_asset.quote_ccy)
                        ),
                        _ => match payload
                            .instrument_exchange_mic
                            .as_deref()
                            .or(existing_asset.instrument_exchange_mic.as_deref())
                        {
                            Some(mic) => format!("{prefix}@{mic}"),
                            None => prefix,
                        },
                    };
                    if self
                        .asset_repository
                        .find_by_instrument_key(&key)?
                        .is_some_and(|asset| asset.id != existing_asset.id)
                    {
                        return Err(Error::ConstraintViolation(
                            concat!(
                                "ASSET_IDENTITY_CONFLICT: An asset with this identity already exists. ",
                                "Your changes were not saved."
                            ).to_string(),
                        ));
                    }
                }
            }
        }

        let asset = self
            .asset_repository
            .update_profile(asset_id, payload)
            .await?;

        if Self::should_reset_sync_state_after_profile_change(&existing_asset, &asset) {
            if let Err(err) = self
                .quote_service
                .reset_sync_state_for_profile_change(&asset.id)
                .await
            {
                warn!(
                    "Failed to reset quote sync state after asset profile update for {}: {}",
                    asset.id, err
                );
            }
        }

        self.event_sink
            .emit(DomainEvent::assets_updated(vec![asset.id.clone()]));

        Ok(asset)
    }

    async fn update_asset_metadata(
        &self,
        asset_id: &str,
        metadata: serde_json::Value,
    ) -> Result<Asset> {
        let asset = self
            .asset_repository
            .update_metadata(asset_id, metadata)
            .await?;

        self.event_sink
            .emit(DomainEvent::assets_updated(vec![asset.id.clone()]));

        Ok(asset)
    }

    /// Creates a new asset directly without network lookups.
    async fn create_asset(&self, mut new_asset: NewAsset) -> Result<Asset> {
        let canonical = canonicalize_market_identity(
            new_asset.instrument_type.clone(),
            new_asset
                .instrument_symbol
                .as_deref()
                .or(new_asset.display_code.as_deref()),
            new_asset.instrument_exchange_mic.as_deref(),
            Some(new_asset.quote_ccy.as_str()),
        );
        new_asset.display_code = canonical.display_code.or(new_asset.display_code.clone());
        new_asset.instrument_symbol = canonical
            .instrument_symbol
            .or(new_asset.instrument_symbol.clone());
        new_asset.instrument_exchange_mic = canonical
            .instrument_exchange_mic
            .or(new_asset.instrument_exchange_mic.clone());
        new_asset.quote_ccy = canonical
            .quote_ccy
            .or_else(|| {
                Self::expected_market_quote_ccy(
                    new_asset.instrument_type.as_ref(),
                    new_asset.quote_mode,
                    new_asset.instrument_exchange_mic.as_deref(),
                )
            })
            .unwrap_or(new_asset.quote_ccy);
        if new_asset.provider_config.is_none() {
            new_asset.provider_config = Self::provider_config_for_resolution(
                new_asset.provider_id.as_deref(),
                new_asset.provider_symbol.as_deref(),
                new_asset.instrument_type.as_ref(),
                new_asset.instrument_symbol.as_deref(),
                new_asset.instrument_exchange_mic.as_deref(),
                Some(new_asset.quote_ccy.as_str()),
            )
            .or_else(|| {
                Self::inferred_provider_config(
                    new_asset.quote_mode,
                    new_asset.instrument_type.as_ref(),
                    new_asset.instrument_symbol.as_deref(),
                    new_asset.instrument_exchange_mic.as_deref(),
                )
            });
        }

        // Pre-check: return existing asset if instrument_key already exists (avoids unique constraint error)
        let key_spec = AssetSpec {
            id: None,
            display_code: new_asset.display_code.clone(),
            instrument_symbol: new_asset.instrument_symbol.clone(),
            instrument_exchange_mic: new_asset.instrument_exchange_mic.clone(),
            instrument_type: new_asset.instrument_type.clone(),
            quote_ccy: new_asset.quote_ccy.clone(),
            requested_quote_ccy: None,
            kind: new_asset.kind.clone(),
            quote_mode: Some(new_asset.quote_mode),
            name: new_asset.name.clone(),
            provider_config: None,
            provider_id: None,
            provider_symbol: None,
            metadata: None,
        };
        if let Some(key) = key_spec.instrument_key() {
            if let Ok(Some(existing)) = self.asset_repository.find_by_instrument_key(&key) {
                return Ok(existing);
            }
        }

        let instrument_type = new_asset.instrument_type.clone();
        let kind = new_asset.kind.clone();
        let asset = self.asset_repository.create(new_asset).await?;

        // Auto-classify the newly created asset
        self.classify_new_asset(&asset.id, instrument_type.as_ref(), &kind)
            .await;

        // Emit event for newly created asset
        self.event_sink
            .emit(DomainEvent::assets_created(vec![asset.id.clone()]));

        Ok(asset)
    }

    /// Creates a minimal asset without network calls.
    /// Returns the existing asset if found, or creates a new minimal one.
    async fn get_or_create_minimal_asset(
        &self,
        asset_id: &str,
        context_currency: Option<String>,
        metadata: Option<super::assets_model::AssetMetadata>,
        quote_mode: Option<String>,
    ) -> Result<Asset> {
        let inferred_instrument_type = metadata.as_ref().and_then(|meta| {
            meta.instrument_symbol
                .as_ref()
                .filter(|s| !s.is_empty())
                .or(meta.display_code.as_ref().filter(|s| !s.is_empty()))
                .map(|_| {
                    meta.instrument_type
                        .clone()
                        .unwrap_or(InstrumentType::Equity)
                })
        });
        let requested_quote_mode = match quote_mode.as_deref() {
            Some("MANUAL") => Some(QuoteMode::Manual),
            Some("MARKET") => Some(QuoteMode::Market),
            _ => None,
        };

        // Try to get existing asset first
        match self.asset_repository.get_by_id(asset_id) {
            Ok(mut existing_asset) => {
                // Reactivate if previously deactivated (e.g., after account deletion)
                if !existing_asset.is_active {
                    info!("Reactivating previously deactivated asset: {}", asset_id);
                    self.asset_repository.reactivate(asset_id).await?;
                    existing_asset.is_active = true;
                }

                if inferred_instrument_type == Some(InstrumentType::Bond) {
                    if let Some(meta) = metadata.as_ref() {
                        if let Some(symbol) = meta
                            .instrument_symbol
                            .as_ref()
                            .or(meta.display_code.as_ref())
                        {
                            let mut spec = AssetSpec::market_instrument(
                                symbol.clone(),
                                symbol.clone(),
                                None,
                                InstrumentType::Bond,
                                existing_asset.quote_ccy.clone(),
                            );
                            spec.metadata = meta.asset_metadata.clone();
                            let resolutions = resolve_bond_aliases(
                                std::slice::from_ref(&spec),
                                &self.get_assets()?,
                            );
                            if let Some((matched_id, isin)) = resolutions[0].matched_isin() {
                                if matched_id == asset_id {
                                    return self.retain_bond_isin(asset_id, isin).await;
                                }
                            }
                        }
                    }
                }
                return Ok(existing_asset);
            }
            Err(Error::Database(DatabaseError::NotFound(_))) => {
                debug!(
                    "Asset not found locally, creating minimal asset: {}",
                    asset_id
                );
            }
            Err(e) => {
                error!("Error fetching asset by ID '{}': {}", asset_id, e);
                return Err(e);
            }
        }

        // Try to find existing asset by instrument_key before creating a new one

        if let Some(ref meta) = metadata {
            let canonical = canonicalize_market_identity(
                inferred_instrument_type.clone(),
                meta.instrument_symbol
                    .as_deref()
                    .or(meta.display_code.as_deref()),
                meta.instrument_exchange_mic.as_deref(),
                context_currency.as_deref(),
            );
            if let Some(ref sym) = canonical.instrument_symbol {
                if !sym.is_empty() {
                    let instrument_type = inferred_instrument_type
                        .clone()
                        .unwrap_or(InstrumentType::Equity);
                    let spec = AssetSpec {
                        id: None,
                        display_code: canonical.display_code.or(meta.display_code.clone()),
                        instrument_symbol: Some(sym.clone()),
                        instrument_exchange_mic: canonical
                            .instrument_exchange_mic
                            .or(meta.instrument_exchange_mic.clone()),
                        instrument_type: Some(instrument_type.clone()),
                        quote_ccy: canonical.quote_ccy.unwrap_or_else(|| {
                            context_currency
                                .clone()
                                .unwrap_or_else(|| "USD".to_string())
                        }),
                        requested_quote_ccy: meta.requested_quote_ccy.clone(),
                        kind: meta.kind.clone().unwrap_or(AssetKind::Investment),
                        quote_mode: None,
                        name: meta.name.clone(),
                        provider_config: meta.provider_config.clone(),
                        provider_id: meta.provider_id.clone(),
                        provider_symbol: meta.provider_symbol.clone(),
                        metadata: None,
                    };
                    if let Some(key) = spec.instrument_key() {
                        if let Ok(Some(existing)) =
                            self.asset_repository.find_by_instrument_key(&key)
                        {
                            if !existing.is_active {
                                self.asset_repository.reactivate(&existing.id).await?;
                            }

                            return Ok(existing);
                        }
                    }
                }
            }
        }

        // Use metadata kind if provided, otherwise default to Investment
        let kind = metadata
            .as_ref()
            .and_then(|m| m.kind.clone())
            .unwrap_or(AssetKind::Investment);

        // Determine quote mode: use input if provided, otherwise default based on kind
        let quote_mode =
            requested_quote_mode.unwrap_or_else(|| Self::default_quote_mode_for_kind(&kind));

        // Extract exchange_mic from metadata
        let exchange_mic = metadata
            .as_ref()
            .and_then(|m| m.instrument_exchange_mic.clone());

        let instrument_type = inferred_instrument_type;
        let allow_provider_lookup = quote_mode == QuoteMode::Market
            && !matches!(
                instrument_type.as_ref(),
                Some(InstrumentType::Crypto | InstrumentType::Fx)
            );
        let symbol_for_resolution = metadata
            .as_ref()
            .and_then(|m| m.instrument_symbol.as_deref().or(m.display_code.as_deref()));
        let explicit_requested_quote_ccy = metadata
            .as_ref()
            .and_then(|m| m.requested_quote_ccy.as_deref());
        let (currency, _) = self
            .resolve_quote_ccy(
                symbol_for_resolution,
                exchange_mic.as_deref(),
                instrument_type.as_ref(),
                explicit_requested_quote_ccy,
                None,
                context_currency
                    .as_deref()
                    .filter(|c| !c.trim().is_empty())
                    .or(Some("USD")),
                allow_provider_lookup,
            )
            .await;

        let name = metadata.as_ref().and_then(|m| m.name.clone());
        let asset_metadata_json = metadata.as_ref().and_then(|m| m.asset_metadata.clone());
        let explicit_provider_config = metadata.as_ref().and_then(|m| m.provider_config.clone());
        let provider_id = metadata.as_ref().and_then(|m| m.provider_id.clone());
        let provider_symbol = metadata.as_ref().and_then(|m| m.provider_symbol.clone());
        let canonical_identity = canonicalize_market_identity(
            instrument_type.clone(),
            metadata
                .as_ref()
                .and_then(|m| m.instrument_symbol.as_deref().or(m.display_code.as_deref())),
            exchange_mic.as_deref(),
            Some(currency.as_str()),
        );
        let provider_config = explicit_provider_config.or_else(|| match quote_mode {
            QuoteMode::Market => Self::provider_config_for_resolution(
                provider_id.as_deref(),
                provider_symbol.as_deref(),
                instrument_type.as_ref(),
                canonical_identity.instrument_symbol.as_deref(),
                canonical_identity
                    .instrument_exchange_mic
                    .as_deref()
                    .or(exchange_mic.as_deref()),
                Some(currency.as_str()),
            )
            .or_else(|| {
                Self::inferred_provider_config(
                    quote_mode,
                    instrument_type.as_ref(),
                    canonical_identity.instrument_symbol.as_deref(),
                    canonical_identity
                        .instrument_exchange_mic
                        .as_deref()
                        .or(exchange_mic.as_deref()),
                )
            }),
            QuoteMode::Manual => None,
        });

        let new_asset = NewAsset {
            id: Some(asset_id.to_string()),
            kind,
            name,
            quote_mode,
            quote_ccy: canonical_identity.quote_ccy.unwrap_or(currency),
            instrument_exchange_mic: canonical_identity.instrument_exchange_mic.or(exchange_mic),
            instrument_symbol: canonical_identity.instrument_symbol,
            instrument_type,
            display_code: canonical_identity
                .display_code
                .or_else(|| metadata.as_ref().and_then(|m| m.display_code.clone())),
            provider_config,
            provider_id,
            provider_symbol,
            metadata: asset_metadata_json,
            is_active: true,
            ..Default::default()
        };

        debug!(
            "Creating minimal asset: id={}, kind={:?}, quote_mode={:?}, name={:?}",
            asset_id, new_asset.kind, new_asset.quote_mode, new_asset.name
        );

        let instrument_type = new_asset.instrument_type.clone();
        let kind = new_asset.kind.clone();
        let asset = self.asset_repository.create(new_asset).await?;

        // Auto-classify the newly created asset
        self.classify_new_asset(&asset.id, instrument_type.as_ref(), &kind)
            .await;

        // Emit event for newly created asset
        self.event_sink
            .emit(DomainEvent::assets_created(vec![asset.id.clone()]));

        Ok(asset)
    }

    /// Updates the quote mode for an asset (MARKET, MANUAL)
    async fn update_quote_mode(&self, asset_id: &str, quote_mode: &str) -> Result<Asset> {
        let asset = self.update_quote_mode_silent(asset_id, quote_mode).await?;
        self.event_sink
            .emit(DomainEvent::assets_updated(vec![asset.id.clone()]));
        Ok(asset)
    }

    /// Updates quote mode without emitting domain events.
    /// Switching to Manual means providers will no longer sync this asset,
    /// so clear any stale error state to keep the health panel clean.
    async fn update_quote_mode_silent(&self, asset_id: &str, quote_mode: &str) -> Result<Asset> {
        let asset = self
            .asset_repository
            .update_quote_mode(asset_id, quote_mode)
            .await?;

        if asset.quote_mode == QuoteMode::Manual {
            if let Err(e) = self.quote_service.delete_sync_state(asset_id).await {
                warn!("Failed to clear sync state for {}: {:?}", asset_id, e);
            }
        }

        Ok(asset)
    }

    async fn get_assets_by_asset_ids(&self, asset_ids: &[String]) -> Result<Vec<Asset>> {
        self.asset_repository.list_by_asset_ids(asset_ids)
    }

    /// Enriches an existing asset's profile with data from market data provider.
    /// Updates the profile JSON (sectors, countries, website) and notes fields.
    async fn enrich_asset_profile(&self, asset_id: &str) -> Result<Asset> {
        let outcome = self.enrich_asset_profile_silent(asset_id).await?;
        if outcome.multiplier_changed {
            self.event_sink
                .emit(DomainEvent::assets_updated(vec![outcome.asset.id.clone()]));
        }
        Ok(outcome.asset)
    }

    /// Enriches multiple assets in batch, with deduplication and sync state tracking.
    async fn enrich_assets(&self, asset_ids: Vec<String>) -> Result<(usize, usize, usize)> {
        if asset_ids.is_empty() {
            return Ok((0, 0, 0));
        }

        // Deduplicate
        let unique_ids: Vec<String> = asset_ids
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        if unique_ids.is_empty() {
            debug!("No enrichable assets in batch");
            return Ok((0, 0, 0));
        }

        let unique_ids_len = unique_ids.len();

        // Filter to only assets that need enrichment
        let ids_to_enrich: Vec<String> = unique_ids
            .into_iter()
            .filter(|asset_id| {
                let needs = match self.quote_service.get_sync_state(asset_id) {
                    Ok(Some(state)) => state.needs_profile_enrichment(),
                    Ok(None) => true,
                    Err(_) => true,
                };
                if !needs {
                    debug!("Skipping enrichment for {} - already enriched", asset_id);
                }
                needs
            })
            .collect();

        let skipped_count = unique_ids_len - ids_to_enrich.len();

        // Enrich assets concurrently (up to 5 at a time)
        let results: Vec<(String, Result<AssetEnrichmentOutcome>)> = stream::iter(ids_to_enrich)
            .map(|asset_id| async move {
                let result = self.enrich_asset_profile_silent(&asset_id).await;
                if result.is_ok() {
                    if let Err(e) = self.quote_service.mark_profile_enriched(&asset_id).await {
                        warn!("Failed to mark profile enriched for {}: {}", asset_id, e);
                    }
                }
                (asset_id, result)
            })
            .buffer_unordered(5)
            .collect()
            .await;

        let mut enriched_count = 0;
        let mut failed_count = 0;
        let mut multiplier_changed_asset_ids = Vec::new();
        for (asset_id, result) in &results {
            match result {
                Ok(outcome) => {
                    enriched_count += 1;
                    if outcome.multiplier_changed {
                        multiplier_changed_asset_ids.push(outcome.asset.id.clone());
                    }
                    info!("Successfully enriched asset profile: {}", asset_id);
                }
                Err(e) => {
                    debug!("Failed to enrich asset {}: {}", asset_id, e);
                    failed_count += 1;
                }
            }
        }

        if !multiplier_changed_asset_ids.is_empty() {
            multiplier_changed_asset_ids.sort();
            multiplier_changed_asset_ids.dedup();
            self.event_sink
                .emit(DomainEvent::assets_updated(multiplier_changed_asset_ids));
        }

        Ok((enriched_count, skipped_count, failed_count))
    }

    async fn cleanup_legacy_metadata(&self, asset_id: &str) -> Result<()> {
        self.asset_repository
            .cleanup_legacy_metadata(asset_id)
            .await
    }

    async fn merge_unknown_asset(
        &self,
        resolved_asset_id: &str,
        unknown_asset_id: &str,
        activity_repository: &dyn crate::activities::ActivityRepositoryTrait,
    ) -> Result<u32> {
        info!(
            "Merging UNKNOWN asset {} into resolved asset {}",
            unknown_asset_id, resolved_asset_id
        );

        let (account_ids, currencies) = match activity_repository
            .get_activity_accounts_and_currencies_by_asset_id(unknown_asset_id)
            .await
        {
            Ok(data) => data,
            Err(e) => {
                warn!(
                    "Failed to load account_ids/currencies for UNKNOWN asset {}: {}",
                    unknown_asset_id, e
                );
                (Vec::new(), Vec::new())
            }
        };

        // 1. Copy user metadata (notes) from UNKNOWN to resolved
        if let Err(e) = self
            .asset_repository
            .copy_user_metadata(unknown_asset_id, resolved_asset_id)
            .await
        {
            warn!(
                "Failed to copy user metadata from {} to {}: {}",
                unknown_asset_id, resolved_asset_id, e
            );
        }

        // 2. Reassign all activities from UNKNOWN to resolved
        let activities_migrated = activity_repository
            .reassign_asset(unknown_asset_id, resolved_asset_id)
            .await?;

        // 3. Deactivate the UNKNOWN asset
        if let Err(e) = self.asset_repository.deactivate(unknown_asset_id).await {
            warn!(
                "Failed to deactivate UNKNOWN asset {}: {}",
                unknown_asset_id, e
            );
        }

        // 4. Emit assets_merged domain event
        self.event_sink.emit(DomainEvent::assets_merged(
            unknown_asset_id.to_string(),
            resolved_asset_id.to_string(),
            activities_migrated,
        ));

        // 5. Emit activities_changed to trigger recalculation for affected accounts
        if activities_migrated > 0 {
            let asset_ids = vec![unknown_asset_id.to_string(), resolved_asset_id.to_string()];
            self.event_sink.emit(DomainEvent::activities_changed(
                account_ids,
                asset_ids,
                currencies,
                None,
            ));
        }

        info!(
            "Merged UNKNOWN asset {} into {}: {} activities migrated",
            unknown_asset_id, resolved_asset_id, activities_migrated
        );

        Ok(activities_migrated)
    }

    async fn ensure_assets(
        &self,
        mut specs: Vec<AssetSpec>,
        _activity_repository: &dyn crate::activities::ActivityRepositoryTrait,
    ) -> Result<EnsureAssetsResult> {
        if specs.is_empty() {
            return Ok(EnsureAssetsResult::default());
        }

        let input_keys: Vec<Option<String>> = specs
            .iter()
            .map(|spec| spec.id.clone().or_else(|| spec.instrument_key()))
            .collect();
        let bond_resolutions = if specs
            .iter()
            .any(|spec| spec.instrument_type == Some(InstrumentType::Bond))
        {
            resolve_bond_aliases(&specs, &self.get_assets()?)
        } else {
            vec![BondAliasResolution::default(); specs.len()]
        };
        let matched_bond_isins: HashMap<_, _> = bond_resolutions
            .iter()
            .filter_map(BondAliasResolution::matched_isin)
            .map(|(id, isin)| (id.to_string(), isin.to_string()))
            .collect();
        for (spec, resolution) in specs.iter_mut().zip(&bond_resolutions) {
            resolution.apply_to_spec(spec);
        }
        let input_to_resolved_key: Vec<(Option<String>, Option<String>)> = input_keys
            .into_iter()
            .zip(&specs)
            .map(|(input, spec)| (input, spec.id.clone().or_else(|| spec.instrument_key())))
            .collect();

        // Deduplicate specs by ID (if present) or by instrument_key
        let unique_specs: Vec<AssetSpec> = specs
            .into_iter()
            .zip(&input_to_resolved_key)
            .fold(HashMap::new(), |mut map, (spec, (input_key, _))| {
                let key = spec.id.clone().unwrap_or_else(|| {
                    spec.instrument_key().unwrap_or_else(|| {
                        format!(
                            "{}:{}@{}",
                            spec.instrument_type
                                .as_ref()
                                .map(|t| t.as_db_str())
                                .unwrap_or("?"),
                            spec.instrument_symbol.as_deref().unwrap_or(""),
                            spec.instrument_exchange_mic.as_deref().unwrap_or("")
                        )
                    })
                });
                let is_original_identity = spec.instrument_key().as_ref() == input_key.as_ref();
                match map.entry(key) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert((spec, is_original_identity));
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry)
                        if is_original_identity && !entry.get().1 =>
                    {
                        entry.insert((spec, true));
                    }
                    _ => {}
                }
                map
            })
            .into_values()
            .map(|(spec, _)| spec)
            .collect();

        // Pre-resolve specs without IDs by looking up via instrument_key
        let mut resolved_specs: Vec<AssetSpec> = Vec::with_capacity(unique_specs.len());
        let mut preexisting_keys: HashSet<String> = HashSet::new();
        for mut spec in unique_specs {
            if spec.id.is_none() {
                if let Some(key) = spec.instrument_key() {
                    if let Ok(Some(existing)) = self.asset_repository.find_by_instrument_key(&key) {
                        preexisting_keys.insert(key);
                        spec.id = Some(existing.id);
                    }
                }
            }
            resolved_specs.push(spec);
        }

        // Collect IDs of specs that have them (for existing asset lookup)
        let ids: Vec<String> = resolved_specs.iter().filter_map(|s| s.id.clone()).collect();

        // 1. Pre-read existing assets by requested IDs.
        let existing_ids: HashSet<String> = if !ids.is_empty() {
            self.asset_repository
                .list_by_asset_ids(&ids)?
                .into_iter()
                .map(|a| a.id)
                .collect()
        } else {
            HashSet::new()
        };

        // 2. Batch upsert all specs (INSERT OR IGNORE)
        // Resolve quote currencies with deduped input keys and bounded parallelism.
        const QUOTE_RESOLUTION_CONCURRENCY: usize = 8;
        let build_resolution_key =
            |symbol: Option<&str>,
             exchange_mic: Option<&str>,
             instrument_type: Option<&InstrumentType>,
             explicit_quote_ccy: Option<&str>,
             terminal_fallback: &str,
             allow_provider_lookup: bool| {
                let lookup_flag = if allow_provider_lookup { "1" } else { "0" };
                format!(
                    "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
                    symbol.unwrap_or_default(),
                    exchange_mic.unwrap_or_default(),
                    instrument_type.map(|it| it.as_db_str()).unwrap_or_default(),
                    explicit_quote_ccy.unwrap_or_default(),
                    terminal_fallback,
                    lookup_flag
                )
            };

        type CreateResolutionInput = (
            Option<String>,
            Option<String>,
            Option<InstrumentType>,
            Option<String>,
            String,
            bool,
        );
        let mut specs_for_create: Vec<(AssetSpec, String)> =
            Vec::with_capacity(resolved_specs.len());
        let mut resolution_inputs_by_key: HashMap<String, CreateResolutionInput> = HashMap::new();

        for spec in &resolved_specs {
            let resolved_spec = spec.clone();
            let quote_mode = resolved_spec
                .quote_mode
                .unwrap_or_else(|| Self::default_quote_mode_for_kind(&resolved_spec.kind));
            let allow_provider_lookup = quote_mode == QuoteMode::Market
                && !matches!(
                    resolved_spec.instrument_type.as_ref(),
                    Some(InstrumentType::Crypto | InstrumentType::Fx)
                );
            let symbol = resolved_spec
                .instrument_symbol
                .as_deref()
                .or(resolved_spec.display_code.as_deref());
            let exchange_mic = resolved_spec.instrument_exchange_mic.as_deref();
            let instrument_type = resolved_spec.instrument_type.as_ref();
            let explicit_quote_ccy = resolved_spec.requested_quote_ccy.as_deref();
            let terminal_fallback = resolved_spec.quote_ccy.as_str();
            let resolution_key = build_resolution_key(
                symbol,
                exchange_mic,
                instrument_type,
                explicit_quote_ccy,
                terminal_fallback,
                allow_provider_lookup,
            );

            resolution_inputs_by_key
                .entry(resolution_key.clone())
                .or_insert((
                    symbol.map(|s| s.to_string()),
                    exchange_mic.map(|mic| mic.to_string()),
                    instrument_type.cloned(),
                    explicit_quote_ccy.map(|quote_ccy| quote_ccy.to_string()),
                    terminal_fallback.to_string(),
                    allow_provider_lookup,
                ));
            specs_for_create.push((resolved_spec, resolution_key));
        }

        let resolved_quote_ccy_by_key: HashMap<String, String> =
            stream::iter(resolution_inputs_by_key)
                .map(|(resolution_key, input)| async move {
                    let (
                        symbol,
                        exchange_mic,
                        instrument_type,
                        explicit_quote_ccy,
                        terminal_fallback,
                        allow_provider_lookup,
                    ) = input;
                    let (resolved_quote_ccy, _) = self
                        .resolve_quote_ccy(
                            symbol.as_deref(),
                            exchange_mic.as_deref(),
                            instrument_type.as_ref(),
                            explicit_quote_ccy.as_deref(),
                            None,
                            Some(terminal_fallback.as_str()),
                            allow_provider_lookup,
                        )
                        .await;
                    (resolution_key, resolved_quote_ccy)
                })
                .buffer_unordered(QUOTE_RESOLUTION_CONCURRENCY)
                .collect::<Vec<(String, String)>>()
                .await
                .into_iter()
                .collect();

        for (spec, resolution_key) in &mut specs_for_create {
            if let Some(resolved_quote_ccy) = resolved_quote_ccy_by_key.get(resolution_key) {
                spec.quote_ccy = resolved_quote_ccy.clone();
            }
        }

        let new_assets: Vec<NewAsset> = specs_for_create
            .iter()
            .filter(|(spec, _)| {
                spec.id
                    .as_ref()
                    .map(|id| !existing_ids.contains(id))
                    .unwrap_or(true)
            })
            .map(|(spec, _)| self.new_asset_from_spec(spec))
            .collect();

        self.asset_repository.create_batch(new_assets).await?;

        // Reactivate any pre-existing assets that were deactivated
        for asset in self.asset_repository.list_by_asset_ids(&ids)? {
            if !asset.is_active && existing_ids.contains(&asset.id) {
                info!("Reactivating previously deactivated asset: {}", asset.id);
                self.asset_repository.reactivate(&asset.id).await?;
            }
        }

        // 3. Fetch all requested assets (by ID + by instrument_key for specs without IDs)
        let mut assets_map: HashMap<String, Asset> = if !ids.is_empty() {
            self.asset_repository
                .list_by_asset_ids(&ids)?
                .into_iter()
                .map(|a| (a.id.clone(), a))
                .collect()
        } else {
            HashMap::new()
        };

        // Also look up assets for specs that didn't have IDs (created with DB-generated UUIDs)
        for spec in &resolved_specs {
            if spec.id.is_none() {
                if let Some(key) = spec.instrument_key() {
                    if let Ok(Some(asset)) = self.asset_repository.find_by_instrument_key(&key) {
                        assets_map.insert(asset.id.clone(), asset);
                    }
                }
            }
        }

        // Newly created (ID-based specs): all spec IDs minus pre-existing IDs
        let mut created_ids: HashSet<String> = ids
            .iter()
            .filter(|id| !existing_ids.contains(*id))
            .cloned()
            .collect();

        // Newly created (instrument-key specs with DB-generated UUIDs)
        for spec in &resolved_specs {
            if spec.id.is_none() {
                if let Some(key) = spec.instrument_key() {
                    if preexisting_keys.contains(&key) {
                        continue;
                    }
                    if let Some(asset) = assets_map
                        .values()
                        .find(|a| a.instrument_key.as_deref() == Some(&key))
                    {
                        created_ids.insert(asset.id.clone());
                    }
                }
            }
        }

        let created_ids: Vec<String> = created_ids.into_iter().collect();

        // 4. Auto-classify newly created assets (instrument_type + asset_class)
        if !created_ids.is_empty() {
            let created_set: HashSet<&str> = created_ids.iter().map(|id| id.as_str()).collect();
            for (spec, _) in &specs_for_create {
                let asset_id = spec.id.as_deref().or_else(|| {
                    spec.instrument_key().and_then(|key| {
                        assets_map
                            .values()
                            .find(|a| a.instrument_key.as_deref() == Some(key.as_str()))
                            .map(|a| a.id.as_str())
                    })
                });
                if let Some(id) = asset_id {
                    if created_set.contains(id) {
                        self.classify_new_asset(id, spec.instrument_type.as_ref(), &spec.kind)
                            .await;
                    }
                }
            }
        }

        for (asset_id, isin) in matched_bond_isins {
            let asset = self.retain_bond_isin(&asset_id, &isin).await?;
            assets_map.insert(asset_id, asset);
        }

        // 5. Emit batch event for created assets
        if !created_ids.is_empty() {
            self.event_sink
                .emit(DomainEvent::assets_created(created_ids.clone()));
        }
        let key_to_id: HashMap<String, String> = assets_map
            .values()
            .filter_map(|asset| Some((asset.instrument_key.clone()?, asset.id.clone())))
            .chain(assets_map.keys().map(|id| (id.clone(), id.clone())))
            .collect();
        let input_to_asset_id = input_to_resolved_key
            .into_iter()
            .filter_map(|(input, resolved)| Some((input?, key_to_id.get(&resolved?)?.clone())))
            .collect();

        Ok(EnsureAssetsResult {
            assets: assets_map,
            input_to_asset_id,
            created_ids,
            merge_candidates: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::assets_model::{
        Asset, AssetKind, AssetSpec, InstrumentType, NewAsset, ProviderProfile, UpdateAssetProfile,
    };
    use super::{
        merge_bond_profile, AssetRepositoryTrait, AssetService, AssetServiceTrait, QuoteMode,
    };
    use crate::errors::{DatabaseError, Error, Result};
    use crate::events::{DomainEvent, MockDomainEventSink};
    use crate::quotes::{
        LatestQuotePair, LatestQuoteSnapshot, ProviderInfo, Quote, QuoteServiceTrait,
        QuoteSyncState, SymbolSearchResult, SymbolSyncPlan, SyncMode, SyncResult,
    };
    use chrono::{NaiveDate, Utc};
    use rust_decimal::Decimal;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    fn treasury_test_asset() -> Asset {
        Asset {
            id: "treasury".into(),
            instrument_type: Some(InstrumentType::Bond),
            instrument_symbol: Some("US912810TH14".into()),
            instrument_key: Some("BOND:US912810TH14".into()),
            quote_mode: QuoteMode::Market,
            quote_ccy: "USD".into(),
            metadata: Some(serde_json::json!({"custom": true, "bond": {"couponRate": 0.04}})),
            ..Default::default()
        }
    }

    fn bond_profile() -> ProviderProfile {
        ProviderProfile {
            bond: Some(super::super::BondSpec {
                isin: Some("US912810TH14".into()),
                treasury_type: Some("Bond".into()),
                coupon_rate: Some(Decimal::new(5, 2)),
                maturity_date: Some(NaiveDate::from_ymd_opt(2043, 5, 15).unwrap()),
                coupon_frequency: Some("SEMI_ANNUAL".into()),
                face_value: Some(Decimal::from(1000)),
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn ensure_bond_broker_symbols_preserve_venues() {
        use crate::activities::activities_service_tests::tests::MockActivityRepository;
        for existing in [false, true] {
            let mut assets = vec![Asset {
                id: "unqualified-bond".into(),
                is_active: true,
                instrument_type: Some(InstrumentType::Bond),
                instrument_symbol: Some("ABC".into()),
                instrument_key: Some("BOND:ABC".into()),
                quote_ccy: "USD".into(),
                ..Default::default()
            }];
            let venues = [("XTSE", "CAD"), ("XNYS", "USD")];
            if existing {
                for (mic, currency) in venues {
                    assets.push(Asset {
                        id: format!("bond-{mic}"),
                        is_active: true,
                        instrument_type: Some(InstrumentType::Bond),
                        instrument_symbol: Some("ABC".into()),
                        instrument_exchange_mic: Some(mic.into()),
                        instrument_key: Some(format!("BOND:ABC@{mic}")),
                        quote_ccy: currency.into(),
                        ..Default::default()
                    });
                }
            }
            let quotes = Arc::new(TestQuoteService {
                reject_provider_calls: true,
                ..Default::default()
            });
            let service = AssetService::new(
                Arc::new(TestAssetRepository::with_assets(assets)),
                quotes.clone(),
            )
            .unwrap();
            let specs = venues
                .into_iter()
                .map(|(mic, currency)| {
                    AssetSpec::market_instrument(
                        "ABC".into(),
                        "ABC".into(),
                        Some(mic.into()),
                        InstrumentType::Bond,
                        currency.into(),
                    )
                })
                .collect();
            let result = service
                .ensure_assets(specs, &MockActivityRepository::default())
                .await
                .unwrap();

            assert_eq!(result.assets.len(), 2);
            assert_eq!(result.created_ids.len(), if existing { 0 } else { 2 });
            assert_eq!(service.get_assets().unwrap().len(), 3);
            for (mic, currency) in venues {
                let key = format!("BOND:ABC@{mic}");
                let id = result.input_to_asset_id.get(&key).unwrap();
                let asset = &result.assets[id];
                assert_ne!(id, "unqualified-bond");
                if existing {
                    assert_eq!(id, &format!("bond-{mic}"));
                }
                assert_eq!(asset.instrument_key.as_deref(), Some(key.as_str()));
                assert_eq!(asset.instrument_exchange_mic.as_deref(), Some(mic));
                assert_eq!(asset.quote_ccy, currency);
            }
            assert!(quotes.search_calls.lock().unwrap().is_empty());
            assert!(quotes.profile_calls.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn ensure_bonds_uses_local_resolution_and_creation_events() {
        use crate::activities::activities_service_tests::tests::MockActivityRepository;
        for existing in [false, true] {
            let mut asset = treasury_test_asset();
            asset.is_active = true;
            let repo = Arc::new(TestAssetRepository::with_assets(if existing {
                vec![asset]
            } else {
                vec![]
            }));
            let quotes = Arc::new(TestQuoteService {
                reject_provider_calls: true,
                ..Default::default()
            });
            let events = Arc::new(MockDomainEventSink::new());
            let service = AssetService::new(repo, quotes.clone())
                .unwrap()
                .with_event_sink(events.clone());
            let spec = AssetSpec::market_instrument(
                "US912810TH14".into(),
                "US912810TH14".into(),
                None,
                InstrumentType::Bond,
                "USD".into(),
            );
            let result = service
                .ensure_assets(vec![spec], &MockActivityRepository::default())
                .await
                .unwrap();
            assert_eq!(result.assets.len(), 1);
            assert_eq!(result.input_to_asset_id.len(), 1);
            assert!(quotes.profile_calls.lock().unwrap().is_empty());
            let emitted = events.events();
            if existing {
                assert!(result.created_ids.is_empty());
                assert!(emitted
                    .iter()
                    .all(|event| matches!(event, DomainEvent::AssetsUpdated { .. })));
            } else {
                assert_eq!(result.created_ids.len(), 1);
                assert!(
                    matches!(&emitted[..], [DomainEvent::AssetsCreated { asset_ids }] if asset_ids == &result.created_ids)
                );
            }
        }
    }

    #[tokio::test]
    async fn bond_profile_failure_is_retryable_and_preserves_saved_metadata() {
        let asset = treasury_test_asset();
        let before = asset.metadata.clone();
        let repo = Arc::new(TestAssetRepository::with_assets(vec![asset]));
        let quotes = Arc::new(TestQuoteService::default());
        let service = AssetService::new(repo.clone(), quotes.clone()).unwrap();
        assert_eq!(
            service
                .enrich_assets(vec!["treasury".into()])
                .await
                .unwrap(),
            (0, 0, 1)
        );
        assert!(quotes.enriched.lock().unwrap().is_empty());
        assert_eq!(repo.get_by_id("treasury").unwrap().metadata, before);
        quotes
            .profiles
            .lock()
            .unwrap()
            .insert("US912810TH14".into(), bond_profile());
        assert_eq!(
            service
                .enrich_assets(vec!["treasury".into()])
                .await
                .unwrap(),
            (1, 0, 0)
        );
        let bond = repo.get_by_id("treasury").unwrap().bond_spec().unwrap();
        assert_eq!(bond.coupon_rate, Some(Decimal::new(4, 2)));
        // Conflicting saved coupon must not be certified by attaching the provider's type.
        assert_eq!(bond.treasury_type, None);
        assert!(bond.maturity_date.is_some());
        assert_eq!(
            service
                .enrich_assets(vec!["treasury".into()])
                .await
                .unwrap(),
            (0, 1, 0)
        );
    }

    #[tokio::test]
    async fn bond_profile_preserves_edits_made_during_fetch() {
        let repo = Arc::new(TestAssetRepository::with_assets(
            vec![treasury_test_asset()],
        ));
        let quotes = Arc::new(TestQuoteService::default());
        quotes
            .profiles
            .lock()
            .unwrap()
            .insert("US912810TH14".into(), bond_profile());
        *quotes.edit_during_fetch.lock().unwrap() = Some(repo.clone());
        let service = AssetService::new(repo.clone(), quotes).unwrap();
        service.enrich_asset_profile("treasury").await.unwrap();
        let asset = repo.get_by_id("treasury").unwrap();
        assert_eq!(
            asset.bond_spec().unwrap().coupon_rate,
            Some(Decimal::new(7, 2))
        );
        assert_eq!(asset.bond_spec().unwrap().treasury_type, None);
        assert_eq!(asset.notes.as_deref(), Some("User edit during fetch"));
        assert_eq!(asset.metadata.unwrap()["custom"], true);
    }

    #[test]
    fn bond_profile_fills_missing_fields_and_preserves_unknown_fields() {
        let mut metadata = Some(serde_json::json!({"custom": true, "bond": {"customBond": true}}));
        merge_bond_profile(
            Some("US912810TH14"),
            &mut metadata,
            &bond_profile().bond.unwrap(),
        )
        .unwrap();
        let value = metadata.unwrap();
        assert_eq!(value["custom"], true);
        assert_eq!(value["bond"]["customBond"], true);
        assert_eq!(value["bond"]["treasuryType"], "Bond");
        let bond: super::super::BondSpec = serde_json::from_value(value["bond"].clone()).unwrap();
        assert!(bond.to_quote_metadata().has_valid_treasury_terms());
    }

    #[test]
    fn bond_profile_rejects_conflicting_identifiers_without_mutation() {
        for symbol in ["US037833EZ91", "US912810TH14"] {
            let mut metadata = Some(serde_json::json!({"bond": {"isin": "US037833EZ91"}}));
            let before = metadata.clone();
            assert!(
                merge_bond_profile(Some(symbol), &mut metadata, &bond_profile().bond.unwrap())
                    .is_err()
            );
            assert_eq!(metadata, before);
        }
    }

    #[derive(Default)]
    struct TestAssetRepository {
        assets: Mutex<Vec<Asset>>,
    }

    impl TestAssetRepository {
        fn with_assets(assets: Vec<Asset>) -> Self {
            Self {
                assets: Mutex::new(assets),
            }
        }
    }

    #[async_trait::async_trait]
    impl AssetRepositoryTrait for TestAssetRepository {
        async fn create(&self, new_asset: NewAsset) -> Result<Asset> {
            let now = Utc::now().naive_utc();
            let key_spec = AssetSpec {
                id: new_asset.id.clone(),
                display_code: new_asset.display_code.clone(),
                instrument_symbol: new_asset.instrument_symbol.clone(),
                instrument_exchange_mic: new_asset.instrument_exchange_mic.clone(),
                instrument_type: new_asset.instrument_type.clone(),
                quote_ccy: new_asset.quote_ccy.clone(),
                requested_quote_ccy: None,
                kind: new_asset.kind.clone(),
                quote_mode: Some(new_asset.quote_mode),
                name: new_asset.name.clone(),
                provider_config: new_asset.provider_config.clone(),
                provider_id: new_asset.provider_id.clone(),
                provider_symbol: new_asset.provider_symbol.clone(),
                metadata: new_asset.metadata.clone(),
            };
            let asset = Asset {
                id: new_asset
                    .id
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                kind: new_asset.kind,
                name: new_asset.name,
                display_code: new_asset.display_code,
                notes: new_asset.notes,
                metadata: new_asset.metadata,
                is_active: new_asset.is_active,
                quote_mode: new_asset.quote_mode,
                quote_ccy: new_asset.quote_ccy,
                instrument_type: new_asset.instrument_type,
                instrument_symbol: new_asset.instrument_symbol,
                instrument_exchange_mic: new_asset.instrument_exchange_mic,
                instrument_key: key_spec.instrument_key(),
                provider_config: new_asset.provider_config,
                exchange_name: None,
                created_at: now,
                updated_at: now,
            };
            self.assets.lock().unwrap().push(asset.clone());
            Ok(asset)
        }

        async fn create_batch(&self, new_assets: Vec<NewAsset>) -> Result<Vec<Asset>> {
            let mut created = Vec::new();
            for asset in new_assets {
                created.push(self.create(asset).await?);
            }
            Ok(created)
        }

        async fn update_profile(
            &self,
            asset_id: &str,
            payload: UpdateAssetProfile,
        ) -> Result<Asset> {
            let mut assets = self.assets.lock().unwrap();
            let asset = assets
                .iter_mut()
                .find(|asset| asset.id == asset_id)
                .ok_or_else(|| Error::Asset(format!("Asset not found: {asset_id}")))?;
            asset.name = payload.name;
            asset.display_code = payload.display_code;
            asset.notes = (!payload.notes.is_empty()).then_some(payload.notes);
            if let Some(kind) = payload.kind {
                asset.kind = kind;
            }
            if let Some(quote_mode) = payload.quote_mode {
                asset.quote_mode = quote_mode;
            }
            if let Some(quote_ccy) = payload.quote_ccy {
                asset.quote_ccy = quote_ccy;
            }
            if let Some(instrument_type) = payload.instrument_type {
                asset.instrument_type = Some(instrument_type);
            }
            if payload.instrument_symbol.is_some() {
                asset.instrument_symbol = payload.instrument_symbol;
            }
            if payload.instrument_exchange_mic.is_some() {
                asset.instrument_exchange_mic = payload.instrument_exchange_mic;
            }
            asset.provider_config = payload.provider_config;
            if payload.metadata.is_some() {
                asset.metadata = payload.metadata;
            }
            asset.updated_at = Utc::now().naive_utc();
            Ok(asset.clone())
        }

        async fn update_quote_mode(&self, _asset_id: &str, _quote_mode: &str) -> Result<Asset> {
            unimplemented!()
        }

        fn get_by_id(&self, asset_id: &str) -> Result<Asset> {
            self.assets
                .lock()
                .unwrap()
                .iter()
                .find(|asset| asset.id == asset_id)
                .cloned()
                .ok_or_else(|| {
                    Error::Database(DatabaseError::NotFound(format!(
                        "Asset not found: {asset_id}"
                    )))
                })
        }

        fn list(&self) -> Result<Vec<Asset>> {
            Ok(self.assets.lock().unwrap().clone())
        }

        fn list_by_asset_ids(&self, asset_ids: &[String]) -> Result<Vec<Asset>> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|asset| asset_ids.contains(&asset.id))
                .cloned()
                .collect())
        }

        async fn delete(&self, _asset_id: &str) -> Result<()> {
            unimplemented!()
        }

        fn search_by_symbol(&self, query: &str) -> Result<Vec<Asset>> {
            let query = query.trim().to_uppercase();
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|asset| {
                    asset
                        .instrument_symbol
                        .as_deref()
                        .is_some_and(|symbol| symbol.to_uppercase().contains(&query))
                })
                .cloned()
                .collect())
        }

        fn find_by_instrument_key(&self, instrument_key: &str) -> Result<Option<Asset>> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .find(|asset| asset.instrument_key.as_deref() == Some(instrument_key))
                .cloned())
        }

        async fn cleanup_legacy_metadata(&self, _asset_id: &str) -> Result<()> {
            Ok(())
        }

        async fn deactivate(&self, _asset_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn reactivate(&self, _asset_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn copy_user_metadata(&self, _source_id: &str, _target_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn deactivate_orphaned_investments(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct TestQuoteService {
        results: Arc<Mutex<HashMap<String, Vec<SymbolSearchResult>>>>,
        profiles: Arc<Mutex<HashMap<String, ProviderProfile>>>,
        latest_quotes: Arc<Mutex<HashMap<String, Quote>>>,
        search_calls: Arc<Mutex<Vec<String>>>,
        profile_calls: Mutex<Vec<String>>,
        reject_provider_calls: bool,
        enriched: Mutex<HashSet<String>>,
        edit_during_fetch: Mutex<Option<Arc<TestAssetRepository>>>,
    }

    impl TestQuoteService {
        fn with_latest_quote(self, asset_id: &str, quote: Quote) -> Self {
            self.latest_quotes
                .lock()
                .unwrap()
                .insert(asset_id.to_string(), quote);
            self
        }
    }

    #[async_trait::async_trait]
    impl QuoteServiceTrait for TestQuoteService {
        fn get_latest_quote(&self, symbol: &str) -> Result<Quote> {
            self.latest_quotes
                .lock()
                .unwrap()
                .get(symbol)
                .cloned()
                .ok_or_else(|| Error::Asset(format!("No latest quote for {symbol}")))
        }

        fn get_latest_quotes(&self, _symbols: &[String]) -> Result<HashMap<String, Quote>> {
            unimplemented!()
        }

        fn get_latest_quotes_as_of(
            &self,
            _symbols: &[String],
            _as_of: chrono::NaiveDate,
        ) -> Result<HashMap<String, Quote>> {
            Ok(HashMap::new())
        }

        fn get_sparse_asset_market_facts(
            &self,
            _requests: &[(String, chrono::NaiveDate)],
        ) -> Result<crate::quotes::SparseAssetMarketFacts> {
            Err(Error::Unexpected(
                "TestQuoteService::get_sparse_asset_market_facts should not be called".to_string(),
            ))
        }

        fn get_latest_quotes_snapshot(
            &self,
            _asset_ids: &[String],
        ) -> Result<HashMap<String, LatestQuoteSnapshot>> {
            unimplemented!()
        }

        fn get_latest_quotes_pair(
            &self,
            _symbols: &[String],
        ) -> Result<HashMap<String, LatestQuotePair>> {
            unimplemented!()
        }

        fn get_historical_quotes(&self, _symbol: &str) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        fn get_all_historical_quotes(&self) -> Result<HashMap<String, Vec<(NaiveDate, Quote)>>> {
            unimplemented!()
        }

        fn get_quotes_in_range(
            &self,
            _symbols: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        fn get_quotes_in_range_filled(
            &self,
            _symbols: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn get_daily_quotes(
            &self,
            _asset_ids: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<HashMap<NaiveDate, HashMap<String, Quote>>> {
            unimplemented!()
        }

        async fn add_quote(&self, _quote: &Quote) -> Result<Quote> {
            unimplemented!()
        }

        async fn update_quote(&self, _quote: Quote) -> Result<Quote> {
            unimplemented!()
        }

        async fn delete_quote(&self, _quote_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn bulk_upsert_quotes(&self, _quotes: Vec<Quote>) -> Result<usize> {
            unimplemented!()
        }

        async fn search_symbol(&self, query: &str) -> Result<Vec<SymbolSearchResult>> {
            self.search_symbol_with_currency(query, None).await
        }

        async fn search_symbol_with_currency(
            &self,
            query: &str,
            _account_currency: Option<&str>,
        ) -> Result<Vec<SymbolSearchResult>> {
            self.search_calls.lock().unwrap().push(query.to_string());
            Ok(self
                .results
                .lock()
                .unwrap()
                .get(&query.to_uppercase())
                .cloned()
                .unwrap_or_default())
        }

        async fn resolve_symbol_quote(
            &self,
            _symbol: &str,
            _mic: Option<&str>,
            _kind: Option<&InstrumentType>,
            _ccy: Option<&str>,
            _provider: Option<&str>,
            _provider_symbol: Option<&str>,
        ) -> Result<crate::quotes::ResolvedQuote> {
            assert!(
                !self.reject_provider_calls,
                "asset resolution must not fetch a quote"
            );
            Ok(crate::quotes::ResolvedQuote::default())
        }

        async fn get_asset_profile(
            &self,
            asset: &Asset,
        ) -> Result<super::super::assets_model::ProviderProfile> {
            assert!(
                !self.reject_provider_calls,
                "asset resolution must not fetch a profile"
            );
            self.profile_calls.lock().unwrap().push(asset.id.clone());
            if let Some(repo) = self.edit_during_fetch.lock().unwrap().take() {
                let mut assets = repo.assets.lock().unwrap();
                assets[0].metadata.as_mut().unwrap()["bond"]["couponRate"] =
                    serde_json::json!(0.07);
                assets[0].notes = Some("User edit during fetch".into());
            }
            let symbol = asset
                .instrument_symbol
                .as_deref()
                .or(asset.display_code.as_deref())
                .unwrap_or_default()
                .to_uppercase();
            self.profiles
                .lock()
                .unwrap()
                .get(&symbol)
                .cloned()
                .ok_or_else(|| Error::Asset(format!("No test profile for {symbol}")))
        }

        async fn fetch_quotes_from_provider(
            &self,
            _asset_id: &str,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn fetch_quotes_for_symbol(
            &self,
            _symbol: &str,
            _currency: &str,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn sync(
            &self,
            _mode: SyncMode,
            _asset_ids: Option<Vec<String>>,
        ) -> Result<SyncResult> {
            unimplemented!()
        }

        async fn resync(&self, _asset_ids: Option<Vec<String>>) -> Result<SyncResult> {
            unimplemented!()
        }

        async fn refresh_sync_state(&self) -> Result<()> {
            unimplemented!()
        }

        fn get_sync_plan(&self) -> Result<Vec<SymbolSyncPlan>> {
            unimplemented!()
        }

        async fn handle_activity_created(
            &self,
            _symbol: &str,
            _activity_date: NaiveDate,
        ) -> Result<()> {
            Ok(())
        }

        async fn handle_activity_deleted(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        async fn delete_sync_state(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        fn get_symbols_needing_sync(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(Vec::new())
        }

        fn get_sync_state(&self, symbol: &str) -> Result<Option<QuoteSyncState>> {
            let mut state = QuoteSyncState::new(symbol.into(), "TEST".into());
            if self.enriched.lock().unwrap().contains(symbol) {
                state.mark_profile_enriched();
            }
            Ok(Some(state))
        }

        async fn mark_profile_enriched(&self, symbol: &str) -> Result<()> {
            self.enriched.lock().unwrap().insert(symbol.into());
            Ok(())
        }

        fn get_assets_needing_profile_enrichment(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(Vec::new())
        }

        fn get_sync_states_with_errors(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(Vec::new())
        }

        async fn reset_sync_errors(&self, _asset_ids: &[String]) -> Result<()> {
            Ok(())
        }

        async fn reset_sync_state_for_profile_change(&self, _asset_id: &str) -> Result<()> {
            Ok(())
        }

        async fn update_position_status_from_holdings(
            &self,
            _current_holdings: &HashMap<String, rust_decimal::Decimal>,
        ) -> Result<()> {
            Ok(())
        }

        async fn get_providers_info(&self) -> Result<Vec<ProviderInfo>> {
            Ok(Vec::new())
        }

        async fn update_provider_settings(
            &self,
            _provider_id: &str,
            _priority: i32,
            _enabled: bool,
        ) -> Result<()> {
            Ok(())
        }
    }

    fn test_asset_service(assets: Vec<Asset>, quote_service: TestQuoteService) -> AssetService {
        AssetService::new(
            Arc::new(TestAssetRepository::with_assets(assets)),
            Arc::new(quote_service),
        )
        .unwrap()
    }

    #[test]
    fn get_asset_profile_normalizes_latest_quote_unit_market_price() {
        let asset = Asset {
            id: "asset-cty-lse".to_string(),
            kind: AssetKind::Investment,
            quote_mode: QuoteMode::Market,
            quote_ccy: "GBp".to_string(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            ..Default::default()
        };
        let quote = Quote {
            id: "quote-cty".to_string(),
            asset_id: asset.id.clone(),
            timestamp: Utc::now(),
            close: Decimal::new(565, 0),
            currency: "GBp".to_string(),
            created_at: Utc::now(),
            ..Default::default()
        };
        let service = test_asset_service(
            vec![asset],
            TestQuoteService::default().with_latest_quote("asset-cty-lse", quote),
        );

        let profile = service.get_asset_profile("asset-cty-lse").unwrap();

        assert_eq!(profile.asset.quote_ccy, "GBp");
        assert_eq!(profile.valuation_market_price, Some(Decimal::new(565, 2)));
        assert_eq!(profile.valuation_market_currency.as_deref(), Some("GBP"));

        let value = serde_json::to_value(profile).unwrap();
        assert_eq!(value["quoteCcy"], serde_json::json!("GBp"));
        assert_eq!(value["valuationMarketCurrency"], serde_json::json!("GBP"));
        assert!(value.get("valuationMarketPrice").is_some());
    }

    #[tokio::test]
    async fn create_asset_preserves_an_unknown_suffix_in_the_identity_key() {
        let service = test_asset_service(Vec::new(), TestQuoteService::default());

        let asset = service
            .create_asset(NewAsset {
                kind: AssetKind::Investment,
                name: Some("Unknown venue security".to_string()),
                display_code: Some("abc.zz".to_string()),
                is_active: true,
                quote_mode: QuoteMode::Market,
                quote_ccy: "usd".to_string(),
                instrument_type: Some(InstrumentType::Equity),
                instrument_symbol: Some("abc.zz".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(asset.display_code.as_deref(), Some("ABC.ZZ"));
        assert_eq!(asset.instrument_symbol.as_deref(), Some("ABC.ZZ"));
        assert_eq!(asset.instrument_exchange_mic, None);
        assert_eq!(asset.instrument_key.as_deref(), Some("EQUITY:ABC.ZZ"));
    }

    #[tokio::test]
    async fn create_asset_accepts_an_iso_only_mic_without_inventing_a_provider_rule() {
        let service = test_asset_service(Vec::new(), TestQuoteService::default());

        let asset = service
            .create_asset(NewAsset {
                kind: AssetKind::Investment,
                name: Some("ISO-only venue security".to_string()),
                display_code: Some("abc.zz".to_string()),
                is_active: true,
                quote_mode: QuoteMode::Market,
                quote_ccy: "usd".to_string(),
                instrument_type: Some(InstrumentType::Equity),
                instrument_symbol: Some("abc.zz".to_string()),
                instrument_exchange_mic: Some("21xx".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(asset.instrument_symbol.as_deref(), Some("ABC.ZZ"));
        assert_eq!(asset.instrument_exchange_mic.as_deref(), Some("21XX"));
        assert_eq!(asset.instrument_key.as_deref(), Some("EQUITY:ABC.ZZ@21XX"));
        assert_eq!(asset.provider_config, None);
    }

    #[tokio::test]
    async fn update_asset_profile_normalizes_explicit_exchange_correction() {
        let existing = Asset {
            id: "legacy-cboe-asset".to_string(),
            kind: AssetKind::Investment,
            name: Some("Legacy Cboe asset".to_string()),
            display_code: Some("VWRPL.XC".to_string()),
            is_active: true,
            quote_mode: QuoteMode::Market,
            quote_ccy: "GBP".to_string(),
            instrument_type: Some(InstrumentType::Equity),
            instrument_symbol: Some("VWRPL.XC".to_string()),
            instrument_exchange_mic: Some("CXE".to_string()),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            ..Default::default()
        };
        let service = test_asset_service(vec![existing], TestQuoteService::default());

        let asset = service
            .update_asset_profile(
                "legacy-cboe-asset",
                UpdateAssetProfile {
                    name: Some("Legacy Cboe asset".to_string()),
                    display_code: Some("VWRPL.XC".to_string()),
                    notes: String::new(),
                    kind: Some(AssetKind::Investment),
                    quote_mode: Some(QuoteMode::Market),
                    quote_ccy: Some("GBP".to_string()),
                    instrument_type: Some(InstrumentType::Equity),
                    instrument_symbol: Some("VWRPL.XC".to_string()),
                    instrument_exchange_mic: Some(" bcxe ".to_string()),
                    provider_config: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(asset.display_code.as_deref(), Some("VWRPL"));
        assert_eq!(asset.instrument_symbol.as_deref(), Some("VWRPL"));
        assert_eq!(asset.instrument_exchange_mic.as_deref(), Some("BCXE"));
    }

    #[tokio::test]
    async fn update_asset_profile_preserves_legacy_mic_when_canonical_twin_exists() {
        let now = Utc::now().naive_utc();
        let legacy = Asset {
            id: "legacy-cboe-asset".to_string(),
            kind: AssetKind::Investment,
            name: Some("Legacy Cboe asset".to_string()),
            display_code: Some("VWRP".to_string()),
            is_active: true,
            quote_mode: QuoteMode::Market,
            quote_ccy: "GBP".to_string(),
            instrument_type: Some(InstrumentType::Equity),
            instrument_symbol: Some("VWRP".to_string()),
            instrument_exchange_mic: Some("CXE".to_string()),
            instrument_key: Some("EQUITY:VWRP@CXE".to_string()),
            created_at: now,
            updated_at: now,
            ..Default::default()
        };
        let canonical_twin = Asset {
            id: "canonical-cboe-asset".to_string(),
            kind: AssetKind::Investment,
            name: Some("Canonical Cboe asset".to_string()),
            display_code: Some("VWRP".to_string()),
            is_active: true,
            quote_mode: QuoteMode::Market,
            quote_ccy: "GBP".to_string(),
            instrument_type: Some(InstrumentType::Equity),
            instrument_symbol: Some("VWRP".to_string()),
            instrument_exchange_mic: Some("BCXE".to_string()),
            instrument_key: Some("EQUITY:VWRP@BCXE".to_string()),
            created_at: now,
            updated_at: now,
            ..Default::default()
        };
        let service = test_asset_service(vec![legacy, canonical_twin], TestQuoteService::default());

        let asset = service
            .update_asset_profile(
                "legacy-cboe-asset",
                UpdateAssetProfile {
                    name: Some("Renamed legacy asset".to_string()),
                    display_code: Some("VWRP".to_string()),
                    notes: "edited notes".to_string(),
                    kind: Some(AssetKind::Investment),
                    quote_mode: Some(QuoteMode::Market),
                    quote_ccy: Some("GBP".to_string()),
                    instrument_type: Some(InstrumentType::Equity),
                    instrument_symbol: Some("VWRP".to_string()),
                    instrument_exchange_mic: Some("CXE".to_string()),
                    provider_config: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(asset.name.as_deref(), Some("Renamed legacy asset"));
        assert_eq!(asset.notes.as_deref(), Some("edited notes"));
        assert_eq!(asset.instrument_exchange_mic.as_deref(), Some("CXE"));
    }

    #[tokio::test]
    async fn update_asset_profile_preserves_unedited_legacy_identity_without_twin() {
        for mic in [None, Some("CXE"), Some(" cxe "), Some(" ")] {
            let existing = Asset {
                id: "legacy".to_string(),
                display_code: Some("VWRPL.XC".to_string()),
                instrument_symbol: Some("VWRPL.XC".to_string()),
                instrument_exchange_mic: Some("CXE".to_string()),
                instrument_type: Some(InstrumentType::Equity),
                instrument_key: Some("EQUITY:VWRPL.XC@CXE".to_string()),
                quote_ccy: "GBP".to_string(),
                ..Default::default()
            };
            let service = test_asset_service(vec![existing], TestQuoteService::default());
            let asset = service
                .update_asset_profile(
                    "legacy",
                    UpdateAssetProfile {
                        name: Some("Renamed".to_string()),
                        notes: "New notes".to_string(),
                        display_code: Some("VWRPL.XC".to_string()),
                        instrument_exchange_mic: mic.map(str::to_string),
                        instrument_symbol: None,
                        kind: None,
                        quote_mode: None,
                        quote_ccy: None,
                        instrument_type: None,
                        provider_config: None,
                        metadata: None,
                    },
                )
                .await
                .unwrap();
            assert_eq!(asset.name.as_deref(), Some("Renamed"));
            assert_eq!(asset.notes.as_deref(), Some("New notes"));
            assert_eq!(asset.instrument_symbol.as_deref(), Some("VWRPL.XC"));
            assert_eq!(asset.instrument_exchange_mic.as_deref(), Some("CXE"));
        }
    }

    #[tokio::test]
    async fn update_asset_profile_rejects_exchange_collision_without_changing_either_asset() {
        for (old_mic, target_mic) in [
            (Some("CXE"), "BCXE"),
            (Some("XNYS"), "XNAS"),
            (None, "XTSE"),
        ] {
            let existing = Asset {
                id: "original".to_string(),
                name: Some("Original".to_string()),
                display_code: Some("ABC".to_string()),
                instrument_symbol: Some("ABC".to_string()),
                instrument_exchange_mic: old_mic.map(str::to_string),
                instrument_type: Some(InstrumentType::Equity),
                quote_ccy: "USD".to_string(),
                ..Default::default()
            };
            let twin = Asset {
                id: "twin".to_string(),
                instrument_exchange_mic: Some(target_mic.to_string()),
                instrument_key: Some(format!("EQUITY:ABC@{target_mic}")),
                ..existing.clone()
            };
            let service = test_asset_service(vec![existing, twin], TestQuoteService::default());
            let error = service
                .update_asset_profile(
                    "original",
                    UpdateAssetProfile {
                        name: Some("Should not save".to_string()),
                        notes: "Should not save".to_string(),
                        instrument_exchange_mic: Some(target_mic.to_string()),
                        display_code: None,
                        instrument_symbol: None,
                        kind: None,
                        quote_mode: None,
                        quote_ccy: None,
                        instrument_type: None,
                        provider_config: None,
                        metadata: None,
                    },
                )
                .await
                .unwrap_err();
            assert!(matches!(error, Error::ConstraintViolation(_)));
            assert!(error.to_string().contains("ASSET_IDENTITY_CONFLICT"));
            let unchanged = service.asset_repository.get_by_id("original").unwrap();
            assert_eq!(unchanged.name.as_deref(), Some("Original"));
            assert_eq!(unchanged.notes, None);
            assert_eq!(unchanged.instrument_exchange_mic.as_deref(), old_mic);
            let twin = service.asset_repository.get_by_id("twin").unwrap();
            assert_eq!(twin.name.as_deref(), Some("Original"));
            assert_eq!(
                twin.instrument_key,
                Some(format!("EQUITY:ABC@{target_mic}"))
            );
        }
    }

    #[tokio::test]
    async fn update_asset_profile_preserves_pair_key_on_unrelated_edit() {
        for instrument_type in [InstrumentType::Crypto, InstrumentType::Fx] {
            let existing = Asset {
                id: "original".to_string(),
                display_code: Some("USD/eur".to_string()),
                instrument_symbol: Some("USD".to_string()),
                instrument_type: Some(instrument_type.clone()),
                quote_mode: QuoteMode::Market,
                quote_ccy: "eur".to_string(),
                ..Default::default()
            };
            let twin = Asset {
                id: "twin".to_string(),
                quote_ccy: "EUR".to_string(),
                instrument_key: Some(format!("{}:USD/EUR", instrument_type.as_db_str())),
                ..existing.clone()
            };
            let service = test_asset_service(vec![existing, twin], TestQuoteService::default());
            let payload = serde_json::from_value(serde_json::json!({
                "name": "Renamed",
                "notes": "Edited",
                "displayCode": "USD/eur",
                "quoteCcy": "EUR"
            }))
            .unwrap();
            let asset = service
                .update_asset_profile("original", payload)
                .await
                .unwrap();
            assert_eq!(asset.name.as_deref(), Some("Renamed"));
            assert_eq!(asset.instrument_symbol.as_deref(), Some("USD"));
            assert_eq!(asset.quote_ccy, "eur");
        }
    }

    #[tokio::test]
    async fn update_asset_profile_checks_exact_persisted_pair_currency() {
        for instrument_type in [InstrumentType::Crypto, InstrumentType::Fx] {
            for quote_mode in [QuoteMode::Manual, QuoteMode::Market] {
                for twin_ccy in ["eur", "EUR"] {
                    let existing = Asset {
                        id: "original".to_string(),
                        instrument_symbol: Some("USD".to_string()),
                        instrument_type: Some(instrument_type.clone()),
                        quote_mode,
                        quote_ccy: "JPY".to_string(),
                        ..Default::default()
                    };
                    let twin = Asset {
                        id: "twin".to_string(),
                        quote_ccy: twin_ccy.to_string(),
                        instrument_key: Some(format!(
                            "{}:USD/{twin_ccy}",
                            instrument_type.as_db_str()
                        )),
                        ..existing.clone()
                    };
                    let service =
                        test_asset_service(vec![existing, twin], TestQuoteService::default());
                    let payload = serde_json::from_value(serde_json::json!({
                        "notes": "Edited", "quoteCcy": "eur"
                    }))
                    .unwrap();
                    let result = service.update_asset_profile("original", payload).await;
                    let persisted_ccy = if quote_mode == QuoteMode::Market {
                        "EUR"
                    } else {
                        "eur"
                    };
                    if twin_ccy == persisted_ccy {
                        assert!(matches!(result, Err(Error::ConstraintViolation(_))));
                        assert_eq!(
                            service
                                .asset_repository
                                .get_by_id("original")
                                .unwrap()
                                .quote_ccy,
                            "JPY"
                        );
                    } else {
                        assert_eq!(result.unwrap().quote_ccy, persisted_ccy);
                    }
                }
            }
        }
    }

    #[test]
    fn test_provider_config_for_resolution_only_stores_required_overrides() {
        let deterministic = AssetService::provider_config_for_resolution(
            Some("YAHOO"),
            Some("SHOP.TO"),
            Some(&InstrumentType::Equity),
            Some("SHOP"),
            Some("XTSE"),
            Some("CAD"),
        );
        assert_eq!(deterministic, None);

        let share_class = AssetService::provider_config_for_resolution(
            Some("YAHOO"),
            Some("BRK-B"),
            Some(&InstrumentType::Equity),
            Some("BRK.B"),
            None,
            Some("USD"),
        );
        assert_eq!(share_class, None);

        let equity_override = AssetService::provider_config_for_resolution(
            Some("YAHOO"),
            Some("SHOP.TSX"),
            Some(&InstrumentType::Equity),
            Some("SHOP"),
            Some("XTSE"),
            Some("CAD"),
        )
        .unwrap();
        assert_eq!(
            equity_override.get("overrides"),
            Some(&serde_json::json!({
                "YAHOO": {
                    "type": "equity_symbol",
                    "symbol": "SHOP.TSX"
                }
            }))
        );

        let metal_override = AssetService::provider_config_for_resolution(
            Some("METAL_PRICE_API"),
            Some("XAU-1KG"),
            Some(&InstrumentType::Metal),
            Some("XAU"),
            None,
            Some("USD"),
        )
        .unwrap();
        assert_eq!(
            metal_override.get("overrides"),
            Some(&serde_json::json!({
                "METAL_PRICE_API": {
                    "type": "metal_symbol",
                    "symbol": "XAU-1KG",
                    "quote": "USD"
                }
            }))
        );

        let missing_crypto_quote = AssetService::provider_config_for_resolution(
            Some("YAHOO"),
            Some("BTC-USD"),
            Some(&InstrumentType::Crypto),
            Some("BTC"),
            None,
            None,
        );
        assert_eq!(missing_crypto_quote, None);

        assert_eq!(
            AssetService::provider_config_for_resolution(
                None,
                Some("SHOP.TO"),
                Some(&InstrumentType::Equity),
                Some("SHOP"),
                Some("XTSE"),
                Some("CAD"),
            ),
            None
        );
    }

    #[test]
    fn test_refresh_market_quote_ccy_on_mic_change_when_quote_not_explicit() {
        assert!(AssetService::should_refresh_market_quote_ccy_on_mic_change(
            QuoteMode::Market,
            None,
            Some("xlon"),
            Some("XNAS"),
        ));
    }

    #[test]
    fn test_do_not_refresh_market_quote_ccy_without_mic_change() {
        assert!(
            !AssetService::should_refresh_market_quote_ccy_on_mic_change(
                QuoteMode::Market,
                None,
                Some(" xnas "),
                Some("XNAS"),
            )
        );
    }

    #[test]
    fn test_do_not_refresh_market_quote_ccy_when_quote_explicitly_set() {
        assert!(
            !AssetService::should_refresh_market_quote_ccy_on_mic_change(
                QuoteMode::Market,
                Some("USD"),
                Some("XLON"),
                Some("XNAS"),
            )
        );
    }

    #[test]
    fn test_bond_provider_config_is_none() {
        let provider_config = AssetService::inferred_provider_config(
            QuoteMode::Market,
            Some(&InstrumentType::Bond),
            Some("US91282CFT32"),
            None,
        );

        assert!(provider_config.is_none());
    }

    #[test]
    fn test_equity_provider_config_is_not_defaulted_to_yahoo() {
        let provider_config = AssetService::inferred_provider_config(
            QuoteMode::Market,
            Some(&InstrumentType::Equity),
            Some("SHOP"),
            Some("XTSE"),
        );

        assert!(provider_config.is_none());
    }

    #[test]
    fn test_bf_isin_equity_prefers_boerse_frankfurt() {
        let provider_config = AssetService::inferred_provider_config(
            QuoteMode::Market,
            Some(&InstrumentType::Equity),
            Some("IE00BTJRMP35"),
            Some("XETR"),
        );

        assert_eq!(
            provider_config,
            Some(serde_json::json!({ "preferred_provider": "BOERSE_FRANKFURT" })),
            "ISIN-backed XETR/XFRA equities should prefer Boerse Frankfurt"
        );
    }

    #[test]
    fn test_profile_provider_change_resets_sync_state() {
        let before = Asset {
            provider_config: Some(serde_json::json!({ "preferred_provider": "YAHOO" })),
            ..test_market_asset()
        };
        let after = Asset {
            provider_config: Some(serde_json::json!({
                "preferred_provider": "BOERSE_FRANKFURT"
            })),
            ..before.clone()
        };

        assert!(AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_bond_isin_metadata_change_resets_sync_state() {
        let before = Asset {
            instrument_type: Some(InstrumentType::Bond),
            metadata: Some(serde_json::json!({
                "identifiers": {
                    "isin": "US912797NQ65"
                }
            })),
            ..test_market_asset()
        };
        let after = Asset {
            metadata: Some(serde_json::json!({
                "identifiers": {
                    "isin": "IT0005415291"
                }
            })),
            ..before.clone()
        };

        assert!(AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_notes_change_does_not_reset_sync_state() {
        let before = test_market_asset();
        let after = Asset {
            notes: Some("Updated notes".to_string()),
            ..before.clone()
        };

        assert!(!AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    fn test_bond_asset(bond: serde_json::Value) -> Asset {
        Asset {
            instrument_type: Some(InstrumentType::Bond),
            instrument_symbol: Some("US912828XY12".to_string()),
            metadata: Some(serde_json::json!({ "bond": bond })),
            ..test_market_asset()
        }
    }

    #[test]
    fn test_bond_pricing_input_change_resets_sync_state() {
        let before = test_bond_asset(serde_json::json!({ "maturityDate": "2032-02-15" }));

        for after in [
            test_bond_asset(serde_json::json!({ "maturityDate": "2033-02-15" })),
            test_bond_asset(serde_json::json!({
                "maturityDate": "2032-02-15", "couponRate": 0.04375
            })),
            test_bond_asset(serde_json::json!({
                "maturityDate": "2032-02-15", "couponFrequency": "ANNUAL"
            })),
        ] {
            assert!(
                AssetService::should_reset_sync_state_after_profile_change(&before, &after),
                "editing a bond pricing input must re-resolve the provider binding"
            );
        }
    }

    #[test]
    fn test_bond_face_value_change_does_not_reset_sync_state() {
        let before = test_bond_asset(serde_json::json!({ "maturityDate": "2032-02-15" }));
        let after = test_bond_asset(serde_json::json!({
            "maturityDate": "2032-02-15", "faceValue": 100
        }));

        // faceValue cancels out of the calculated price, so it changes nothing.
        assert!(!AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_non_bond_metadata_change_does_not_reset_sync_state() {
        let before = test_market_asset();
        let after = Asset {
            metadata: Some(serde_json::json!({ "contractMultiplier": 50 })),
            ..before.clone()
        };

        assert!(!AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_quote_mode_change_resets_sync_state() {
        let before = test_market_asset();
        let after = Asset {
            quote_mode: QuoteMode::Manual,
            ..before.clone()
        };

        assert!(AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_quote_ccy_change_resets_sync_state() {
        let before = test_market_asset();
        let after = Asset {
            quote_ccy: "EUR".to_string(),
            ..before.clone()
        };

        assert!(AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_instrument_type_change_resets_sync_state() {
        let before = test_market_asset();
        let after = Asset {
            instrument_type: Some(InstrumentType::Bond),
            ..before.clone()
        };

        assert!(AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_instrument_symbol_change_resets_sync_state() {
        let before = test_market_asset();
        let after = Asset {
            instrument_symbol: Some("MSFT".to_string()),
            ..before.clone()
        };

        assert!(AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[test]
    fn test_instrument_exchange_mic_change_resets_sync_state() {
        let before = Asset {
            instrument_exchange_mic: Some("XNAS".to_string()),
            ..test_market_asset()
        };
        let after = Asset {
            instrument_exchange_mic: Some("XLON".to_string()),
            ..before.clone()
        };

        assert!(AssetService::should_reset_sync_state_after_profile_change(
            &before, &after
        ));
    }

    #[tokio::test]
    async fn bulk_enrichment_emits_one_event_for_all_multiplier_changes() {
        let option_a = Asset {
            id: "option-a".to_string(),
            instrument_symbol: Some("OPTION-A".to_string()),
            instrument_type: None,
            ..test_market_asset()
        };
        let option_b = Asset {
            id: "option-b".to_string(),
            instrument_symbol: Some("OPTION-B".to_string()),
            instrument_type: None,
            ..test_market_asset()
        };
        let equity = Asset {
            id: "equity".to_string(),
            instrument_symbol: Some("EQUITY".to_string()),
            ..test_market_asset()
        };
        let quote_service = TestQuoteService::default();
        for symbol in ["OPTION-A", "OPTION-B"] {
            quote_service.profiles.lock().unwrap().insert(
                symbol.to_string(),
                ProviderProfile {
                    symbol: symbol.to_string(),
                    asset_type: Some("OPTION".to_string()),
                    currency: "USD".to_string(),
                    data_source: "TEST".to_string(),
                    ..Default::default()
                },
            );
        }
        quote_service.profiles.lock().unwrap().insert(
            "EQUITY".to_string(),
            ProviderProfile {
                symbol: "EQUITY".to_string(),
                asset_type: Some("EQUITY".to_string()),
                currency: "USD".to_string(),
                data_source: "TEST".to_string(),
                ..Default::default()
            },
        );
        let event_sink = Arc::new(MockDomainEventSink::new());
        let service = AssetService::new(
            Arc::new(TestAssetRepository::with_assets(vec![
                option_a, option_b, equity,
            ])),
            Arc::new(quote_service),
        )
        .unwrap()
        .with_event_sink(event_sink.clone());

        assert_eq!(
            service
                .enrich_assets(vec![
                    "option-a".to_string(),
                    "equity".to_string(),
                    "option-b".to_string(),
                ])
                .await
                .unwrap(),
            (3, 0, 0),
        );

        let events = event_sink.events();
        assert_eq!(events.len(), 1);
        let DomainEvent::AssetsUpdated { asset_ids } = &events[0] else {
            panic!("expected one AssetsUpdated event");
        };
        assert_eq!(
            asset_ids,
            &vec!["option-a".to_string(), "option-b".to_string()]
        );
    }

    fn test_market_asset() -> Asset {
        Asset {
            id: "asset-1".to_string(),
            kind: AssetKind::Investment,
            quote_mode: QuoteMode::Market,
            quote_ccy: "USD".to_string(),
            instrument_type: Some(InstrumentType::Equity),
            instrument_symbol: Some("AAPL".to_string()),
            ..Default::default()
        }
    }
}
