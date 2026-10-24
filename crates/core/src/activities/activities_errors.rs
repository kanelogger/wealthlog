use diesel::result::Error as DieselError;
use thiserror::Error;

/// Custom error type for activity-related operations
#[derive(Debug, Error)]
pub enum ActivityError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid data: {0}")]
    InvalidData(String),
    /// A create hit an existing activity with the same dedupe identity.
    ///
    /// The display text is byte-for-byte the old `InvalidData` message so
    /// every existing entry point keeps its response body and IPC string.
    #[error(
        "Invalid data: Duplicate activity detected. A matching activity already exists{}.",
        existing_activity_id.as_ref().map(|id| format!(" (id: {id})")).unwrap_or_default()
    )]
    DuplicateActivity {
        existing_activity_id: Option<String>,
    },
    #[error("Asset error: {0}")]
    AssetError(String),
    #[error("Currency exchange error: {0}")]
    CurrencyExchangeError(String),
}

impl ActivityError {
    /// Build the typed duplicate error, optionally carrying the existing id.
    pub fn duplicate(existing_activity_id: Option<&str>) -> Self {
        Self::DuplicateActivity {
            existing_activity_id: existing_activity_id.map(str::to_string),
        }
    }

    /// The existing activity id when this is a duplicate error.
    pub fn duplicate_existing_activity_id(&self) -> Option<&str> {
        match self {
            Self::DuplicateActivity {
                existing_activity_id,
            } => existing_activity_id.as_deref(),
            _ => None,
        }
    }
}

impl From<DieselError> for ActivityError {
    fn from(err: DieselError) -> Self {
        match err {
            DieselError::NotFound => ActivityError::NotFound("Record not found".to_string()),
            _ => ActivityError::DatabaseError(err.to_string()),
        }
    }
}

impl From<ActivityError> for String {
    fn from(error: ActivityError) -> Self {
        error.to_string()
    }
}

impl From<ActivityError> for diesel::result::Error {
    fn from(err: ActivityError) -> Self {
        // Convert ActivityError to a diesel error
        // Using DatabaseError as it's the most appropriate for general errors
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::SerializationFailure,
            Box::new(format!("{}", err)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ActivityError;

    #[test]
    fn duplicate_display_matches_the_legacy_text_with_and_without_id() {
        let with_id = ActivityError::duplicate(Some("act-1"));
        assert_eq!(
            with_id.to_string(),
            "Invalid data: Duplicate activity detected. A matching activity already exists (id: act-1)."
        );
        assert_eq!(with_id.duplicate_existing_activity_id(), Some("act-1"));

        let without_id = ActivityError::duplicate(None);
        assert_eq!(
            without_id.to_string(),
            "Invalid data: Duplicate activity detected. A matching activity already exists."
        );
        assert_eq!(without_id.duplicate_existing_activity_id(), None);
    }
}
