//! Activity rules — pattern-based auto-categorization for activities.
//! On activity create, the rule with the highest priority that matches `notes`
//! (and optionally `activity_type`) wins; its category assignment is created.

pub mod matcher;
pub mod model;
pub mod service;
pub mod traits;

pub use matcher::{compile_regex_pattern, match_rules, RuleMatch, MAX_REGEX_PATTERN_LEN};
pub use model::{
    CategorizationRule, NewCategorizationRule, RuleAmountOp, RuleMatchType,
    UpdateCategorizationRule,
};
pub use service::CategorizationRulesService;
pub use traits::{CategorizationRulesRepositoryTrait, CategorizationRulesServiceTrait};
