use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{JournalError, JournalResult};

/// Exact decimal at the API boundary. JSON numbers are deliberately rejected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExactDecimal(Decimal);

impl schemars::JsonSchema for ExactDecimal {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExactDecimal".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type":"string",
            "pattern":"^-?([0-9]+(\\.[0-9]*)?|\\.[0-9]+)$",
            "description":"Exact decimal string; never a JSON number. Missing is distinct from zero; business validation checks scale, range and sign."
        })
    }
}

impl ExactDecimal {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn new(value: Decimal) -> Self {
        Self(value.normalize())
    }

    pub fn value(self) -> Decimal {
        self.0
    }

    pub fn parse(value: &str) -> JournalResult<Self> {
        // from_str_exact rejects excess precision instead of rounding it.
        if value.is_empty()
            || value.trim() != value
            || value
                .bytes()
                .any(|c| !matches!(c, b'0'..=b'9' | b'-' | b'.'))
        {
            return Err(invalid("expected a finite decimal string"));
        }
        Decimal::from_str_exact(value)
            .map(Self::new)
            .map_err(|_| invalid("decimal is invalid or outside the supported precision"))
    }

    pub fn nonnegative(self) -> JournalResult<Self> {
        if self.0 < Decimal::ZERO {
            Err(invalid("amount or quantity must be nonnegative"))
        } else {
            Ok(self)
        }
    }

    pub fn checked_add(self, other: Self) -> JournalResult<Self> {
        self.0
            .checked_add(other.0)
            .map(Self::new)
            .ok_or_else(|| invalid("decimal sum is outside the supported range"))
    }
}

impl std::fmt::Display for ExactDecimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.normalize())
    }
}

impl Serialize for ExactDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ExactDecimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Resource identity makes impossible currency/asset combinations unrepresentable.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum FundResource {
    #[serde(rename_all = "camelCase")]
    Cash {
        account_id: String,
        currency: String,
    },
    #[serde(rename_all = "camelCase")]
    Security {
        account_id: String,
        asset_id: String,
    },
}

impl FundResource {
    pub fn account_id(&self) -> &str {
        match self {
            Self::Cash { account_id, .. } | Self::Security { account_id, .. } => account_id,
        }
    }

    pub fn validate(&self) -> JournalResult<()> {
        nonblank(self.account_id(), "accountId")?;
        match self {
            Self::Cash { currency, .. } => validate_currency(currency),
            Self::Security { asset_id, .. } => nonblank(asset_id, "assetId"),
        }
    }
}

/// None means the ledger cannot establish B, never a known zero balance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBalance {
    pub actual: Option<ExactDecimal>,
    pub allocated: ExactDecimal,
    pub unallocated: Option<ExactDecimal>,
    pub excess: Option<ExactDecimal>,
    pub negative_actual: Option<ExactDecimal>,
}

impl ResourceBalance {
    pub fn calculate(actual: Option<ExactDecimal>, allocated: ExactDecimal) -> JournalResult<Self> {
        allocated.nonnegative()?;
        let known = actual.map(|b| b.value().max(Decimal::ZERO));
        Ok(Self {
            actual,
            allocated,
            unallocated: known
                .map(|b| ExactDecimal::new((b - allocated.value()).max(Decimal::ZERO))),
            excess: known.map(|b| ExactDecimal::new((allocated.value() - b).max(Decimal::ZERO))),
            negative_actual: actual.map(|b| ExactDecimal::new((-b.value()).max(Decimal::ZERO))),
        })
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InvestmentSourceKind {
    Activity,
    ManualTrade,
    Snapshot,
}

impl InvestmentSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Activity => "ACTIVITY",
            Self::ManualTrade => "MANUAL_TRADE",
            Self::Snapshot => "SNAPSHOT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedSource {
    pub kind: InvestmentSourceKind,
    pub id: String,
    pub version: String,
    /// Whether the owning accounting service actually applied this source.
    pub applied: bool,
    /// Supplied by accounting, not inferred from the balance difference.
    pub effect: SourceEffect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceEffect {
    Ordinary,
    Trade,
    CashTransfer,
    SecurityTransfer,
    CorporateAction,
    Calibration,
    None,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceObservation {
    pub resource: FundResource,
    pub actual: Option<ExactDecimal>,
    pub observed_at: String,
    pub as_of_date: chrono::NaiveDate,
    pub tracking_mode: String,
    pub state_version: String,
    pub sources: Vec<ConfirmedSource>,
    /// Known writes can advance a resource baseline without silently clearing
    /// earlier, unconfirmed source changes. Explicit reconciliation clears it.
    #[serde(default)]
    pub unresolved_sources: Vec<super::FundReconciliationItem>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllocationDelta {
    pub purpose_id: String,
    pub resource: FundResource,
    pub delta: ExactDecimal,
}

pub(crate) fn apply_deltas(
    balances: &BTreeMap<(String, FundResource), ExactDecimal>,
    deltas: &[AllocationDelta],
    actual: &BTreeMap<FundResource, Option<ExactDecimal>>,
) -> JournalResult<BTreeMap<(String, FundResource), ExactDecimal>> {
    let mut result = balances.clone();
    let mut resources = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for line in deltas {
        line.resource.validate()?;
        nonblank(&line.purpose_id, "purposeId")?;
        if line.delta == ExactDecimal::ZERO {
            return Err(invalid("allocation entries must have a nonzero delta"));
        }
        let key = (line.purpose_id.clone(), line.resource.clone());
        if !identities.insert(key.clone()) {
            return Err(invalid(
                "a purpose/resource may occur only once per operation",
            ));
        }
        let next = result
            .get(&key)
            .copied()
            .unwrap_or_default()
            .checked_add(line.delta)?;
        if next.value() < Decimal::ZERO {
            return Err(JournalError::NegativeAllocation);
        }
        result.insert(key, next);
        resources.insert(line.resource.clone());
    }
    for resource in resources {
        let total = |rows: &BTreeMap<(String, FundResource), ExactDecimal>| {
            rows.iter()
                .filter(|((_, r), _)| *r == resource)
                .try_fold(ExactDecimal::ZERO, |sum, (_, v)| sum.checked_add(*v))
        };
        let before = total(balances)?;
        let after = total(&result)?;
        match actual.get(&resource).copied().flatten() {
            Some(b) if after.value() > b.value().max(Decimal::ZERO) && after > before => {
                return Err(JournalError::InsufficientResource);
            }
            None if after > before => return Err(JournalError::ResourceUnavailable),
            _ => {}
        }
    }
    Ok(result)
}

pub(crate) fn validate_currency(value: &str) -> JournalResult<()> {
    if value.len() == 3 && value.bytes().all(|c| c.is_ascii_uppercase()) {
        Ok(())
    } else {
        Err(invalid("currency must be an uppercase three-letter code"))
    }
}

pub(crate) fn nonblank(value: &str, field: &str) -> JournalResult<()> {
    if value.trim().is_empty() {
        Err(invalid(format!("{field} must not be blank")))
    } else {
        Ok(())
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> JournalError {
    JournalError::InvalidInput(message.into())
}
