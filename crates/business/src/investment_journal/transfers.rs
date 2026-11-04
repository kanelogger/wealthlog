use std::collections::BTreeSet;

use num_bigint::BigInt;
use num_traits::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use wealthfolio_core::fx::currency::currency_minor_unit;

use super::model::{invalid, nonblank, validate_currency};
use super::{ExactDecimal, JournalResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransferShare {
    pub purpose_id: String,
    pub source_amount: ExactDecimal,
    pub destination_amount: ExactDecimal,
}

/// Distribute the *recorded* two transfer legs. No FX service is consulted.
/// Ties in largest remainder use the stable purpose ID, independent of input order.
pub fn apportion_transfer(
    source: ExactDecimal,
    destination: ExactDecimal,
    destination_currency: &str,
    recorded_destination_precision: u32,
    shares: &[(String, ExactDecimal)],
) -> JournalResult<Vec<TransferShare>> {
    validate_currency(destination_currency)?;
    if recorded_destination_precision > 28 {
        return Err(invalid(
            "recorded destination precision is outside decimal range",
        ));
    }
    if source.value() <= Decimal::ZERO || destination.value() <= Decimal::ZERO || shares.is_empty()
    {
        return Err(invalid(
            "transfer legs and migration shares must be positive",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut sum = ExactDecimal::ZERO;
    for (id, share) in shares {
        nonblank(id, "purposeId")?;
        if !ids.insert(id) || share.value() <= Decimal::ZERO {
            return Err(invalid("purpose shares must be positive and unique"));
        }
        sum = sum.checked_add(*share)?;
    }
    if sum > source {
        return Err(invalid(
            "migration shares exceed the source transfer amount",
        ));
    }
    let precision = destination
        .value()
        .scale()
        .max(currency_minor_unit(destination_currency).scale())
        .max(recorded_destination_precision);
    let power = |scale| BigInt::from(10).pow(scale);
    let destination_units = BigInt::from(destination.value().mantissa())
        * power(precision - destination.value().scale());
    // Integer ratios preserve exact remainder ordering for repeating decimals
    // and avoid overflow in intermediate Decimal multiplication.
    let fraction = |part: Decimal| {
        let numerator =
            &destination_units * BigInt::from(part.mantissa()) * power(source.value().scale());
        let denominator = BigInt::from(source.value().mantissa()) * power(part.scale());
        (numerator, denominator)
    };
    let target = if sum == source {
        destination_units.clone()
    } else {
        let (n, d) = fraction(sum.value());
        let floor = &n / &d;
        if (&n % &d) * 2 >= d {
            floor + 1
        } else {
            floor
        }
    };
    let mut rows = Vec::new();
    let mut assigned = BigInt::from(0);
    for (id, amount) in shares {
        let (n, d) = fraction(amount.value());
        let floor = &n / &d;
        assigned += &floor;
        let remainder = n % &d;
        rows.push((id.clone(), *amount, floor, remainder, d));
    }
    rows.sort_by(|a, b| (&b.3 * &a.4).cmp(&(&a.3 * &b.4)).then(a.0.cmp(&b.0)));
    let remainder = (target - assigned)
        .to_usize()
        .ok_or_else(|| invalid("invalid transfer rounding remainder"))?;
    if remainder > rows.len() {
        return Err(invalid(
            "transfer rounding did not conserve the recorded target",
        ));
    }
    for row in rows.iter_mut().take(remainder) {
        row.2 += 1;
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter()
        .map(|(purpose_id, source_amount, units, _, _)| {
            let mantissa = units
                .to_i128()
                .ok_or_else(|| invalid("transfer share out of range"))?;
            let amount = Decimal::try_from_i128_with_scale(mantissa, precision)
                .map_err(|_| invalid("transfer share out of range"))?;
            Ok(TransferShare {
                purpose_id,
                source_amount,
                destination_amount: ExactDecimal::new(amount),
            })
        })
        .collect()
}
