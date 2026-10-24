//! Request replay and business audit contracts.
//!
//! A request key identifies "this same submit" for one ledger, subject,
//! operation and key. The first submit freezes a snapshot and stores its
//! result; a retry with the same key and content returns that stored result
//! instead of writing again. Same key with different content conflicts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::audit::BusinessAuditEntry;

/// Stored outcome of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome<T> {
    /// First execution for this key.
    Executed(T),
    /// The key was already completed; this is the stored result.
    Replayed(T),
}

impl<T> RequestOutcome<T> {
    pub fn value(&self) -> &T {
        match self {
            RequestOutcome::Executed(value) | RequestOutcome::Replayed(value) => value,
        }
    }

    pub fn is_replay(&self) -> bool {
        matches!(self, RequestOutcome::Replayed(_))
    }

    pub fn into_value(self) -> T {
        match self {
            RequestOutcome::Executed(value) | RequestOutcome::Replayed(value) => value,
        }
    }
}

/// One frozen request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    pub ledger_id: String,
    pub subject: String,
    pub operation: String,
    pub request_key: String,
    /// Stable hash of the canonical request content.
    pub request_hash: String,
    /// The frozen request body, replayed verbatim to detect content drift.
    pub request_snapshot: serde_json::Value,
}

impl RequestRecord {
    pub fn new(
        ledger_id: impl Into<String>,
        subject: impl Into<String>,
        operation: impl Into<String>,
        request_key: impl Into<String>,
        request_snapshot: serde_json::Value,
    ) -> Self {
        let request_snapshot = canonicalize(request_snapshot);
        let request_hash = hash_json(&request_snapshot);
        Self {
            ledger_id: ledger_id.into(),
            subject: subject.into(),
            operation: operation.into(),
            request_key: request_key.into(),
            request_hash,
            request_snapshot,
        }
    }
}

/// What a replay decision tells the caller to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestReplay {
    /// First submission: write the request row, run the use case, store the result.
    Fresh,
    /// Same key and content with a stored result: return it, write nothing.
    Completed(String),
    /// A previous attempt with this key failed before completing; retry it.
    Retry,
    /// Same key, different content.
    Conflict,
}

impl RequestReplay {
    /// Decide from an existing row (if any) for the same key.
    pub fn decide(existing: Option<&crate::store::RequestRow>, request_hash: &str) -> Self {
        match existing {
            None => RequestReplay::Fresh,
            Some(row) if row.request_hash != request_hash => RequestReplay::Conflict,
            Some(row) => match row.state.as_str() {
                "completed" => {
                    RequestReplay::Completed(row.result_json.clone().unwrap_or_default())
                }
                _ => RequestReplay::Retry,
            },
        }
    }
}

/// Canonical JSON so the same logical request always hashes the same way:
/// object keys sorted, no insignificant whitespace.
pub fn canonicalize(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(String, serde_json::Value)> = map
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            serde_json::Value::Object(entries.into_iter().collect())
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonicalize).collect())
        }
        other => other,
    }
}

pub fn hash_json(value: &serde_json::Value) -> String {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(encoded);
    hex::encode(digest)
}

/// One audit write. Kept next to the request types because both are written
/// inside the same business transaction.
pub type AuditWrite = BusinessAuditEntry;
