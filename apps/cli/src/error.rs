use serde_json::Value;

/// Exit codes are part of the CLI contract.
pub const EXIT_OK: u8 = 0;
pub const EXIT_INTERNAL: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_AUTH: u8 = 3;
pub const EXIT_CONFLICT: u8 = 4;
pub const EXIT_UNAVAILABLE: u8 = 5;
pub const EXIT_LOCKED: u8 = 6;

#[derive(Debug, Clone)]
pub struct CliError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub request_id: Option<String>,
    pub request_key: Option<String>,
    pub ledger_id: Option<String>,
    pub data: Value,
    pub exit: u8,
}

impl CliError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        exit: u8,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
            request_id: None,
            request_key: None,
            ledger_id: None,
            data: Value::Null,
            exit,
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message, false, EXIT_USAGE)
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::new("unauthorized", message, false, EXIT_AUTH)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new("service_unavailable", message, true, EXIT_UNAVAILABLE)
    }

    pub fn locked(message: impl Into<String>) -> Self {
        Self::new("profile_locked", message, false, EXIT_LOCKED)
    }

    /// The requested ledger is not the one the running instance has active.
    /// Keep the protocol code so callers branch on `profile_mismatch` rather
    /// than a generic usage error; the structured data names both ledgers.
    pub fn profile_mismatch(
        message: impl Into<String>,
        active_ledger_id: Option<&str>,
        requested_ledger_id: &str,
    ) -> Self {
        let mut error = Self::new("profile_mismatch", message, false, EXIT_USAGE);
        error.data = serde_json::json!({
            "expectedLedgerId": active_ledger_id,
            "receivedLedgerId": requested_ledger_id,
        });
        error
    }

    pub fn unknown(message: impl Into<String>) -> Self {
        Self::new("result_unknown", message, true, EXIT_UNAVAILABLE)
    }

    pub fn with_request_key(mut self, request_key: Option<String>) -> Self {
        self.request_key = request_key;
        self
    }

    pub fn with_ledger(mut self, ledger_id: impl Into<String>) -> Self {
        self.ledger_id = Some(ledger_id.into());
        self
    }

    pub fn value(&self) -> Value {
        serde_json::json!({
            "code": self.code,
            "message": self.message,
            "retryable": self.retryable,
            "requestId": self.request_id,
            "data": self.data,
        })
    }
}

/// Map a structured tool/business error to an exit code.
pub fn exit_for_code(code: &str) -> u8 {
    match code {
        "duplicate_activity" | "request_conflict" | "version_conflict" | "request_in_progress" => {
            EXIT_CONFLICT
        }
        "profile_locked" => EXIT_LOCKED,
        "service_unavailable" | "result_unknown" => EXIT_UNAVAILABLE,
        "unauthorized"
        | "scope_denied"
        | "forbidden"
        | "authentication_failed"
        | "write_blocked" => EXIT_AUTH,
        "invalid_input"
        | "invalid_amount"
        | "activity_out_of_scope"
        | "activity_not_found"
        | "tool_not_found"
        | "profile_required"
        | "profile_mismatch" => EXIT_USAGE,
        "storage_error" | "execution_failed" | "internal_error" => EXIT_INTERNAL,
        _ => EXIT_INTERNAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_mismatch_keeps_code_and_named_ledgers() {
        let error = CliError::profile_mismatch(
            "wrong ledger",
            Some("11111111-1111-1111-1111-111111111111"),
            "22222222-2222-2222-2222-222222222222",
        );
        assert_eq!(error.code, "profile_mismatch");
        assert_eq!(error.exit, EXIT_USAGE);
        assert_eq!(
            error.data["expectedLedgerId"],
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(
            error.data["receivedLedgerId"],
            "22222222-2222-2222-2222-222222222222"
        );
        assert_eq!(exit_for_code(&error.code), EXIT_USAGE);
    }
}
