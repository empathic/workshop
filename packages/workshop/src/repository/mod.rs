// Repository layer — each domain lives in its own file with `impl ConversationRepository`.
//
// All callers still import `crate::repository::ConversationRepository` or
// `crate::repository::SearchFilters` — no callsite changes required.

use sqlx::sqlite::SqlitePool;

mod attributions;
mod auth;
mod chat;
mod conversations;
mod entries;
mod inbox;
mod search;
mod settings;
mod tasks;

#[cfg(test)]
pub(crate) mod test_helpers;

/// Filters for faceted search
#[derive(Debug, Default)]
pub struct SearchFilters {
    /// Filter by message role (e.g., "user", "assistant")
    pub role: Option<String>,
    /// Filter entries after this Unix timestamp
    pub date_from: Option<i64>,
    /// Filter entries before this Unix timestamp
    pub date_to: Option<i64>,
    /// Only return conversations containing tool use
    pub has_tools: Option<bool>,
}

#[derive(Clone)]
pub struct ConversationRepository {
    pub(crate) pool: SqlitePool,
}

impl ConversationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}
