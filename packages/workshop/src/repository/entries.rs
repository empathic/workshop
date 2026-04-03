use anyhow::Result;
use sqlx::Row;
use tracing::debug;

use crate::models::ConversationEntry;

use super::ConversationRepository;

impl ConversationRepository {
    pub async fn add_entries_batch(&self, entries: &[ConversationEntry]) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        for entry in entries {
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO conversation_entries
                (conversation_id, entry_uuid, parent_uuid, entry_type, role, content, timestamp, raw_json, token_count, model)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#
            )
            .bind(&entry.conversation_id)
            .bind(&entry.entry_uuid)
            .bind(&entry.parent_uuid)
            .bind(&entry.entry_type)
            .bind(&entry.role)
            .bind(&entry.content)
            .bind(&entry.timestamp)
            .bind(&entry.raw_json)
            .bind(entry.token_count)
            .bind(&entry.model)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        debug!("Added {} entries in batch", entries.len());
        Ok(())
    }

    pub async fn get_conversation_entries(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<ConversationEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT id, conversation_id, entry_uuid, parent_uuid, entry_type,
                   role, content, timestamp, raw_json, token_count, model
            FROM conversation_entries
            WHERE conversation_id = ?
            ORDER BY timestamp ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ConversationEntry {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                entry_uuid: row.get("entry_uuid"),
                parent_uuid: row.get("parent_uuid"),
                entry_type: row.get("entry_type"),
                role: row.get("role"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                raw_json: row.get("raw_json"),
                token_count: row.get("token_count"),
                model: row.get("model"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{Conversation, ConversationEntry};
    use crate::repository::test_helpers;

    fn make_entry(conv_id: &str, uuid: &str, timestamp: &str) -> ConversationEntry {
        ConversationEntry {
            id: None,
            conversation_id: conv_id.to_string(),
            entry_uuid: uuid.to_string(),
            parent_uuid: None,
            entry_type: "message".to_string(),
            role: Some("user".to_string()),
            content: Some(format!("content for {}", uuid)),
            timestamp: timestamp.to_string(),
            raw_json: "{}".to_string(),
            token_count: None,
            model: None,
        }
    }

    #[tokio::test]
    async fn batch_insert_and_retrieve() {
        let repo = test_helpers::test_repository().await;
        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![
            make_entry("conv-1", "e-1", "2024-01-01T00:00:00Z"),
            make_entry("conv-1", "e-2", "2024-01-01T00:01:00Z"),
            make_entry("conv-1", "e-3", "2024-01-01T00:02:00Z"),
        ];
        repo.add_entries_batch(&entries).await.unwrap();

        let fetched = repo.get_conversation_entries("conv-1").await.unwrap();
        assert_eq!(fetched.len(), 3);
        assert_eq!(fetched[0].entry_uuid, "e-1");
        assert_eq!(fetched[2].entry_uuid, "e-3");
    }

    #[tokio::test]
    async fn entries_ordered_by_timestamp() {
        let repo = test_helpers::test_repository().await;
        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        // Insert out of order
        let entries = vec![
            make_entry("conv-1", "e-late", "2024-01-01T00:05:00Z"),
            make_entry("conv-1", "e-early", "2024-01-01T00:01:00Z"),
            make_entry("conv-1", "e-mid", "2024-01-01T00:03:00Z"),
        ];
        repo.add_entries_batch(&entries).await.unwrap();

        let fetched = repo.get_conversation_entries("conv-1").await.unwrap();
        assert_eq!(fetched[0].entry_uuid, "e-early");
        assert_eq!(fetched[1].entry_uuid, "e-mid");
        assert_eq!(fetched[2].entry_uuid, "e-late");
    }

    #[tokio::test]
    async fn duplicate_uuid_ignored() {
        let repo = test_helpers::test_repository().await;
        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![make_entry("conv-1", "e-1", "2024-01-01T00:00:00Z")];
        repo.add_entries_batch(&entries).await.unwrap();
        // Insert same UUID again
        repo.add_entries_batch(&entries).await.unwrap();

        let fetched = repo.get_conversation_entries("conv-1").await.unwrap();
        assert_eq!(fetched.len(), 1);
    }

    #[tokio::test]
    async fn entries_isolated_by_conversation() {
        let repo = test_helpers::test_repository().await;
        let c1 = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        let c2 = Conversation::new("conv-2".to_string(), "inst-1".to_string());
        repo.create_conversation(&c1).await.unwrap();
        repo.create_conversation(&c2).await.unwrap();

        repo.add_entries_batch(&[make_entry("conv-1", "e-1", "2024-01-01T00:00:00Z")])
            .await
            .unwrap();
        repo.add_entries_batch(&[
            make_entry("conv-2", "e-2", "2024-01-01T00:00:00Z"),
            make_entry("conv-2", "e-3", "2024-01-01T00:01:00Z"),
        ])
        .await
        .unwrap();

        let c1_entries = repo.get_conversation_entries("conv-1").await.unwrap();
        let c2_entries = repo.get_conversation_entries("conv-2").await.unwrap();
        assert_eq!(c1_entries.len(), 1);
        assert_eq!(c2_entries.len(), 2);
    }

    #[tokio::test]
    async fn empty_batch_is_noop() {
        let repo = test_helpers::test_repository().await;
        repo.add_entries_batch(&[]).await.unwrap();
    }

    #[tokio::test]
    async fn entry_fields_roundtrip() {
        let repo = test_helpers::test_repository().await;
        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        let entry = ConversationEntry {
            id: None,
            conversation_id: "conv-1".to_string(),
            entry_uuid: "uuid-abc".to_string(),
            parent_uuid: Some("uuid-parent".to_string()),
            entry_type: "tool_use".to_string(),
            role: Some("assistant".to_string()),
            content: Some("result data".to_string()),
            timestamp: "2024-06-15T12:30:00Z".to_string(),
            raw_json: r#"{"key":"value"}"#.to_string(),
            token_count: Some(42),
            model: Some("claude-3".to_string()),
        };
        repo.add_entries_batch(&[entry]).await.unwrap();

        let fetched = repo.get_conversation_entries("conv-1").await.unwrap();
        let e = &fetched[0];
        assert!(e.id.is_some());
        assert_eq!(e.entry_uuid, "uuid-abc");
        assert_eq!(e.parent_uuid, Some("uuid-parent".to_string()));
        assert_eq!(e.entry_type, "tool_use");
        assert_eq!(e.role, Some("assistant".to_string()));
        assert_eq!(e.content, Some("result data".to_string()));
        assert_eq!(e.raw_json, r#"{"key":"value"}"#);
        assert_eq!(e.token_count, Some(42));
        assert_eq!(e.model, Some("claude-3".to_string()));
    }
}
