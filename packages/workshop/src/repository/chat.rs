use anyhow::{Context, Result};
use sqlx::Row;

use crate::models::{ChatMessage, ChatTopicSummary};

use super::ConversationRepository;

impl ConversationRepository {
    pub async fn insert_chat_message(&self, msg: &ChatMessage) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO chat_messages (uuid, scope, user_id, display_name, content, created_at, forwarded_from, topic)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&msg.uuid)
        .bind(&msg.scope)
        .bind(&msg.user_id)
        .bind(&msg.display_name)
        .bind(&msg.content)
        .bind(msg.created_at)
        .bind(&msg.forwarded_from)
        .bind(&msg.topic)
        .execute(&self.pool)
        .await
        .context("Failed to insert chat message")?;

        Ok(result.last_insert_rowid())
    }

    /// Get paginated chat history for a scope, ordered newest-first.
    /// Optionally filter by topic. Returns (messages_oldest_first, has_more).
    pub async fn get_chat_history(
        &self,
        scope: &str,
        before_id: Option<i64>,
        limit: i64,
        topic: Option<&str>,
    ) -> Result<(Vec<ChatMessage>, bool)> {
        // Fetch limit+1 to detect whether there are more pages
        let fetch_limit = limit + 1;

        // Build query dynamically based on topic filter
        let (topic_clause, has_topic) = match topic {
            Some(_) => (" AND topic = ?", true),
            None => ("", false),
        };

        let rows = if let Some(bid) = before_id {
            let sql = format!(
                r#"
                SELECT id, uuid, scope, user_id, display_name, content, created_at, forwarded_from, topic
                FROM chat_messages
                WHERE scope = ? AND id < ?{}
                ORDER BY id DESC
                LIMIT ?
                "#,
                topic_clause
            );
            let mut q = sqlx::query(&sql).bind(scope).bind(bid);
            if has_topic {
                q = q.bind(topic.unwrap());
            }
            q.bind(fetch_limit).fetch_all(&self.pool).await?
        } else {
            let sql = format!(
                r#"
                SELECT id, uuid, scope, user_id, display_name, content, created_at, forwarded_from, topic
                FROM chat_messages
                WHERE scope = ?{}
                ORDER BY id DESC
                LIMIT ?
                "#,
                topic_clause
            );
            let mut q = sqlx::query(&sql).bind(scope);
            if has_topic {
                q = q.bind(topic.unwrap());
            }
            q.bind(fetch_limit).fetch_all(&self.pool).await?
        };

        let has_more = rows.len() as i64 > limit;
        let mut messages: Vec<ChatMessage> = rows
            .into_iter()
            .take(limit as usize)
            .map(|r| ChatMessage {
                id: r.get("id"),
                uuid: r.get("uuid"),
                scope: r.get("scope"),
                user_id: r.get("user_id"),
                display_name: r.get("display_name"),
                content: r.get("content"),
                created_at: r.get("created_at"),
                forwarded_from: r.get("forwarded_from"),
                topic: r.get("topic"),
            })
            .collect();

        // Reverse so oldest is first (natural reading order)
        messages.reverse();

        Ok((messages, has_more))
    }

    pub async fn get_chat_message_by_id(&self, id: i64) -> Result<Option<ChatMessage>> {
        let row = sqlx::query(
            r#"
            SELECT id, uuid, scope, user_id, display_name, content, created_at, forwarded_from, topic
            FROM chat_messages
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| ChatMessage {
            id: r.get("id"),
            uuid: r.get("uuid"),
            scope: r.get("scope"),
            user_id: r.get("user_id"),
            display_name: r.get("display_name"),
            content: r.get("content"),
            created_at: r.get("created_at"),
            forwarded_from: r.get("forwarded_from"),
            topic: r.get("topic"),
        }))
    }

    /// Get list of topics for a scope with message counts.
    pub async fn get_chat_topics(&self, scope: &str) -> Result<Vec<ChatTopicSummary>> {
        let rows = sqlx::query(
            r#"
            SELECT topic, COUNT(*) as message_count, MAX(created_at) as latest_at
            FROM chat_messages
            WHERE scope = ? AND topic IS NOT NULL
            GROUP BY topic
            ORDER BY latest_at DESC
            "#,
        )
        .bind(scope)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ChatTopicSummary {
                topic: r.get("topic"),
                message_count: r.get("message_count"),
                latest_at: r.get("latest_at"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{ChatMessage, User};
    use crate::repository::test_helpers;
    use chrono::Utc;

    fn make_user(id: &str, username: &str) -> User {
        let now = Utc::now().timestamp();
        User {
            id: id.to_string(),
            username: username.to_string(),
            display_name: username.to_string(),
            password_hash: "hashed".to_string(),
            is_admin: false,
            is_disabled: false,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_msg(scope: &str, user_id: &str, content: &str, topic: Option<&str>) -> ChatMessage {
        ChatMessage {
            id: None,
            uuid: uuid::Uuid::new_v4().to_string(),
            scope: scope.to_string(),
            user_id: user_id.to_string(),
            display_name: "Test User".to_string(),
            content: content.to_string(),
            created_at: Utc::now().timestamp(),
            forwarded_from: None,
            topic: topic.map(String::from),
        }
    }

    #[tokio::test]
    async fn insert_and_get_by_id() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        let msg = make_msg("global", "u-1", "hello world", None);
        let id = repo.insert_chat_message(&msg).await.unwrap();
        assert!(id > 0);

        let fetched = repo.get_chat_message_by_id(id).await.unwrap().unwrap();
        assert_eq!(fetched.content, "hello world");
        assert_eq!(fetched.scope, "global");
    }

    #[tokio::test]
    async fn chat_history_ordering() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        repo.insert_chat_message(&make_msg("global", "u-1", "first", None))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("global", "u-1", "second", None))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("global", "u-1", "third", None))
            .await
            .unwrap();

        let (msgs, has_more) = repo
            .get_chat_history("global", None, 10, None)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 3);
        assert!(!has_more);
        // Should be oldest-first
        assert_eq!(msgs[0].content, "first");
        assert_eq!(msgs[2].content, "third");
    }

    #[tokio::test]
    async fn chat_history_pagination() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        for i in 0..5 {
            repo.insert_chat_message(&make_msg("global", "u-1", &format!("msg {}", i), None))
                .await
                .unwrap();
        }

        // Get latest 2
        let (msgs, has_more) = repo
            .get_chat_history("global", None, 2, None)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(has_more);
        assert_eq!(msgs[0].content, "msg 3");
        assert_eq!(msgs[1].content, "msg 4");

        // Get next page using before_id
        let before = msgs[0].id.unwrap();
        let (older, _) = repo
            .get_chat_history("global", Some(before), 2, None)
            .await
            .unwrap();
        assert_eq!(older.len(), 2);
        assert_eq!(older[0].content, "msg 1");
        assert_eq!(older[1].content, "msg 2");
    }

    #[tokio::test]
    async fn chat_scope_isolation() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        repo.insert_chat_message(&make_msg("global", "u-1", "global msg", None))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("inst-1", "u-1", "instance msg", None))
            .await
            .unwrap();

        let (global, _) = repo
            .get_chat_history("global", None, 10, None)
            .await
            .unwrap();
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].content, "global msg");

        let (inst, _) = repo
            .get_chat_history("inst-1", None, 10, None)
            .await
            .unwrap();
        assert_eq!(inst.len(), 1);
        assert_eq!(inst[0].content, "instance msg");
    }

    #[tokio::test]
    async fn chat_topic_filter() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        repo.insert_chat_message(&make_msg("global", "u-1", "no topic", None))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("global", "u-1", "topic a msg", Some("topic-a")))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("global", "u-1", "topic b msg", Some("topic-b")))
            .await
            .unwrap();

        let (filtered, _) = repo
            .get_chat_history("global", None, 10, Some("topic-a"))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].content, "topic a msg");
    }

    #[tokio::test]
    async fn chat_topics_listing() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        repo.insert_chat_message(&make_msg("global", "u-1", "msg1", Some("bugs")))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("global", "u-1", "msg2", Some("bugs")))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("global", "u-1", "msg3", Some("features")))
            .await
            .unwrap();
        repo.insert_chat_message(&make_msg("global", "u-1", "no topic", None))
            .await
            .unwrap();

        let topics = repo.get_chat_topics("global").await.unwrap();
        assert_eq!(topics.len(), 2);
        // Topics with message counts
        let bugs = topics.iter().find(|t| t.topic == "bugs").unwrap();
        assert_eq!(bugs.message_count, 2);
        let features = topics.iter().find(|t| t.topic == "features").unwrap();
        assert_eq!(features.message_count, 1);
    }
}
