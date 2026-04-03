use anyhow::{Context, Result};
use sqlx::Row;

use crate::models::{InputAttribution, attribution_content_matches};

use super::ConversationRepository;

impl ConversationRepository {
    pub async fn record_input_attribution(&self, attr: &InputAttribution) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO input_attributions (instance_id, user_id, display_name, timestamp, entry_uuid, content_preview, task_id)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&attr.instance_id)
        .bind(&attr.user_id)
        .bind(&attr.display_name)
        .bind(attr.timestamp)
        .bind(&attr.entry_uuid)
        .bind(&attr.content_preview)
        .bind(attr.task_id)
        .execute(&self.pool)
        .await
        .context("Failed to record input attribution")?;
        Ok(result.last_insert_rowid())
    }

    pub async fn correlate_attribution(
        &self,
        instance_id: &str,
        entry_uuid: &str,
        timestamp: i64,
        entry_content: Option<&str>,
    ) -> Result<Option<InputAttribution>> {
        // Strategy: content match first (reliable), timestamp fallback (legacy).
        //
        // The timestamp-only path is vulnerable to label swapping when two users
        // type within seconds of each other — the entry with the closest timestamp
        // might match the WRONG attribution. Content matching avoids this entirely.

        // 1. Try content-based match within the time window
        if let Some(content) = entry_content
            && !content.trim().is_empty()
        {
            // Fetch candidates within the time window
            let rows = sqlx::query(
                    r#"
                    SELECT id, instance_id, user_id, display_name, timestamp, entry_uuid, content_preview, task_id
                    FROM input_attributions
                    WHERE instance_id = ? AND entry_uuid IS NULL AND ABS(timestamp - ?) <= 30
                    ORDER BY ABS(timestamp - ?) ASC
                    "#,
                )
                .bind(instance_id)
                .bind(timestamp)
                .bind(timestamp)
                .fetch_all(&self.pool)
                .await?;

            // Find the first row whose content_preview matches
            for r in &rows {
                let preview: Option<String> = r.get("content_preview");
                if let Some(ref preview) = preview
                    && attribution_content_matches(preview, content)
                {
                    let attr_id: i64 = r.get("id");
                    // Claim it
                    sqlx::query("UPDATE input_attributions SET entry_uuid = ? WHERE id = ?")
                        .bind(entry_uuid)
                        .bind(attr_id)
                        .execute(&self.pool)
                        .await?;

                    return Ok(Some(InputAttribution {
                        id: r.get("id"),
                        instance_id: r.get("instance_id"),
                        user_id: r.get("user_id"),
                        display_name: r.get("display_name"),
                        timestamp: r.get("timestamp"),
                        entry_uuid: Some(entry_uuid.to_string()),
                        content_preview: r.get("content_preview"),
                        task_id: r.get("task_id"),
                    }));
                }
            }
        }

        // No timestamp-only fallback. Without content to match against,
        // a closest-timestamp heuristic can swap labels when two users type
        // concurrently, and the UPDATE permanently poisons future lookups.
        // "Unknown" is more honest than "wrong".
        Ok(None)
    }

    /// Look up an attribution that was already correlated to a specific entry UUID.
    pub async fn get_attribution_by_entry_uuid(
        &self,
        entry_uuid: &str,
    ) -> Result<Option<InputAttribution>> {
        let row = sqlx::query(
            r#"
            SELECT id, instance_id, user_id, display_name, timestamp, entry_uuid, content_preview, task_id
            FROM input_attributions
            WHERE entry_uuid = ?
            LIMIT 1
            "#,
        )
        .bind(entry_uuid)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| InputAttribution {
            id: r.get("id"),
            instance_id: r.get("instance_id"),
            user_id: r.get("user_id"),
            display_name: r.get("display_name"),
            timestamp: r.get("timestamp"),
            entry_uuid: r.get("entry_uuid"),
            content_preview: r.get("content_preview"),
            task_id: r.get("task_id"),
        }))
    }

    /// Batch-fetch attributions for a set of entry UUIDs.
    /// Returns only entries that have a correlated attribution (entry_uuid IS NOT NULL).
    pub async fn get_attributions_for_entry_uuids(
        &self,
        entry_uuids: &[&str],
    ) -> Result<Vec<crate::models::EntryAttribution>> {
        if entry_uuids.is_empty() {
            return Ok(vec![]);
        }

        // SQLite doesn't support array binds, so build a comma-separated placeholder list
        let placeholders: Vec<&str> = entry_uuids.iter().map(|_| "?").collect();
        let query = format!(
            "SELECT entry_uuid, user_id, display_name, task_id FROM input_attributions WHERE entry_uuid IN ({})",
            placeholders.join(", ")
        );

        let mut q = sqlx::query(&query);
        for uuid in entry_uuids {
            q = q.bind(uuid);
        }

        let rows = q.fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::models::EntryAttribution {
                entry_uuid: r.get("entry_uuid"),
                user_id: r.get("user_id"),
                display_name: r.get("display_name"),
                task_id: r.get("task_id"),
            })
            .collect())
    }

    /// Get or correlate attribution for an entry.
    /// First checks for an already-correlated attribution (by entry_uuid),
    /// then falls back to content+timestamp correlation for new entries.
    pub async fn get_or_correlate_attribution(
        &self,
        instance_id: &str,
        entry_uuid: &str,
        timestamp: i64,
        entry_content: Option<&str>,
    ) -> Result<Option<InputAttribution>> {
        // Fast path: already correlated
        if let Some(attr) = self.get_attribution_by_entry_uuid(entry_uuid).await? {
            return Ok(Some(attr));
        }
        // Slow path: correlate by content + timestamp
        self.correlate_attribution(instance_id, entry_uuid, timestamp, entry_content)
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{InputAttribution, User};
    use crate::repository::test_helpers;
    use chrono::Utc;

    fn make_user(id: &str) -> User {
        let now = Utc::now().timestamp();
        User {
            id: id.to_string(),
            username: id.to_string(),
            display_name: id.to_string(),
            password_hash: "hashed".to_string(),
            is_admin: false,
            is_disabled: false,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_attribution(
        instance_id: &str,
        user_id: &str,
        content: Option<&str>,
        ts: i64,
    ) -> InputAttribution {
        InputAttribution {
            id: None,
            instance_id: instance_id.to_string(),
            user_id: user_id.to_string(),
            display_name: user_id.to_string(),
            timestamp: ts,
            entry_uuid: None,
            content_preview: content.map(String::from),
            task_id: None,
        }
    }

    #[tokio::test]
    async fn record_and_get_by_entry_uuid() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1")).await.unwrap();
        let now = Utc::now().timestamp();

        let mut attr = make_attribution("inst-1", "u-1", Some("hello world"), now);
        attr.entry_uuid = Some("entry-1".into());
        let id = repo.record_input_attribution(&attr).await.unwrap();
        assert!(id > 0);

        let fetched = repo
            .get_attribution_by_entry_uuid("entry-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.user_id, "u-1");
        assert_eq!(fetched.instance_id, "inst-1");
    }

    #[tokio::test]
    async fn get_attribution_by_entry_uuid_not_found() {
        let repo = test_helpers::test_repository().await;
        let result = repo
            .get_attribution_by_entry_uuid("nonexistent")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn correlate_by_content_match() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        let now = Utc::now().timestamp();

        // Record an uncorrelated attribution (no entry_uuid)
        let attr = make_attribution("inst-1", "alice", Some("fix the bug in auth"), now);
        repo.record_input_attribution(&attr).await.unwrap();

        // Correlate using matching content
        let result = repo
            .correlate_attribution("inst-1", "entry-abc", now + 2, Some("fix the bug in auth"))
            .await
            .unwrap();

        let matched = result.unwrap();
        assert_eq!(matched.user_id, "alice");
        assert_eq!(matched.entry_uuid, Some("entry-abc".to_string()));
    }

    #[tokio::test]
    async fn correlate_no_match_without_content() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        let now = Utc::now().timestamp();

        // Record an attribution
        let attr = make_attribution("inst-1", "alice", Some("hello"), now);
        repo.record_input_attribution(&attr).await.unwrap();

        // Attempt to correlate without content — should return None (no timestamp-only fallback)
        let result = repo
            .correlate_attribution("inst-1", "entry-abc", now, None)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn correlate_no_match_empty_content() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        let now = Utc::now().timestamp();

        let attr = make_attribution("inst-1", "alice", Some("hello"), now);
        repo.record_input_attribution(&attr).await.unwrap();

        let result = repo
            .correlate_attribution("inst-1", "entry-abc", now, Some("   "))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn correlate_outside_time_window() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        let now = Utc::now().timestamp();

        let attr = make_attribution("inst-1", "alice", Some("hello world"), now);
        repo.record_input_attribution(&attr).await.unwrap();

        // 60 seconds later — outside 30s window
        let result = repo
            .correlate_attribution("inst-1", "entry-abc", now + 60, Some("hello world"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn correlate_wrong_instance() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        let now = Utc::now().timestamp();

        let attr = make_attribution("inst-1", "alice", Some("hello"), now);
        repo.record_input_attribution(&attr).await.unwrap();

        let result = repo
            .correlate_attribution("inst-OTHER", "entry-abc", now, Some("hello"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn correlate_already_claimed_not_reused() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        let now = Utc::now().timestamp();

        let attr = make_attribution("inst-1", "alice", Some("hello world"), now);
        repo.record_input_attribution(&attr).await.unwrap();

        // First correlation claims it
        let first = repo
            .correlate_attribution("inst-1", "entry-1", now, Some("hello world"))
            .await
            .unwrap();
        assert!(first.is_some());

        // Second correlation with same content should NOT find it (already claimed)
        let second = repo
            .correlate_attribution("inst-1", "entry-2", now, Some("hello world"))
            .await
            .unwrap();
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn correlate_picks_correct_content_when_multiple_candidates() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        repo.create_user(&make_user("bob")).await.unwrap();
        let now = Utc::now().timestamp();

        // Two attributions from different users at similar times
        let attr_alice = make_attribution("inst-1", "alice", Some("hello from alice"), now);
        let attr_bob = make_attribution("inst-1", "bob", Some("hello from bob"), now + 1);
        repo.record_input_attribution(&attr_alice).await.unwrap();
        repo.record_input_attribution(&attr_bob).await.unwrap();

        // Correlate with bob's content — should match bob, not alice
        let result = repo
            .correlate_attribution("inst-1", "entry-1", now + 1, Some("hello from bob"))
            .await
            .unwrap();
        let matched = result.unwrap();
        assert_eq!(matched.user_id, "bob");
    }

    #[tokio::test]
    async fn batch_get_attributions() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        repo.create_user(&make_user("bob")).await.unwrap();
        repo.create_user(&make_user("carol")).await.unwrap();
        let now = Utc::now().timestamp();

        let mut a1 = make_attribution("inst-1", "alice", Some("msg1"), now);
        a1.entry_uuid = Some("entry-1".to_string());
        let mut a2 = make_attribution("inst-1", "bob", Some("msg2"), now);
        a2.entry_uuid = Some("entry-2".to_string());
        let mut a3 = make_attribution("inst-1", "carol", Some("msg3"), now);
        a3.entry_uuid = Some("entry-3".to_string());
        repo.record_input_attribution(&a1).await.unwrap();
        repo.record_input_attribution(&a2).await.unwrap();
        repo.record_input_attribution(&a3).await.unwrap();

        let results = repo
            .get_attributions_for_entry_uuids(&["entry-1", "entry-3"])
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        let user_ids: Vec<&str> = results.iter().map(|a| a.user_id.as_str()).collect();
        assert!(user_ids.contains(&"alice"));
        assert!(user_ids.contains(&"carol"));
    }

    #[tokio::test]
    async fn batch_get_empty_input() {
        let repo = test_helpers::test_repository().await;
        let results = repo.get_attributions_for_entry_uuids(&[]).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn get_or_correlate_uses_cached_first() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("alice")).await.unwrap();
        let now = Utc::now().timestamp();

        // Record an already-correlated attribution
        let mut attr = make_attribution("inst-1", "alice", Some("cached"), now);
        attr.entry_uuid = Some("entry-1".to_string());
        repo.record_input_attribution(&attr).await.unwrap();

        // get_or_correlate should return the cached one immediately
        let result = repo
            .get_or_correlate_attribution("inst-1", "entry-1", now, Some("cached"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.user_id, "alice");
    }

    #[tokio::test]
    async fn get_or_correlate_falls_back_to_correlation() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("bob")).await.unwrap();
        let now = Utc::now().timestamp();

        // Record uncorrelated attribution
        let attr = make_attribution("inst-1", "bob", Some("new message"), now);
        repo.record_input_attribution(&attr).await.unwrap();

        // get_or_correlate should fall back to correlation
        let result = repo
            .get_or_correlate_attribution("inst-1", "entry-new", now, Some("new message"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.user_id, "bob");
        assert_eq!(result.entry_uuid, Some("entry-new".to_string()));
    }
}
