use anyhow::{Context, Result};
use sqlx::Row;
use tracing::debug;

use crate::models::{
    Comment, Conversation, ConversationShare, ConversationSummary, ConversationWithEntries,
    PaginatedResponse, Tag,
};

use super::ConversationRepository;

impl ConversationRepository {
    pub async fn create_conversation(&self, conv: &Conversation) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO conversations (id, session_id, instance_id, title, created_at, updated_at, is_public, is_deleted, metadata_json, file_hash, file_mtime, import_version)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&conv.id)
        .bind(&conv.session_id)
        .bind(&conv.instance_id)
        .bind(&conv.title)
        .bind(conv.created_at)
        .bind(conv.updated_at)
        .bind(conv.is_public)
        .bind(conv.is_deleted)
        .bind(&conv.metadata_json)
        .bind(&conv.file_hash)
        .bind(conv.file_mtime)
        .bind(conv.import_version)
        .execute(&self.pool)
        .await
        .context("Failed to create conversation")?;

        debug!("Created conversation: {}", conv.id);
        Ok(())
    }

    pub async fn update_conversation_title(&self, id: &str, title: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE conversations SET title = ?, updated_at = ? WHERE id = ?")
            .bind(title)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update conversation title")?;
        Ok(())
    }

    pub async fn get_conversation(&self, id: &str) -> Result<Option<Conversation>> {
        let row = sqlx::query(
            r#"
            SELECT id, session_id, instance_id, title, created_at, updated_at,
                   is_public, is_deleted, metadata_json, file_hash, file_mtime, import_version
            FROM conversations
            WHERE id = ? AND is_deleted = 0
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| Conversation {
            id: row.get("id"),
            session_id: row.get("session_id"),
            instance_id: row.get("instance_id"),
            title: row.get("title"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            is_public: row.get::<i32, _>("is_public") != 0,
            is_deleted: row.get::<i32, _>("is_deleted") != 0,
            metadata_json: row.get("metadata_json"),
            file_hash: row.get("file_hash"),
            file_mtime: row.get("file_mtime"),
            import_version: row.get("import_version"),
        }))
    }

    pub async fn get_conversation_by_session_id(
        &self,
        session_id: &str,
    ) -> Result<Option<Conversation>> {
        let row = sqlx::query(
            r#"
            SELECT id, session_id, instance_id, title, created_at, updated_at,
                   is_public, is_deleted, metadata_json, file_hash, file_mtime, import_version
            FROM conversations
            WHERE session_id = ? AND is_deleted = 0
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| Conversation {
            id: row.get("id"),
            session_id: row.get("session_id"),
            instance_id: row.get("instance_id"),
            title: row.get("title"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            is_public: row.get::<i32, _>("is_public") != 0,
            is_deleted: row.get::<i32, _>("is_deleted") != 0,
            metadata_json: row.get("metadata_json"),
            file_hash: row.get("file_hash"),
            file_mtime: row.get("file_mtime"),
            import_version: row.get("import_version"),
        }))
    }

    pub async fn update_conversation_file_metadata(
        &self,
        session_id: &str,
        file_hash: &str,
        file_mtime: i64,
        import_version: i64,
        updated_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE conversations SET file_hash = ?, file_mtime = ?, import_version = ?, updated_at = ? WHERE session_id = ? AND is_deleted = 0",
        )
        .bind(file_hash)
        .bind(file_mtime)
        .bind(import_version)
        .bind(updated_at)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .context("Failed to update conversation file metadata")?;
        Ok(())
    }

    pub async fn delete_conversation_entries(&self, conversation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM conversation_entries WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete conversation entries")?;
        Ok(())
    }

    // Paginated conversation listing
    pub async fn list_conversations_paginated(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedResponse<ConversationSummary>> {
        let offset = (page - 1) * per_page;

        let count_row =
            sqlx::query("SELECT COUNT(*) as total FROM conversations WHERE is_deleted = 0")
                .fetch_one(&self.pool)
                .await?;
        let total: i64 = count_row.get("total");

        let rows = sqlx::query(
            r#"
            SELECT c.id, c.title, c.instance_id, c.created_at, c.updated_at, c.is_public,
                   COUNT(e.id) as entry_count
            FROM conversations c
            LEFT JOIN conversation_entries e ON c.id = e.conversation_id
            WHERE c.is_deleted = 0
            GROUP BY c.id
            ORDER BY c.updated_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let items = rows
            .into_iter()
            .map(|row| ConversationSummary {
                id: row.get("id"),
                title: row.get("title"),
                instance_id: row.get("instance_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                entry_count: row.get::<i64, _>("entry_count") as i32,
                is_public: row.get::<i32, _>("is_public") != 0,
            })
            .collect();

        let total_pages = if total == 0 {
            1
        } else {
            (total + per_page - 1) / per_page
        };

        Ok(PaginatedResponse {
            items,
            total,
            page,
            per_page,
            total_pages,
        })
    }

    // Full conversation retrieval
    pub async fn get_conversation_with_entries(
        &self,
        id: &str,
    ) -> Result<Option<ConversationWithEntries>> {
        let conversation = match self.get_conversation(id).await? {
            Some(c) => c,
            None => return Ok(None),
        };

        let entries = self.get_conversation_entries(id).await?;
        let comments = self.get_conversation_comments(id).await?;

        // Get tags
        let rows = sqlx::query(
            r#"
            SELECT t.id, t.name, t.color
            FROM tags t
            JOIN conversation_tags ct ON t.id = ct.tag_id
            WHERE ct.conversation_id = ?
            "#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let tags = rows
            .into_iter()
            .map(|row| Tag {
                id: row.get("id"),
                name: row.get("name"),
                color: row.get("color"),
            })
            .collect();

        // Batch-fetch attributions for all entries (for task_id and user info)
        let entry_uuids: Vec<&str> = entries.iter().map(|e| e.entry_uuid.as_str()).collect();
        let attributions = self
            .get_attributions_for_entry_uuids(&entry_uuids)
            .await
            .unwrap_or_default();

        Ok(Some(ConversationWithEntries {
            conversation,
            entries,
            comments,
            tags,
            attributions,
        }))
    }

    // Comment CRUD
    pub async fn add_comment(&self, comment: &Comment) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO comments (conversation_id, entry_uuid, author, content, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&comment.conversation_id)
        .bind(&comment.entry_uuid)
        .bind(&comment.author)
        .bind(&comment.content)
        .bind(comment.created_at)
        .bind(comment.updated_at)
        .execute(&self.pool)
        .await
        .context("Failed to add comment")?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_conversation_comments(&self, conversation_id: &str) -> Result<Vec<Comment>> {
        let rows = sqlx::query(
            r#"
            SELECT id, conversation_id, entry_uuid, author, content, created_at, updated_at
            FROM comments
            WHERE conversation_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Comment {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                entry_uuid: row.get("entry_uuid"),
                author: row.get("author"),
                content: row.get("content"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect())
    }

    // Share CRUD
    pub async fn create_share(&self, share: &ConversationShare) -> Result<String> {
        sqlx::query(
            r#"
            INSERT INTO conversation_shares
            (conversation_id, share_token, title, description, created_at, expires_at, access_count, max_access_count, password_hash)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&share.conversation_id)
        .bind(&share.share_token)
        .bind(&share.title)
        .bind(&share.description)
        .bind(share.created_at)
        .bind(share.expires_at)
        .bind(share.access_count)
        .bind(share.max_access_count)
        .bind(&share.password_hash)
        .execute(&self.pool)
        .await
        .context("Failed to create share")?;

        Ok(share.share_token.clone())
    }

    pub async fn get_share(&self, token: &str) -> Result<Option<ConversationShare>> {
        let row = sqlx::query(
            r#"
            SELECT id, conversation_id, share_token, title, description,
                   created_at, expires_at, access_count, max_access_count, password_hash
            FROM conversation_shares
            WHERE share_token = ?
            "#,
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| ConversationShare {
            id: row.get("id"),
            conversation_id: row.get("conversation_id"),
            share_token: row.get("share_token"),
            title: row.get("title"),
            description: row.get("description"),
            created_at: row.get("created_at"),
            expires_at: row.get("expires_at"),
            access_count: row.get("access_count"),
            max_access_count: row.get("max_access_count"),
            password_hash: row.get("password_hash"),
        }))
    }

    pub async fn increment_share_access(&self, token: &str) -> Result<()> {
        sqlx::query(
            "UPDATE conversation_shares SET access_count = access_count + 1 WHERE share_token = ?",
        )
        .bind(token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{Comment, Conversation, ConversationEntry, ConversationShare};
    use crate::repository::test_helpers;

    fn make_conversation(id: &str, instance_id: &str) -> Conversation {
        let mut conv = Conversation::new(id.to_string(), instance_id.to_string());
        conv.title = Some(format!("Test {}", id));
        conv
    }

    fn make_entry(
        conversation_id: &str,
        uuid: &str,
        role: &str,
        content: &str,
    ) -> ConversationEntry {
        ConversationEntry {
            id: None,
            conversation_id: conversation_id.to_string(),
            entry_uuid: uuid.to_string(),
            parent_uuid: None,
            entry_type: "message".to_string(),
            role: Some(role.to_string()),
            content: Some(content.to_string()),
            timestamp: chrono::Utc::now().to_rfc3339(),
            raw_json: "{}".to_string(),
            token_count: None,
            model: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_conversation() {
        let repo = test_helpers::test_repository().await;
        let conv = make_conversation("conv-1", "inst-1");
        repo.create_conversation(&conv).await.unwrap();

        let fetched = repo.get_conversation("conv-1").await.unwrap().unwrap();
        assert_eq!(fetched.id, "conv-1");
        assert_eq!(fetched.instance_id, "inst-1");
        assert_eq!(fetched.title, Some("Test conv-1".to_string()));
        assert!(!fetched.is_deleted);
    }

    #[tokio::test]
    async fn get_nonexistent_conversation() {
        let repo = test_helpers::test_repository().await;
        let result = repo.get_conversation("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_conversation_by_session_id() {
        let repo = test_helpers::test_repository().await;
        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string())
            .with_session_id("session-abc".to_string());
        repo.create_conversation(&conv).await.unwrap();

        let fetched = repo
            .get_conversation_by_session_id("session-abc")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, "conv-1");
        assert_eq!(fetched.session_id, Some("session-abc".to_string()));
    }

    #[tokio::test]
    async fn update_conversation_title() {
        let repo = test_helpers::test_repository().await;
        let conv = make_conversation("conv-1", "inst-1");
        repo.create_conversation(&conv).await.unwrap();

        repo.update_conversation_title("conv-1", "New Title")
            .await
            .unwrap();

        let fetched = repo.get_conversation("conv-1").await.unwrap().unwrap();
        assert_eq!(fetched.title, Some("New Title".to_string()));
        assert!(fetched.updated_at >= conv.updated_at);
    }

    #[tokio::test]
    async fn list_conversations_paginated() {
        let repo = test_helpers::test_repository().await;
        for i in 0..5 {
            let mut conv = make_conversation(&format!("conv-{}", i), "inst-1");
            conv.created_at = 1000 + i;
            conv.updated_at = 1000 + i;
            repo.create_conversation(&conv).await.unwrap();
        }

        // Page 1, 2 per page
        let page1 = repo.list_conversations_paginated(1, 2).await.unwrap();
        assert_eq!(page1.items.len(), 2);
        assert_eq!(page1.total, 5);
        assert_eq!(page1.total_pages, 3);
        assert_eq!(page1.page, 1);

        // Page 3 (last page, only 1 item)
        let page3 = repo.list_conversations_paginated(3, 2).await.unwrap();
        assert_eq!(page3.items.len(), 1);
    }

    #[tokio::test]
    async fn list_conversations_ordered_by_updated_at() {
        let repo = test_helpers::test_repository().await;
        let mut c1 = make_conversation("conv-old", "inst-1");
        c1.updated_at = 1000;
        let mut c2 = make_conversation("conv-new", "inst-1");
        c2.updated_at = 2000;
        repo.create_conversation(&c1).await.unwrap();
        repo.create_conversation(&c2).await.unwrap();

        let page = repo.list_conversations_paginated(1, 10).await.unwrap();
        assert_eq!(page.items[0].id, "conv-new");
        assert_eq!(page.items[1].id, "conv-old");
    }

    #[tokio::test]
    async fn list_conversations_with_entry_count() {
        let repo = test_helpers::test_repository().await;
        let conv = make_conversation("conv-1", "inst-1");
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![
            make_entry("conv-1", "e-1", "user", "hello"),
            make_entry("conv-1", "e-2", "assistant", "hi there"),
        ];
        repo.add_entries_batch(&entries).await.unwrap();

        let page = repo.list_conversations_paginated(1, 10).await.unwrap();
        assert_eq!(page.items[0].entry_count, 2);
    }

    #[tokio::test]
    async fn get_conversation_with_entries() {
        let repo = test_helpers::test_repository().await;
        let conv = make_conversation("conv-1", "inst-1");
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![
            make_entry("conv-1", "e-1", "user", "hello"),
            make_entry("conv-1", "e-2", "assistant", "world"),
        ];
        repo.add_entries_batch(&entries).await.unwrap();

        let full = repo
            .get_conversation_with_entries("conv-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(full.entries.len(), 2);
        assert_eq!(full.conversation.id, "conv-1");
    }

    #[tokio::test]
    async fn add_and_get_comments() {
        let repo = test_helpers::test_repository().await;
        let conv = make_conversation("conv-1", "inst-1");
        repo.create_conversation(&conv).await.unwrap();

        let comment = Comment::new(
            "conv-1".to_string(),
            "Great conversation!".to_string(),
            Some("alice".to_string()),
            None,
        );
        let id = repo.add_comment(&comment).await.unwrap();
        assert!(id > 0);

        let comments = repo.get_conversation_comments("conv-1").await.unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].author, "alice");
        assert_eq!(comments[0].content, "Great conversation!");
    }

    #[tokio::test]
    async fn share_lifecycle() {
        let repo = test_helpers::test_repository().await;
        let conv = make_conversation("conv-1", "inst-1");
        repo.create_conversation(&conv).await.unwrap();

        let share = ConversationShare::new("conv-1".to_string(), Some(7));
        let token = repo.create_share(&share).await.unwrap();

        let fetched = repo.get_share(&token).await.unwrap().unwrap();
        assert_eq!(fetched.conversation_id, "conv-1");
        assert_eq!(fetched.access_count, 0);

        repo.increment_share_access(&token).await.unwrap();
        repo.increment_share_access(&token).await.unwrap();

        let fetched = repo.get_share(&token).await.unwrap().unwrap();
        assert_eq!(fetched.access_count, 2);
    }

    #[tokio::test]
    async fn update_conversation_file_metadata() {
        let repo = test_helpers::test_repository().await;
        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string())
            .with_session_id("session-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        // Update file metadata
        let now = chrono::Utc::now().timestamp();
        repo.update_conversation_file_metadata("session-1", "abc123hash", 9999, 2, now)
            .await
            .unwrap();

        // Verify
        let fetched = repo.get_conversation("conv-1").await.unwrap().unwrap();
        assert_eq!(fetched.file_hash, Some("abc123hash".to_string()));
        assert_eq!(fetched.file_mtime, Some(9999));
        assert_eq!(fetched.import_version, Some(2));
        assert_eq!(fetched.updated_at, now);
    }

    #[tokio::test]
    async fn update_conversation_file_metadata_noop_for_missing_session() {
        let repo = test_helpers::test_repository().await;
        // Should succeed (SQL UPDATE with no matching rows is not an error)
        repo.update_conversation_file_metadata("nonexistent", "hash", 0, 1, 0)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_conversation_entries() {
        let repo = test_helpers::test_repository().await;
        let conv = make_conversation("conv-1", "inst-1");
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![make_entry("conv-1", "e-1", "user", "hello")];
        repo.add_entries_batch(&entries).await.unwrap();

        let before = repo.get_conversation_entries("conv-1").await.unwrap();
        assert_eq!(before.len(), 1);

        repo.delete_conversation_entries("conv-1").await.unwrap();

        let after = repo.get_conversation_entries("conv-1").await.unwrap();
        assert!(after.is_empty());
    }
}
