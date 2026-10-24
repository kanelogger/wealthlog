use anyhow::Result;
use async_trait::async_trait;

use super::model::{CategorizationRule, NewCategorizationRule, UpdateCategorizationRule};
use super::service::CategorizationRulesService;

/// Surface of `CategorizationRulesService` consumed by agent tools.
/// Mirrors the inherent method signatures exactly.
#[async_trait]
pub trait CategorizationRulesServiceTrait: Send + Sync {
    async fn list(&self) -> Result<Vec<CategorizationRule>>;
    async fn create(&self, new_rule: NewCategorizationRule) -> Result<CategorizationRule>;
}

#[async_trait]
impl CategorizationRulesServiceTrait for CategorizationRulesService {
    async fn list(&self) -> Result<Vec<CategorizationRule>> {
        CategorizationRulesService::list(self).await
    }

    async fn create(&self, new_rule: NewCategorizationRule) -> Result<CategorizationRule> {
        CategorizationRulesService::create(self, new_rule).await
    }
}

#[async_trait]
pub trait CategorizationRulesRepositoryTrait: Send + Sync {
    async fn list(&self) -> Result<Vec<CategorizationRule>>;
    async fn get(&self, id: &str) -> Result<Option<CategorizationRule>>;
    async fn create(&self, new_rule: NewCategorizationRule) -> Result<CategorizationRule>;
    async fn update(&self, id: &str, patch: UpdateCategorizationRule)
        -> Result<CategorizationRule>;
    /// Atomically create-or-update the rule identified by `new_rule.id` (required).
    /// Runs the existence check and the write in the same transaction, so two
    /// concurrent upserts of the same id can't both take the create branch.
    async fn upsert(&self, new_rule: NewCategorizationRule) -> Result<CategorizationRule>;
    async fn delete(&self, id: &str) -> Result<()>;
}
