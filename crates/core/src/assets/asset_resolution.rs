use std::borrow::Cow;

use super::Asset;
use wealthfolio_market_data::{
    ProviderId, ProviderInstrument, ProviderOverrides, QuoteContext, ResolverChain, SymbolResolver,
};

fn provider_overrides_for_asset(asset: &Asset) -> Option<ProviderOverrides> {
    asset
        .provider_overrides()
        .and_then(|json| ProviderOverrides::from_json(json).ok())
}

pub(crate) fn custom_provider_code(asset: &Asset) -> Option<String> {
    asset
        .provider_config
        .as_ref()
        .and_then(|config| config.get("custom_provider_code"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn asset_provider_alias_symbols(asset: &Asset) -> Vec<String> {
    let mut aliases = Vec::new();
    if let Some(overrides) = provider_overrides_for_asset(asset) {
        aliases.extend(
            overrides
                .overrides
                .values()
                .map(ProviderInstrument::to_symbol_string),
        );
    }
    if let Some(custom_code) = custom_provider_code(asset) {
        aliases.push(custom_code);
    }
    if let (Some(provider), Some(instrument)) =
        (asset.preferred_provider(), asset.to_instrument_id())
    {
        let context = QuoteContext {
            instrument,
            identifiers: Default::default(),
            overrides: provider_overrides_for_asset(asset),
            currency_hint: (!asset.quote_ccy.trim().is_empty())
                .then(|| Cow::Owned(asset.quote_ccy.clone())),
            preferred_provider: Some(Cow::Owned(provider.clone())),
            bond_metadata: None,
            custom_provider_code: custom_provider_code(asset),
        };
        let provider_id: ProviderId = Cow::Owned(provider);
        if let Ok(resolved) = ResolverChain::new().resolve(&provider_id, &context) {
            aliases.push(resolved.instrument.to_symbol_string());
        }
    }

    aliases.sort_by_key(|alias| alias.to_uppercase());
    aliases.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    aliases
}
