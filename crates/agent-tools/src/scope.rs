//! Agent scopes — the enforced permission model for agent tool access.
//!
//! Scope names combine a business category with a read, draft or write action.
//! Only scopes that gate shipped tools are defined; add new variants when
//! the tools that need them ship.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A single granted or required permission scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentScope {
    AccountsRead,
    HoldingsRead,
    PerformanceRead,
    ActivitiesRead,
    HealthRead,
    ClassificationRead,
    ActivitiesDraft,
    ActivitiesWrite,
    OrdinaryWrite,
    ClassificationSuggest,
    ClassificationWrite,
}

impl AgentScope {
    /// Every scope, in stable display order (read scopes first, then the
    /// draft/write/suggest scopes).
    pub const ALL: &'static [AgentScope] = &[
        AgentScope::AccountsRead,
        AgentScope::HoldingsRead,
        AgentScope::PerformanceRead,
        AgentScope::ActivitiesRead,
        AgentScope::HealthRead,
        AgentScope::ClassificationRead,
        AgentScope::ActivitiesDraft,
        AgentScope::ActivitiesWrite,
        AgentScope::OrdinaryWrite,
        AgentScope::ClassificationSuggest,
        AgentScope::ClassificationWrite,
    ];

    /// The read-only scopes — what the `read-only` preset grants. Kept
    /// separate from [`AgentScope::ALL`] so adding a write scope never
    /// silently widens read-only tokens.
    pub const READ_SCOPES: &'static [AgentScope] = &[
        AgentScope::AccountsRead,
        AgentScope::HoldingsRead,
        AgentScope::PerformanceRead,
        AgentScope::ActivitiesRead,
        AgentScope::HealthRead,
        AgentScope::ClassificationRead,
    ];

    /// Canonical wire format, e.g. `"accounts:read"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentScope::AccountsRead => "accounts:read",
            AgentScope::HoldingsRead => "holdings:read",
            AgentScope::PerformanceRead => "performance:read",
            AgentScope::ActivitiesRead => "activities:read",
            AgentScope::HealthRead => "health:read",
            AgentScope::ClassificationRead => "classification:read",
            AgentScope::ActivitiesDraft => "activities:draft",
            AgentScope::ActivitiesWrite => "activities:write",
            AgentScope::OrdinaryWrite => "ordinary:write",
            AgentScope::ClassificationSuggest => "classification:suggest",
            AgentScope::ClassificationWrite => "classification:write",
        }
    }

    /// Parse the canonical wire format. Unknown scopes return `None`;
    /// callers decide whether unknown means "ignore" (forward compat for
    /// tokens minted by a newer version) or "reject".
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|scope| scope.as_str() == s)
    }
}

impl std::fmt::Display for AgentScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for AgentScope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentScope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        AgentScope::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown agent scope: {s}")))
    }
}

/// A set of granted scopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentScopeSet(BTreeSet<AgentScope>);

impl AgentScopeSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// The read-only preset: every defined read scope.
    pub fn read_only() -> Self {
        Self(AgentScope::READ_SCOPES.iter().copied().collect())
    }

    /// Read-only preset plus the ability to prepare (but not commit)
    /// activity drafts.
    pub fn read_activity_draft() -> Self {
        let mut set = Self::read_only();
        set.insert(AgentScope::ActivitiesDraft);
        set
    }

    /// Read-only preset plus drafting activities and classification
    /// suggestions.
    ///
    /// Formal writes are disabled for MCP this release, so no preset grants
    /// the write scopes.
    pub fn read_activity_draft_classification_suggest() -> Self {
        let mut set = Self::read_activity_draft();
        set.insert(AgentScope::ClassificationSuggest);
        set
    }

    /// Build from canonical scope strings, silently skipping unknown ones
    /// (forward compatibility with scopes minted by a newer version). Used on
    /// the auth path.
    pub fn from_strs<'a>(scopes: impl IntoIterator<Item = &'a str>) -> Self {
        Self(scopes.into_iter().filter_map(AgentScope::parse).collect())
    }

    pub fn insert(&mut self, scope: AgentScope) {
        self.0.insert(scope);
    }

    pub fn contains(&self, scope: AgentScope) -> bool {
        self.0.contains(&scope)
    }

    /// True when every scope in `required` is granted.
    pub fn grants_all(&self, required: &[AgentScope]) -> bool {
        required.iter().all(|scope| self.0.contains(scope))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = AgentScope> + '_ {
        self.0.iter().copied()
    }

    /// Validate scope dependencies. Returns a human-readable error when a
    /// granted scope is missing a prerequisite: committing requires the
    /// matching draft/suggest scope.
    pub fn dependency_error(&self) -> Option<String> {
        if self.contains(AgentScope::ActivitiesWrite) && !self.contains(AgentScope::ActivitiesDraft)
        {
            return Some(
                "activities:write requires activities:draft (commit needs a draft)".to_string(),
            );
        }
        if self.contains(AgentScope::OrdinaryWrite)
            && (!self.contains(AgentScope::ActivitiesRead)
                || !self.contains(AgentScope::AccountsRead))
        {
            return Some("ordinary:write requires activities:read and accounts:read".to_string());
        }
        if self.contains(AgentScope::ClassificationWrite)
            && !self.contains(AgentScope::ClassificationSuggest)
        {
            return Some(
                "classification:write requires classification:suggest (commit needs a draft)"
                    .to_string(),
            );
        }
        None
    }
}

impl FromIterator<AgentScope> for AgentScopeSet {
    fn from_iter<I: IntoIterator<Item = AgentScope>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_canonical_strings() {
        for scope in AgentScope::ALL {
            assert_eq!(AgentScope::parse(scope.as_str()), Some(*scope));
        }
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(AgentScope::parse("accounts:write"), None);
        assert_eq!(AgentScope::parse(""), None);
        assert_eq!(AgentScope::parse("financial-planning:read"), None);
    }

    #[test]
    fn from_strs_skips_unknown_and_retired_scopes() {
        let set = AgentScopeSet::from_strs([
            "accounts:read",
            "not-a-scope",
            "financial-planning:read",
            "holdings:read",
        ]);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![AgentScope::AccountsRead, AgentScope::HoldingsRead]
        );
        assert!(AgentScopeSet::from_strs(["financial-planning:read"]).is_empty());
    }

    #[test]
    fn read_only_excludes_write_and_suggest_scopes() {
        let set = AgentScopeSet::read_only();
        assert!(set.contains(AgentScope::HoldingsRead));
        assert!(!set.contains(AgentScope::ActivitiesDraft));
        assert!(!set.contains(AgentScope::ActivitiesWrite));
        assert!(!set.contains(AgentScope::ClassificationSuggest));
        assert!(!set.contains(AgentScope::ClassificationWrite));
    }

    #[test]
    fn presets_layer_correctly() {
        assert!(AgentScopeSet::read_activity_draft().contains(AgentScope::ActivitiesDraft));
        assert!(!AgentScopeSet::read_activity_draft().contains(AgentScope::ActivitiesWrite));

        let suggested = AgentScopeSet::read_activity_draft_classification_suggest();
        assert!(suggested.contains(AgentScope::ActivitiesDraft));
        assert!(suggested.contains(AgentScope::ClassificationSuggest));
        // No preset grants a formal-write scope while MCP writes are disabled.
        assert!(!suggested.contains(AgentScope::ActivitiesWrite));
        assert!(!suggested.contains(AgentScope::ClassificationWrite));
    }

    #[test]
    fn ordinary_write_requires_read_scopes() {
        let mut set = AgentScopeSet::from_strs(["ordinary:write"]);
        assert!(set.dependency_error().is_some());
        set.insert(AgentScope::ActivitiesRead);
        assert!(set.dependency_error().is_some());
        set.insert(AgentScope::AccountsRead);
        assert!(set.dependency_error().is_none());
        assert!(!AgentScopeSet::read_only().contains(AgentScope::OrdinaryWrite));
    }

    #[test]
    fn grants_all_requires_every_scope() {
        let set = AgentScopeSet::from_strs(["accounts:read"]);
        assert!(set.grants_all(&[AgentScope::AccountsRead]));
        assert!(!set.grants_all(&[AgentScope::AccountsRead, AgentScope::HoldingsRead]));
        assert!(set.grants_all(&[]));
    }

    #[test]
    fn dependency_error_flags_write_without_matching_draft_scope() {
        let mut set = AgentScopeSet::read_only();
        set.insert(AgentScope::ActivitiesWrite);
        assert!(set.dependency_error().is_some());

        set.insert(AgentScope::ActivitiesDraft);
        assert!(set.dependency_error().is_none());

        set.insert(AgentScope::ClassificationWrite);
        assert!(set.dependency_error().is_some());

        set.insert(AgentScope::ClassificationSuggest);
        assert!(set.dependency_error().is_none());
    }
}
