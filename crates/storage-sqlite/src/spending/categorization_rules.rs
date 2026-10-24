//! Storage adapter for spending::categorization_rules.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::db::{get_connection, DbPool, WriteHandle};
use crate::errors::StorageError;
use crate::schema::spending_categorization_rules;
use wealthfolio_spending::categorization_rules::{
    CategorizationRule, CategorizationRulesRepositoryTrait, NewCategorizationRule, RuleAmountOp,
    RuleMatchType, UpdateCategorizationRule,
};

#[derive(Queryable, Identifiable, Selectable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::spending_categorization_rules)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[serde(rename_all = "camelCase")]
pub struct CategorizationRuleDB {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub match_type: String,
    pub taxonomy_id: Option<String>,
    pub category_id: Option<String>,
    pub activity_type: Option<String>,
    pub priority: i32,
    pub is_global: i32,
    pub account_id: Option<String>,
    pub preset_id: Option<String>,
    pub preset_rule_key: Option<String>,
    pub preset_version: Option<String>,
    pub preset_modified: i32,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_op: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_value2: Option<String>,
}

#[derive(Insertable, AsChangeset, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::spending_categorization_rules)]
pub struct NewCategorizationRuleDB {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub match_type: String,
    pub taxonomy_id: Option<String>,
    pub category_id: Option<String>,
    pub activity_type: Option<String>,
    pub priority: i32,
    pub is_global: i32,
    pub account_id: Option<String>,
    pub preset_id: Option<String>,
    pub preset_rule_key: Option<String>,
    pub preset_version: Option<String>,
    pub preset_modified: i32,
    pub created_at: String,
    pub updated_at: String,
    pub amount_op: Option<String>,
    pub amount_value: Option<String>,
    pub amount_value2: Option<String>,
}

fn parse_db_decimal(value: &Option<String>, rule_id: &str, column: &str) -> Option<Decimal> {
    value.as_deref().and_then(|s| match Decimal::from_str(s) {
        Ok(d) => Some(d),
        Err(err) => {
            log::warn!("Categorization rule {rule_id} has invalid {column} '{s}': {err}");
            None
        }
    })
}

fn parse_dt(s: &str) -> NaiveDateTime {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.naive_utc())
        .unwrap_or_else(|_| chrono::Utc::now().naive_utc())
}

impl From<CategorizationRuleDB> for CategorizationRule {
    fn from(db: CategorizationRuleDB) -> Self {
        let amount_op = db.amount_op.as_deref().and_then(|s| {
            let op = RuleAmountOp::try_parse(s);
            if op.is_none() {
                log::warn!("Categorization rule {} has unknown amount_op '{s}'", db.id);
            }
            op
        });
        Self {
            amount_op,
            amount_value: parse_db_decimal(&db.amount_value, &db.id, "amount_value"),
            amount_value2: parse_db_decimal(&db.amount_value2, &db.id, "amount_value2"),
            id: db.id,
            name: db.name,
            pattern: db.pattern,
            match_type: RuleMatchType::parse(&db.match_type),
            taxonomy_id: db.taxonomy_id,
            category_id: db.category_id,
            activity_type: db.activity_type,
            priority: db.priority,
            is_global: db.is_global != 0,
            account_id: db.account_id,
            preset_id: db.preset_id,
            preset_rule_key: db.preset_rule_key,
            preset_version: db.preset_version,
            preset_modified: db.preset_modified != 0,
            created_at: parse_dt(&db.created_at),
            updated_at: parse_dt(&db.updated_at),
        }
    }
}

pub struct CategorizationRulesRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}

impl CategorizationRulesRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        Self { pool, writer }
    }
}

fn new_rule_db(new_rule: NewCategorizationRule, now: &str) -> NewCategorizationRuleDB {
    let NewCategorizationRule {
        id,
        name,
        pattern,
        match_type,
        taxonomy_id,
        category_id,
        activity_type,
        amount_op,
        amount_value,
        amount_value2,
        priority,
        is_global,
        account_id,
    } = new_rule;
    let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());

    NewCategorizationRuleDB {
        id,
        name,
        pattern,
        match_type: match_type.as_str().to_string(),
        taxonomy_id,
        category_id,
        activity_type,
        priority,
        is_global: if is_global { 1 } else { 0 },
        account_id,
        preset_id: None,
        preset_rule_key: None,
        preset_version: None,
        preset_modified: 0,
        created_at: now.to_string(),
        updated_at: now.to_string(),
        amount_op: amount_op.map(|op| op.as_str().to_string()),
        amount_value: amount_value.map(|d| d.to_string()),
        amount_value2: amount_value2.map(|d| d.to_string()),
    }
}

#[async_trait]
impl CategorizationRulesRepositoryTrait for CategorizationRulesRepository {
    async fn list(&self) -> Result<Vec<CategorizationRule>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let rows = spending_categorization_rules::table
            .order((
                spending_categorization_rules::priority.desc(),
                spending_categorization_rules::created_at.asc(),
                spending_categorization_rules::id.asc(),
            ))
            .load::<CategorizationRuleDB>(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get(&self, id: &str) -> Result<Option<CategorizationRule>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let row = spending_categorization_rules::table
            .find(id)
            .first::<CategorizationRuleDB>(&mut conn)
            .optional()
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(row.map(Into::into))
    }

    async fn create(&self, new_rule: NewCategorizationRule) -> Result<CategorizationRule> {
        let now = chrono::Utc::now().to_rfc3339();
        let row = new_rule_db(new_rule, &now);
        self.writer
            .exec_tx(move |tx| {
                let inserted = diesel::insert_into(spending_categorization_rules::table)
                    .values(&row)
                    .returning(CategorizationRuleDB::as_returning())
                    .get_result(tx.conn())
                    .map_err(StorageError::from)?;

                Ok(inserted)
            })
            .await
            .map(CategorizationRule::from)
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn update(
        &self,
        id: &str,
        patch: UpdateCategorizationRule,
    ) -> Result<CategorizationRule> {
        let id = id.to_string();

        self.writer
            .exec_tx(move |tx| {
                let mut existing: CategorizationRuleDB = spending_categorization_rules::table
                    .find(&id)
                    .first::<CategorizationRuleDB>(tx.conn())
                    .map_err(StorageError::from)?;
                if let Some(v) = patch.name {
                    existing.name = v;
                }
                if let Some(v) = patch.pattern {
                    existing.pattern = v;
                }
                if let Some(v) = patch.match_type {
                    existing.match_type = v.as_str().to_string();
                }
                if let Some(v) = patch.taxonomy_id {
                    existing.taxonomy_id = v;
                }
                if let Some(v) = patch.category_id {
                    existing.category_id = v;
                }
                if let Some(v) = patch.activity_type {
                    existing.activity_type = v;
                }
                if let Some(v) = patch.amount_op {
                    existing.amount_op = v.map(|op| op.as_str().to_string());
                }
                if let Some(v) = patch.amount_value {
                    existing.amount_value = v.map(|d| d.to_string());
                }
                if let Some(v) = patch.amount_value2 {
                    existing.amount_value2 = v.map(|d| d.to_string());
                }
                if let Some(v) = patch.priority {
                    existing.priority = v;
                }
                if let Some(v) = patch.is_global {
                    existing.is_global = if v { 1 } else { 0 };
                }
                if let Some(v) = patch.account_id {
                    existing.account_id = v;
                }
                // If this rule came from a preset, mark it as user-modified so
                // future preset updates can ask before overwriting.
                if existing.preset_id.is_some() {
                    existing.preset_modified = 1;
                }
                existing.updated_at = chrono::Utc::now().to_rfc3339();

                diesel::update(spending_categorization_rules::table.find(&id))
                    .set((
                        spending_categorization_rules::name.eq(&existing.name),
                        spending_categorization_rules::pattern.eq(&existing.pattern),
                        spending_categorization_rules::match_type.eq(&existing.match_type),
                        spending_categorization_rules::taxonomy_id.eq(&existing.taxonomy_id),
                        spending_categorization_rules::category_id.eq(&existing.category_id),
                        spending_categorization_rules::activity_type.eq(&existing.activity_type),
                        spending_categorization_rules::amount_op.eq(&existing.amount_op),
                        spending_categorization_rules::amount_value.eq(&existing.amount_value),
                        spending_categorization_rules::amount_value2.eq(&existing.amount_value2),
                        spending_categorization_rules::priority.eq(existing.priority),
                        spending_categorization_rules::is_global.eq(existing.is_global),
                        spending_categorization_rules::account_id.eq(&existing.account_id),
                        spending_categorization_rules::preset_modified.eq(existing.preset_modified),
                        spending_categorization_rules::updated_at.eq(&existing.updated_at),
                    ))
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;

                Ok(existing)
            })
            .await
            .map(CategorizationRule::from)
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn upsert(&self, new_rule: NewCategorizationRule) -> Result<CategorizationRule> {
        let id = new_rule
            .id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("upsert requires an explicit rule id"))?;
        let now = chrono::Utc::now().to_rfc3339();
        self.writer
            .exec_tx(move |tx| {
                let existing: Option<CategorizationRuleDB> = spending_categorization_rules::table
                    .find(&id)
                    .first::<CategorizationRuleDB>(tx.conn())
                    .optional()
                    .map_err(StorageError::from)?;

                match existing {
                    None => {
                        let row = new_rule_db(new_rule, &now);
                        let inserted = diesel::insert_into(spending_categorization_rules::table)
                            .values(&row)
                            .returning(CategorizationRuleDB::as_returning())
                            .get_result(tx.conn())
                            .map_err(StorageError::from)?;

                        Ok(inserted)
                    }
                    Some(mut existing) => {
                        existing.name = new_rule.name;
                        existing.pattern = new_rule.pattern;
                        existing.match_type = new_rule.match_type.as_str().to_string();
                        existing.taxonomy_id = new_rule.taxonomy_id;
                        existing.category_id = new_rule.category_id;
                        existing.activity_type = new_rule.activity_type;
                        existing.amount_op = new_rule.amount_op.map(|op| op.as_str().to_string());
                        existing.amount_value = new_rule.amount_value.map(|d| d.to_string());
                        existing.amount_value2 = new_rule.amount_value2.map(|d| d.to_string());
                        existing.priority = new_rule.priority;
                        existing.is_global = if new_rule.is_global { 1 } else { 0 };
                        existing.account_id = new_rule.account_id;
                        existing.updated_at = now.clone();

                        diesel::update(spending_categorization_rules::table.find(&existing.id))
                            .set((
                                spending_categorization_rules::name.eq(&existing.name),
                                spending_categorization_rules::pattern.eq(&existing.pattern),
                                spending_categorization_rules::match_type.eq(&existing.match_type),
                                spending_categorization_rules::taxonomy_id
                                    .eq(&existing.taxonomy_id),
                                spending_categorization_rules::category_id
                                    .eq(&existing.category_id),
                                spending_categorization_rules::activity_type
                                    .eq(&existing.activity_type),
                                spending_categorization_rules::amount_op.eq(&existing.amount_op),
                                spending_categorization_rules::amount_value
                                    .eq(&existing.amount_value),
                                spending_categorization_rules::amount_value2
                                    .eq(&existing.amount_value2),
                                spending_categorization_rules::priority.eq(existing.priority),
                                spending_categorization_rules::is_global.eq(existing.is_global),
                                spending_categorization_rules::account_id.eq(&existing.account_id),
                                spending_categorization_rules::updated_at.eq(&existing.updated_at),
                            ))
                            .execute(tx.conn())
                            .map_err(StorageError::from)?;

                        Ok(existing)
                    }
                }
            })
            .await
            .map(CategorizationRule::from)
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.writer
            .exec_tx(move |tx| {
                diesel::delete(spending_categorization_rules::table.find(&id))
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_pool, init, run_migrations, write_actor::spawn_writer};
    use tempfile::tempdir;

    fn setup_repo() -> CategorizationRulesRepository {
        let app_data = tempdir()
            .expect("tempdir")
            .keep()
            .to_string_lossy()
            .to_string();
        let db_path = init(&app_data).expect("init db");
        run_migrations(&db_path).expect("migrate db");
        let pool = create_pool(&db_path).expect("create pool");
        let writer = spawn_writer(pool.as_ref().clone()).expect("writer");
        CategorizationRulesRepository::new(pool, writer)
    }

    fn preset_rule() -> NewCategorizationRule {
        NewCategorizationRule {
            id: Some("rule-ca-groceries".to_string()),
            name: "Groceries".to_string(),
            pattern: "grocery".to_string(),
            match_type: RuleMatchType::Contains,
            taxonomy_id: None,
            category_id: None,
            activity_type: None,
            amount_op: None,
            amount_value: None,
            amount_value2: None,
            priority: 0,
            is_global: true,
            account_id: None,
        }
    }

    #[tokio::test]
    async fn amount_condition_round_trips_and_updates() {
        use wealthfolio_spending::categorization_rules::RuleAmountOp;

        let repo = setup_repo();
        let mut new_rule = preset_rule();
        new_rule.id = Some("rule-amount".to_string());
        new_rule.amount_op = Some(RuleAmountOp::Between);
        new_rule.amount_value = Some("45.50".parse().unwrap());
        new_rule.amount_value2 = Some("55".parse().unwrap());

        let created = repo.create(new_rule).await.expect("create rule");
        assert_eq!(created.amount_op, Some(RuleAmountOp::Between));
        assert_eq!(created.amount_value, Some("45.50".parse().unwrap()));
        assert_eq!(created.amount_value2, Some("55".parse().unwrap()));

        let fetched = repo.get("rule-amount").await.expect("get").expect("some");
        assert_eq!(fetched.amount_op, Some(RuleAmountOp::Between));
        assert_eq!(fetched.amount_value, Some("45.50".parse().unwrap()));
        assert_eq!(fetched.amount_value2, Some("55".parse().unwrap()));

        // Patch to a single-value operator: upper value cleared explicitly.
        let updated = repo
            .update(
                "rule-amount",
                UpdateCategorizationRule {
                    amount_op: Some(Some(RuleAmountOp::Gte)),
                    amount_value: Some(Some("100".parse().unwrap())),
                    amount_value2: Some(None),
                    ..Default::default()
                },
            )
            .await
            .expect("update rule");
        assert_eq!(updated.amount_op, Some(RuleAmountOp::Gte));
        assert_eq!(updated.amount_value, Some("100".parse().unwrap()));
        assert_eq!(updated.amount_value2, None);

        // Patch that omits the amount fields leaves the condition untouched.
        let untouched = repo
            .update(
                "rule-amount",
                UpdateCategorizationRule {
                    name: Some("renamed".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("update rule");
        assert_eq!(untouched.name, "renamed");
        assert_eq!(untouched.amount_op, Some(RuleAmountOp::Gte));
        assert_eq!(untouched.amount_value, Some("100".parse().unwrap()));

        // Clearing the whole condition round-trips to NULLs.
        let cleared = repo
            .update(
                "rule-amount",
                UpdateCategorizationRule {
                    amount_op: Some(None),
                    amount_value: Some(None),
                    amount_value2: Some(None),
                    ..Default::default()
                },
            )
            .await
            .expect("update rule");
        assert_eq!(cleared.amount_op, None);
        assert_eq!(cleared.amount_value, None);
        assert_eq!(cleared.amount_value2, None);
    }

    #[tokio::test]
    async fn upsert_creates_then_updates_the_same_row_without_duplicating() {
        let repo = setup_repo();
        let rule_id = "addon:test-addon:abc123".to_string();
        let new_rule = |name: &str| NewCategorizationRule {
            id: Some(rule_id.clone()),
            name: name.to_string(),
            pattern: "SUPERMARKET".to_string(),
            match_type: RuleMatchType::Contains,
            taxonomy_id: None,
            category_id: None,
            activity_type: Some("WITHDRAWAL".to_string()),
            amount_op: None,
            amount_value: None,
            amount_value2: None,
            priority: 0,
            is_global: true,
            account_id: None,
        };

        let created = repo
            .upsert(new_rule("Groceries"))
            .await
            .expect("first upsert creates");
        assert_eq!(created.id, rule_id);
        assert_eq!(created.name, "Groceries");

        // A second upsert with the same id must update the existing row in
        // place, not collide on the primary key (the bug this replaces) or
        // create a duplicate row.
        let updated = repo
            .upsert(new_rule("Groceries (renamed)"))
            .await
            .expect("second upsert updates in place");
        assert_eq!(updated.id, rule_id);
        assert_eq!(updated.name, "Groceries (renamed)");

        let all = repo.list().await.expect("list rules");
        assert_eq!(all.iter().filter(|r| r.id == rule_id).count(), 1);
    }
}
