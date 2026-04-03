use anyhow::Result;
use chrono::Utc;

use crate::models::InboxItem;
use crate::repository::ConversationRepository;

impl ConversationRepository {
    /// Upsert an inbox item for an instance.
    /// - If a row exists with the same event_type == "completed_turn", increments turn_count.
    /// - Otherwise replaces the row (higher priority events win).
    pub async fn upsert_inbox_item(
        &self,
        instance_id: &str,
        event_type: &str,
        metadata_json: Option<&str>,
    ) -> Result<InboxItem> {
        let now = Utc::now().timestamp();

        // Try to find existing row
        let existing: Option<(String, i32)> = sqlx::query_as(
            "SELECT event_type, turn_count FROM instance_inbox WHERE instance_id = ?",
        )
        .bind(instance_id)
        .fetch_optional(&self.pool)
        .await?;

        match existing {
            Some((existing_type, turn_count))
                if existing_type == "completed_turn" && event_type == "completed_turn" =>
            {
                // Same type: increment turn_count
                sqlx::query(
                    "UPDATE instance_inbox SET turn_count = ?, updated_at = ?, metadata_json = COALESCE(?, metadata_json) WHERE instance_id = ?",
                )
                .bind(turn_count + 1)
                .bind(now)
                .bind(metadata_json)
                .bind(instance_id)
                .execute(&self.pool)
                .await?;

                Ok(InboxItem {
                    instance_id: instance_id.to_string(),
                    event_type: event_type.to_string(),
                    turn_count: turn_count + 1,
                    created_at: now, // Will be actual created_at from DB but close enough
                    updated_at: now,
                    metadata_json: metadata_json.map(String::from),
                })
            }
            _ => {
                // Replace (or insert new)
                sqlx::query(
                    r#"INSERT INTO instance_inbox (instance_id, event_type, turn_count, created_at, updated_at, metadata_json)
                    VALUES (?, ?, 1, ?, ?, ?)
                    ON CONFLICT(instance_id) DO UPDATE SET
                        event_type = excluded.event_type,
                        turn_count = 1,
                        created_at = excluded.created_at,
                        updated_at = excluded.updated_at,
                        metadata_json = excluded.metadata_json"#,
                )
                .bind(instance_id)
                .bind(event_type)
                .bind(now)
                .bind(now)
                .bind(metadata_json)
                .execute(&self.pool)
                .await?;

                Ok(InboxItem {
                    instance_id: instance_id.to_string(),
                    event_type: event_type.to_string(),
                    turn_count: 1,
                    created_at: now,
                    updated_at: now,
                    metadata_json: metadata_json.map(String::from),
                })
            }
        }
    }

    /// List all active inbox items.
    pub async fn list_inbox(&self) -> Result<Vec<InboxItem>> {
        let rows: Vec<(String, String, i32, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT instance_id, event_type, turn_count, created_at, updated_at, metadata_json FROM instance_inbox ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(instance_id, event_type, turn_count, created_at, updated_at, metadata_json)| {
                    InboxItem {
                        instance_id,
                        event_type,
                        turn_count,
                        created_at,
                        updated_at,
                        metadata_json,
                    }
                },
            )
            .collect())
    }

    /// Dismiss (delete) an inbox item. Returns true if a row was deleted.
    pub async fn dismiss_inbox_item(&self, instance_id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM instance_inbox WHERE instance_id = ?")
            .bind(instance_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Clear inbox item only if it matches the given event_type.
    /// Returns true if a row was deleted.
    pub async fn clear_inbox_by_type(&self, instance_id: &str, event_type: &str) -> Result<bool> {
        let result =
            sqlx::query("DELETE FROM instance_inbox WHERE instance_id = ? AND event_type = ?")
                .bind(instance_id)
                .bind(event_type)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }
}
