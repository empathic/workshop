use anyhow::Result;
use sqlx::Row;

use crate::models::{PaginatedResponse, SearchMatchEntry, SearchResultConversation};

use super::{ConversationRepository, SearchFilters};

impl ConversationRepository {
    // Full-text search across conversation entries with optional faceted filters
    pub async fn search_conversations(
        &self,
        query: &str,
        page: i64,
        per_page: i64,
        max_snippets: i32,
        filters: &SearchFilters,
    ) -> Result<PaginatedResponse<SearchResultConversation>> {
        let escaped = Self::escape_fts_query(query);
        if escaped.is_empty() {
            return Ok(PaginatedResponse {
                items: vec![],
                total: 0,
                page,
                per_page,
                total_pages: 1,
            });
        }

        let offset = (page - 1) * per_page;

        // Build dynamic filter conditions for the entry-level filters
        let mut entry_conditions = Vec::new();
        let mut bind_values: Vec<String> = vec![escaped.clone()];

        if let Some(ref role) = filters.role {
            entry_conditions.push("fts.role = ?".to_string());
            bind_values.push(role.clone());
        }

        if let Some(date_from) = filters.date_from {
            entry_conditions.push("ce.timestamp >= ?".to_string());
            bind_values.push(date_from.to_string());
        }

        if let Some(date_to) = filters.date_to {
            entry_conditions.push("ce.timestamp <= ?".to_string());
            bind_values.push(date_to.to_string());
        }

        // Build the WHERE clause suffix for entry filters
        let entry_filter_clause = if entry_conditions.is_empty() {
            String::new()
        } else {
            format!(" AND {}", entry_conditions.join(" AND "))
        };

        // has_tools filter: require at least one entry with a tool_use mention
        let has_tools_join = if filters.has_tools == Some(true) {
            " AND EXISTS (SELECT 1 FROM conversation_entries te WHERE te.conversation_id = c.id AND (te.raw_json LIKE '%\"ToolUse\"%' OR te.entry_type LIKE '%tool%'))"
        } else {
            ""
        };

        // Step 1: Count matching conversations
        let count_query = format!(
            r#"
            SELECT COUNT(DISTINCT fts.conversation_id) as total
            FROM conversation_entries_fts fts
            JOIN conversations c ON fts.conversation_id = c.id
            JOIN conversation_entries ce ON fts.rowid = ce.id
            WHERE conversation_entries_fts MATCH ?
              AND c.is_deleted = 0{}{}
            "#,
            entry_filter_clause, has_tools_join
        );

        let mut count_builder = sqlx::query(&count_query);
        for val in &bind_values {
            count_builder = count_builder.bind(val);
        }
        let count_row = count_builder.fetch_one(&self.pool).await?;
        let total: i64 = count_row.get("total");

        // Step 2: Find matching conversations with match count, ranked by BM25
        let convo_query = format!(
            r#"
            SELECT fts.conversation_id,
                   c.title,
                   c.instance_id,
                   c.created_at,
                   c.updated_at,
                   COUNT(*) as match_count,
                   MIN(fts.rank) as best_rank,
                   (SELECT COUNT(*) FROM conversation_entries WHERE conversation_id = c.id) as entry_count
            FROM conversation_entries_fts fts
            JOIN conversations c ON fts.conversation_id = c.id
            JOIN conversation_entries ce ON fts.rowid = ce.id
            WHERE conversation_entries_fts MATCH ?
              AND c.is_deleted = 0{}{}
            GROUP BY fts.conversation_id
            ORDER BY best_rank ASC
            LIMIT ? OFFSET ?
            "#,
            entry_filter_clause, has_tools_join
        );

        let mut convo_builder = sqlx::query(&convo_query);
        for val in &bind_values {
            convo_builder = convo_builder.bind(val);
        }
        convo_builder = convo_builder.bind(per_page).bind(offset);
        let convo_rows = convo_builder.fetch_all(&self.pool).await?;

        // Step 3: For each conversation, fetch top N snippets (with same filters)
        let snippet_query = format!(
            r#"
            SELECT snippet(conversation_entries_fts, 0, '[[HL_START]]', '[[HL_END]]', '...', 32) as snippet,
                   fts.entry_uuid,
                   fts.role,
                   ce.timestamp
            FROM conversation_entries_fts fts
            JOIN conversation_entries ce ON fts.rowid = ce.id
            WHERE conversation_entries_fts MATCH ?
              AND fts.conversation_id = ?{}
            ORDER BY fts.rank ASC
            LIMIT ?
            "#,
            entry_filter_clause
        );

        let mut items = Vec::new();
        for row in convo_rows {
            let conversation_id: String = row.get("conversation_id");
            let match_count: i64 = row.get("match_count");

            let mut snippet_builder = sqlx::query(&snippet_query);
            snippet_builder = snippet_builder.bind(&escaped).bind(&conversation_id);

            // Bind the filter values (skip the first which is the FTS query)
            for val in bind_values.iter().skip(1) {
                snippet_builder = snippet_builder.bind(val);
            }
            snippet_builder = snippet_builder.bind(max_snippets);

            let snippet_rows = snippet_builder.fetch_all(&self.pool).await?;

            let matches = snippet_rows
                .into_iter()
                .map(|sr| {
                    let raw_snippet: String = sr.get("snippet");
                    // HTML-escape the snippet, then restore highlight markers
                    let safe = raw_snippet
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                        .replace('"', "&quot;")
                        .replace("[[HL_START]]", "<mark>")
                        .replace("[[HL_END]]", "</mark>");
                    SearchMatchEntry {
                        entry_uuid: sr.get("entry_uuid"),
                        role: sr.get("role"),
                        snippet: safe,
                        timestamp: sr.get("timestamp"),
                    }
                })
                .collect();

            items.push(SearchResultConversation {
                id: conversation_id,
                title: row.get("title"),
                instance_id: row.get("instance_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                entry_count: row.get::<i64, _>("entry_count") as i32,
                match_count: match_count as i32,
                matches,
            });
        }

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

    /// Escape a raw user query for FTS5 by stripping all characters that are not
    /// alphanumeric or basic punctuation, then wrapping each remaining token in
    /// double quotes. This prevents both FTS5 syntax errors and any attempt to
    /// inject FTS5 operators (AND, OR, NOT, NEAR, etc.) or break out of quoting.
    pub(crate) fn escape_fts_query(raw: &str) -> String {
        raw.split_whitespace()
            .map(|word| {
                let cleaned: String = word
                    .chars()
                    .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '\''))
                    .collect();
                if cleaned.is_empty() {
                    String::new()
                } else {
                    format!("\"{}\"", cleaned)
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_fts_query_basic() {
        // Simple words should be quoted
        assert_eq!(
            ConversationRepository::escape_fts_query("hello world"),
            "\"hello\" \"world\""
        );
    }

    #[test]
    fn test_escape_fts_query_special_chars() {
        // Special characters should be stripped except allowed ones
        assert_eq!(
            ConversationRepository::escape_fts_query("hello@world!test"),
            "\"helloworldtest\""
        );
    }

    #[test]
    fn test_escape_fts_query_allowed_punctuation() {
        // Hyphens, underscores, dots, and apostrophes are allowed
        assert_eq!(
            ConversationRepository::escape_fts_query("it's a-test_name.txt"),
            "\"it's\" \"a-test_name.txt\""
        );
    }

    #[test]
    fn test_escape_fts_query_fts_operators_stripped() {
        // FTS5 operators should be treated as regular words (quoted)
        assert_eq!(
            ConversationRepository::escape_fts_query("AND OR NOT NEAR"),
            "\"AND\" \"OR\" \"NOT\" \"NEAR\""
        );
    }

    #[test]
    fn test_escape_fts_query_injection_attempt() {
        // Attempts to inject FTS5 syntax should be neutralized
        assert_eq!(
            ConversationRepository::escape_fts_query("test\" OR \"hack"),
            "\"test\" \"OR\" \"hack\""
        );
    }

    #[test]
    fn test_escape_fts_query_unicode() {
        // Unicode characters should be preserved (alphanumeric includes unicode)
        assert_eq!(
            ConversationRepository::escape_fts_query("café résumé"),
            "\"café\" \"résumé\""
        );
    }

    #[test]
    fn test_escape_fts_query_cjk() {
        // CJK characters should be preserved
        assert_eq!(
            ConversationRepository::escape_fts_query("你好 世界"),
            "\"你好\" \"世界\""
        );
    }

    #[test]
    fn test_escape_fts_query_emoji_stripped() {
        // Emoji are not alphanumeric and should be stripped
        assert_eq!(
            ConversationRepository::escape_fts_query("hello 👋 world"),
            "\"hello\" \"world\""
        );
    }

    #[test]
    fn test_escape_fts_query_empty() {
        // Empty and whitespace-only queries
        assert_eq!(ConversationRepository::escape_fts_query(""), "");
        assert_eq!(ConversationRepository::escape_fts_query("   "), "");
    }

    #[test]
    fn test_escape_fts_query_only_special_chars() {
        // Query with only special characters
        assert_eq!(ConversationRepository::escape_fts_query("!@#$%"), "");
    }

    #[test]
    fn test_escape_fts_query_mixed_whitespace() {
        // Multiple spaces and tabs should be collapsed
        assert_eq!(
            ConversationRepository::escape_fts_query("hello   world\tfoo"),
            "\"hello\" \"world\" \"foo\""
        );
    }

    // --- Integration tests for search_conversations ---

    use crate::models::{Conversation, ConversationEntry};
    use crate::repository::test_helpers;

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
    async fn search_conversations_empty_query() {
        let repo = test_helpers::test_repository().await;
        let filters = super::super::SearchFilters::default();
        let result = repo
            .search_conversations("", 1, 10, 3, &filters)
            .await
            .unwrap();
        assert_eq!(result.total, 0);
        assert!(result.items.is_empty());
    }

    #[tokio::test]
    async fn search_conversations_special_chars_only() {
        let repo = test_helpers::test_repository().await;
        let filters = super::super::SearchFilters::default();
        // Query with only special chars → escapes to empty → returns empty
        let result = repo
            .search_conversations("!@#$%", 1, 10, 3, &filters)
            .await
            .unwrap();
        assert_eq!(result.total, 0);
    }

    #[tokio::test]
    async fn search_conversations_finds_matching_entries() {
        let repo = test_helpers::test_repository().await;

        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![
            make_entry(
                "conv-1",
                "e-1",
                "user",
                "Fix the authentication bug in login",
            ),
            make_entry(
                "conv-1",
                "e-2",
                "assistant",
                "I found the issue in the auth module",
            ),
        ];
        repo.add_entries_batch(&entries).await.unwrap();

        let filters = super::super::SearchFilters::default();
        let result = repo
            .search_conversations("authentication", 1, 10, 3, &filters)
            .await
            .unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].id, "conv-1");
        assert!(result.items[0].match_count > 0);
        assert!(!result.items[0].matches.is_empty());
    }

    #[tokio::test]
    async fn search_conversations_no_match() {
        let repo = test_helpers::test_repository().await;

        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![make_entry("conv-1", "e-1", "user", "Hello world")];
        repo.add_entries_batch(&entries).await.unwrap();

        let filters = super::super::SearchFilters::default();
        let result = repo
            .search_conversations("xyzzyzzy", 1, 10, 3, &filters)
            .await
            .unwrap();
        assert_eq!(result.total, 0);
        assert!(result.items.is_empty());
    }

    #[tokio::test]
    async fn search_conversations_with_role_filter() {
        let repo = test_helpers::test_repository().await;

        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        let entries = vec![
            make_entry("conv-1", "e-1", "user", "Fix the bug please"),
            make_entry("conv-1", "e-2", "assistant", "I will fix the bug"),
        ];
        repo.add_entries_batch(&entries).await.unwrap();

        // Filter to only user messages
        let filters = super::super::SearchFilters {
            role: Some("user".to_string()),
            ..Default::default()
        };
        let result = repo
            .search_conversations("bug", 1, 10, 3, &filters)
            .await
            .unwrap();
        assert_eq!(result.total, 1);
        // Snippet should be from the user entry only
        assert!(
            result.items[0]
                .matches
                .iter()
                .all(|m| m.role == Some("user".to_string()))
        );
    }

    #[tokio::test]
    async fn search_conversations_pagination() {
        let repo = test_helpers::test_repository().await;

        // Create 3 conversations each containing the word "test"
        for i in 0..3 {
            let conv = Conversation::new(format!("conv-{}", i), "inst-1".to_string());
            repo.create_conversation(&conv).await.unwrap();
            let entry = make_entry(
                &format!("conv-{}", i),
                &format!("e-{}", i),
                "user",
                &format!("This is test number {}", i),
            );
            repo.add_entries_batch(&[entry]).await.unwrap();
        }

        let filters = super::super::SearchFilters::default();
        let page1 = repo
            .search_conversations("test", 1, 2, 3, &filters)
            .await
            .unwrap();
        assert_eq!(page1.total, 3);
        assert_eq!(page1.items.len(), 2);
        assert_eq!(page1.total_pages, 2);

        let page2 = repo
            .search_conversations("test", 2, 2, 3, &filters)
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 1);
    }

    #[tokio::test]
    async fn search_conversations_snippet_html_escaping() {
        let repo = test_helpers::test_repository().await;

        let conv = Conversation::new("conv-1".to_string(), "inst-1".to_string());
        repo.create_conversation(&conv).await.unwrap();

        // Content with HTML characters
        let entries = vec![make_entry(
            "conv-1",
            "e-1",
            "user",
            "Use <script>alert('xss')</script> safely",
        )];
        repo.add_entries_batch(&entries).await.unwrap();

        let filters = super::super::SearchFilters::default();
        let result = repo
            .search_conversations("safely", 1, 10, 3, &filters)
            .await
            .unwrap();

        if !result.items.is_empty() && !result.items[0].matches.is_empty() {
            let snippet = &result.items[0].matches[0].snippet;
            // HTML should be escaped
            assert!(!snippet.contains("<script>"));
        }
    }
}
