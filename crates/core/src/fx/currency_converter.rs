use crate::fx::fx_errors::FxError;
use crate::fx::fx_model::ExchangeRate;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

type RateHistory = BTreeMap<NaiveDate, (Decimal, Arc<ExchangeRate>)>;

/// A calculator for currency conversions using a Graph-based approach.
/// It stores rates as independent time-series per pair and calculates paths on demand.
/// It supports exact matching and "nearest neighbor" lookups (past or future).
pub struct CurrencyConverter {
    /// Graph adjacency list: Currency -> Set of connected currencies.
    /// Used to quickly find valid paths during conversion.
    adj: HashMap<String, HashSet<String>>,

    /// The actual rate data.
    /// Key: (From_Currency, To_Currency)
    /// Value: BTreeMap<Date, Rate>
    /// Using BTreeMap allows O(log N) lookup for dates.
    rates: HashMap<(String, String), RateHistory>,
}

impl CurrencyConverter {
    /// Creates a new `CurrencyConverter` from a Vec of ExchangeRate.
    pub fn new(exchange_rates: Vec<ExchangeRate>) -> Result<Self, FxError> {
        let mut converter = CurrencyConverter {
            adj: HashMap::new(),
            rates: HashMap::new(),
        };
        converter.add_historical_rates(exchange_rates);
        Ok(converter)
    }

    /// Adds historical FX rates.
    /// Fast insertion O(1) per rate. No matrix pre-calculation.
    /// Automatically handles inverses and graph connectivity.
    pub fn add_historical_rates(&mut self, rates: Vec<ExchangeRate>) {
        for rate in rates {
            if rate.from_currency == rate.to_currency {
                continue;
            }

            let date = rate.timestamp.date_naive();
            let forward_pair = (rate.from_currency.clone(), rate.to_currency.clone());
            let inverse_pair = (rate.to_currency.clone(), rate.from_currency.clone());
            let forward_rate = rate.rate;
            let source = Arc::new(rate.clone());

            // 1. Store Forward Rate
            self.rates
                .entry(forward_pair.clone())
                .or_default()
                .insert(date, (forward_rate, source.clone()));

            self.adj
                .entry(rate.from_currency.clone())
                .or_default()
                .insert(rate.to_currency.clone());

            // 2. Store Inverse Rate (if valid)
            if let Some(inverse_rate) = Decimal::ONE.checked_div(forward_rate) {
                self.rates
                    .entry(inverse_pair)
                    .or_default()
                    .insert(date, (inverse_rate, source));

                self.adj
                    .entry(rate.to_currency.clone())
                    .or_default()
                    .insert(rate.from_currency.clone());
            }
        }
    }

    /// Finds the direct rate between two connected currencies.
    /// Strategy: Bidirectional Nearest Neighbor.
    /// 1. Finds the closest rate ON or BEFORE the date.
    /// 2. Finds the closest rate AFTER the date.
    /// 3. Returns the one with the smallest day difference.
    fn get_direct_rate(
        &self,
        from: &str,
        to: &str,
        date: NaiveDate,
    ) -> Option<&(Decimal, Arc<ExchangeRate>)> {
        let key = (from.to_string(), to.to_string());

        if let Some(history) = self.rates.get(&key) {
            // Find the closest rate in the past/present (<= date)
            let prev = history.range(..=date).next_back();

            // Find the closest rate in the future/present (>= date)
            let next = history.range(date..).next();

            match (prev, next) {
                (Some((d1, r1)), Some((d2, r2))) => {
                    // If exact match found, d1 == d2 == date.
                    if d1 == d2 {
                        return Some(r1);
                    }

                    // Compare distances.
                    // date is NaiveDate (value), d1/d2 are &NaiveDate (refs)
                    // We must dereference d1/d2, but NOT date.
                    let dist_prev = (date - *d1).num_days().abs();
                    let dist_next = (*d2 - date).num_days().abs();

                    if dist_prev <= dist_next {
                        Some(r1)
                    } else {
                        Some(r2)
                    }
                }
                (Some((_, r)), None) => Some(r), // Only past exists
                (None, Some((_, r))) => Some(r), // Only future exists (e.g., Static single rate)
                (None, None) => None,
            }
        } else {
            None
        }
    }

    /// Converts an amount using Breadth-First Search (BFS) to find the shortest path.
    /// Uses `get_direct_rate` to validate edges, ensuring the best available rate is used per hop.
    pub fn convert_amount(
        &self,
        amount: Decimal,
        from_currency: &str,
        to_currency: &str,
        date: NaiveDate,
    ) -> Result<Decimal, FxError> {
        self.convert_selected(amount, from_currency, to_currency, date, false)
            .map(|(converted, _)| converted)
    }

    /// Resolve the same native path and nearest dates while retaining original
    /// forward observations, including when an edge uses an inverse rate.
    pub fn convert_with_sources(
        &self,
        amount: Decimal,
        from_currency: &str,
        to_currency: &str,
        date: NaiveDate,
    ) -> Result<(Decimal, Vec<ExchangeRate>), FxError> {
        self.convert_selected(amount, from_currency, to_currency, date, true)
    }

    fn convert_selected(
        &self,
        amount: Decimal,
        from_currency: &str,
        to_currency: &str,
        date: NaiveDate,
        retain_sources: bool,
    ) -> Result<(Decimal, Vec<ExchangeRate>), FxError> {
        if from_currency == to_currency {
            return Ok((amount, vec![]));
        }

        // BFS State: (Current Currency, Accumulated Rate)
        let mut queue: VecDeque<(String, Decimal, Vec<ExchangeRate>)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();

        queue.push_back((from_currency.to_string(), Decimal::ONE, vec![]));
        visited.insert(from_currency.to_string());

        while let Some((current_curr, current_rate, sources)) = queue.pop_front() {
            if current_curr == to_currency {
                return amount
                    .checked_mul(current_rate)
                    .map(|value| (value, sources))
                    .ok_or_else(|| {
                        FxError::RateNotFound(
                            "FX conversion is outside the supported decimal range".into(),
                        )
                    });
            }

            if let Some(neighbors) = self.adj.get(&current_curr) {
                // A repeated initialization of the same facts must choose the
                // same equal-length route, including its source diagnostics.
                let mut neighbors: Vec<_> = neighbors.iter().collect();
                neighbors.sort();
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        // Check if a rate exists nearby for this specific edge
                        if let Some((rate, source)) =
                            self.get_direct_rate(&current_curr, neighbor, date)
                        {
                            let next_rate = current_rate.checked_mul(*rate).ok_or_else(|| {
                                FxError::RateNotFound(
                                    "FX path is outside the supported decimal range".into(),
                                )
                            })?;
                            let mut next_sources = sources.clone();
                            if retain_sources {
                                next_sources.push(source.as_ref().clone());
                            }
                            visited.insert(neighbor.clone());
                            queue.push_back((neighbor.clone(), next_rate, next_sources));
                        }
                    }
                }
            }
        }

        Err(FxError::RateNotFound(format!(
            "No conversion path found for {} -> {} on or near {}",
            from_currency, to_currency, date
        )))
    }

    /// Alias for backward compatibility. Functionally identical to `convert_amount`
    /// as strict vs nearest logic is handled inside `get_direct_rate`.
    pub fn convert_amount_nearest(
        &self,
        amount: Decimal,
        from_currency: &str,
        to_currency: &str,
        date: NaiveDate,
    ) -> Result<Decimal, FxError> {
        self.convert_amount(amount, from_currency, to_currency, date)
    }

    /// Helper for single unit conversion (nearest).
    pub fn get_rate_nearest(
        &self,
        from_currency: &str,
        to_currency: &str,
        date: NaiveDate,
    ) -> Result<Decimal, FxError> {
        self.convert_amount(Decimal::ONE, from_currency, to_currency, date)
    }

    /// Helper for single unit conversion (standard).
    pub fn get_rate(
        &self,
        from_currency: &str,
        to_currency: &str,
        date: NaiveDate,
    ) -> Result<Decimal, FxError> {
        self.convert_amount(Decimal::ONE, from_currency, to_currency, date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};

    fn make_rate(from: &str, to: &str, rate: f64, y: i32, m: u32, d: u32) -> ExchangeRate {
        let date_time = Utc.from_utc_datetime(
            &NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
        );

        ExchangeRate {
            id: format!("{}-{}", from, to),
            from_currency: from.to_string(),
            to_currency: to.to_string(),
            rate: Decimal::from_f64_retain(rate).unwrap(),
            source: "MANUAL".to_string(),
            timestamp: date_time,
        }
    }

    #[test]
    fn test_exact_date_match() {
        let rates = vec![make_rate("USD", "EUR", 0.90, 2023, 10, 25)];
        let converter = CurrencyConverter::new(rates).unwrap();
        let date = NaiveDate::from_ymd_opt(2023, 10, 25).unwrap();

        let rate = converter.get_rate("USD", "EUR", date).unwrap();
        assert_eq!(rate, Decimal::from_f64_retain(0.90).unwrap());
    }

    #[test]
    fn equal_length_routes_keep_the_same_values_and_original_sources_after_reload() {
        let rates = vec![
            make_rate("EUR", "USD", 2.0, 2023, 10, 25),
            make_rate("USD", "CNY", 3.0, 2023, 10, 25),
            make_rate("EUR", "AUD", 3.0, 2023, 10, 25),
            make_rate("AUD", "CNY", 5.0, 2023, 10, 25),
        ];
        let date = NaiveDate::from_ymd_opt(2023, 10, 25).unwrap();
        for reverse in [false, true] {
            for _ in 0..16 {
                let mut input = rates.clone();
                if reverse {
                    input.reverse();
                }
                let converter = CurrencyConverter::new(input).unwrap();
                let (value, sources) = converter
                    .convert_with_sources(Decimal::ONE, "EUR", "CNY", date)
                    .unwrap();
                assert_eq!(value, Decimal::from(15));
                assert_eq!(
                    sources
                        .iter()
                        .map(|source| source.id.as_str())
                        .collect::<Vec<_>>(),
                    ["EUR-AUD", "AUD-CNY"]
                );
                assert_eq!(
                    converter
                        .convert_amount(Decimal::ONE, "EUR", "CNY", date)
                        .unwrap(),
                    value
                );
            }
        }
    }

    #[test]
    fn test_nearest_future_is_closer() {
        // Target: 2023-10-27
        // Option A: 2023-10-20 (7 days past)
        // Option B: 2023-10-30 (3 days future) -> Should Pick B
        let rates = vec![
            make_rate("GBP", "GBX", 100.0, 2023, 10, 20),
            make_rate("GBP", "GBX", 101.0, 2023, 10, 30),
        ];
        let converter = CurrencyConverter::new(rates).unwrap();

        let date = NaiveDate::from_ymd_opt(2023, 10, 27).unwrap();
        let rate = converter.get_rate("GBP", "GBX", date).unwrap();
        assert_eq!(rate, Decimal::from(101));
    }

    #[test]
    fn test_nearest_past_is_closer() {
        // Target: 2023-10-22
        // Option A: 2023-10-20 (2 days past) -> Should Pick A
        // Option B: 2023-10-30 (8 days future)
        let rates = vec![
            make_rate("GBP", "GBX", 100.0, 2023, 10, 20),
            make_rate("GBP", "GBX", 101.0, 2023, 10, 30),
        ];
        let converter = CurrencyConverter::new(rates).unwrap();

        let date = NaiveDate::from_ymd_opt(2023, 10, 22).unwrap();
        let rate = converter.get_rate("GBP", "GBX", date).unwrap();
        assert_eq!(rate, Decimal::from(100));
    }

    #[test]
    fn test_single_static_rate_works_anywhere() {
        let rates = vec![make_rate("GBP", "GBX", 100.0, 2023, 6, 15)];
        let converter = CurrencyConverter::new(rates).unwrap();

        // 1. Way Past
        let r1 = converter
            .get_rate("GBP", "GBX", NaiveDate::from_ymd_opt(2000, 1, 1).unwrap())
            .unwrap();
        assert_eq!(r1, Decimal::from(100));

        // 2. Way Future
        let r2 = converter
            .get_rate("GBP", "GBX", NaiveDate::from_ymd_opt(2050, 1, 1).unwrap())
            .unwrap();
        assert_eq!(r2, Decimal::from(100));
    }
}
