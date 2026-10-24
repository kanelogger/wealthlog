use chrono::{Days, NaiveDate};

use crate::errors::{Error, Result, ValidationError};

pub const SNAPSHOT_READ_FUTURE_GRACE_DAYS: u64 = 2;

pub fn min_supported_snapshot_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("1970-01-01 is a valid date")
}

pub fn max_snapshot_read_date(today: NaiveDate) -> NaiveDate {
    today
        .checked_add_days(Days::new(SNAPSHOT_READ_FUTURE_GRACE_DAYS))
        .unwrap_or(NaiveDate::MAX)
}

pub fn snapshot_date_requires_remediation(date: NaiveDate, today: NaiveDate) -> bool {
    date < min_supported_snapshot_date() || date > max_snapshot_read_date(today)
}

pub fn snapshot_recalculation_start_after_delete(
    date: NaiveDate,
    today: NaiveDate,
) -> Option<NaiveDate> {
    (!snapshot_date_requires_remediation(date, today)).then_some(date)
}

pub fn validate_snapshot_write_date(
    account_id: &str,
    date: NaiveDate,
    source: &str,
    today: NaiveDate,
) -> Result<()> {
    validate_snapshot_date(
        account_id,
        date,
        source,
        min_supported_snapshot_date(),
        today,
    )
}

pub fn validate_snapshot_read_date(
    account_id: &str,
    date: NaiveDate,
    source: &str,
    today: NaiveDate,
) -> Result<()> {
    validate_snapshot_date(
        account_id,
        date,
        source,
        min_supported_snapshot_date(),
        max_snapshot_read_date(today),
    )
}

fn validate_snapshot_date(
    account_id: &str,
    date: NaiveDate,
    source: &str,
    min_date: NaiveDate,
    max_date: NaiveDate,
) -> Result<()> {
    if date < min_date || date > max_date {
        return Err(Error::Validation(ValidationError::InvalidSnapshotDate {
            account_id: account_id.to_string(),
            date,
            min_date,
            max_date,
            snapshot_source: source.to_string(),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::utils::time_utils::user_date_from_utc;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn write_policy_uses_fixed_inclusive_floor_and_today_ceiling() {
        let today = date(2026, 8, 4);
        assert!(validate_snapshot_write_date("a", date(1970, 1, 1), "CSV_IMPORT", today).is_ok());
        assert!(validate_snapshot_write_date("a", today, "CSV_IMPORT", today).is_ok());
        assert!(
            validate_snapshot_write_date("a", date(1969, 12, 31), "CSV_IMPORT", today).is_err()
        );
        assert!(validate_snapshot_write_date("a", date(2026, 8, 5), "CSV_IMPORT", today).is_err());
    }

    #[test]
    fn read_policy_allows_two_day_timezone_grace() {
        let today = date(2026, 8, 4);
        assert!(
            validate_snapshot_read_date("a", date(2026, 8, 6), "BROKER_IMPORTED", today).is_ok()
        );
        assert!(
            validate_snapshot_read_date("a", date(2026, 8, 7), "BROKER_IMPORTED", today).is_err()
        );
        assert!(!snapshot_date_requires_remediation(date(2026, 8, 6), today));
        assert!(snapshot_date_requires_remediation(date(2026, 8, 7), today));
    }

    #[test]
    fn fixed_floor_does_not_age_after_acceptance() {
        let boundary = date(1970, 1, 1);
        assert!(
            validate_snapshot_write_date("a", boundary, "MANUAL_ENTRY", date(2026, 8, 4)).is_ok()
        );
        assert!(
            validate_snapshot_read_date("a", boundary, "MANUAL_ENTRY", date(2086, 8, 4)).is_ok()
        );
    }

    #[test]
    fn invalid_snapshot_delete_requires_full_rebuild() {
        let today = date(2026, 8, 4);
        assert_eq!(
            snapshot_recalculation_start_after_delete(date(2024, 7, 20), today),
            Some(date(2024, 7, 20))
        );
        assert_eq!(
            snapshot_recalculation_start_after_delete(date(224, 7, 20), today),
            None
        );
        assert_eq!(
            snapshot_recalculation_start_after_delete(date(2026, 8, 7), today),
            None
        );
    }

    #[test]
    fn auckland_write_is_readable_from_los_angeles_at_the_same_instant() {
        let instant = Utc.with_ymd_and_hms(2026, 8, 4, 12, 30, 0).unwrap();
        let auckland_today = user_date_from_utc(instant, chrono_tz::Pacific::Auckland);
        let los_angeles_today = user_date_from_utc(instant, chrono_tz::America::Los_Angeles);

        assert_eq!(auckland_today, date(2026, 8, 5));
        assert_eq!(los_angeles_today, date(2026, 8, 4));
        assert!(validate_snapshot_read_date(
            "a",
            auckland_today,
            "MANUAL_ENTRY",
            los_angeles_today,
        )
        .is_ok());
    }
}
