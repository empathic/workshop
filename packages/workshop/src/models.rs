use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub session_id: Option<String>,
    pub instance_id: String,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_public: bool,
    pub is_deleted: bool,
    pub metadata_json: Option<String>,
    pub file_hash: Option<String>,
    pub file_mtime: Option<i64>,
    /// Import format version - triggers re-import when import logic changes
    pub import_version: Option<i64>,
}

impl Conversation {
    pub fn new(id: String, instance_id: String) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id,
            session_id: None,
            instance_id,
            title: None,
            created_at: now,
            updated_at: now,
            is_public: false,
            is_deleted: false,
            metadata_json: None,
            file_hash: None,
            file_mtime: None,
            import_version: None,
        }
    }

    pub fn with_session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub id: Option<i64>, // None for new entries
    pub conversation_id: String,
    pub entry_uuid: String,
    pub parent_uuid: Option<String>,
    pub entry_type: String,
    pub role: Option<String>,
    pub content: Option<String>,
    pub timestamp: String,
    pub raw_json: String,
    pub token_count: Option<i32>,
    pub model: Option<String>,
}

impl ConversationEntry {
    /// Construct from a provider-agnostic Turn + supplemental data.
    pub fn from_turn(
        conversation_id: String,
        turn: &toolpath_convo::Turn,
        entry_type: String,
        raw_json: String,
    ) -> Self {
        let role = match &turn.role {
            toolpath_convo::Role::User => Some("user".to_string()),
            toolpath_convo::Role::Assistant => Some("assistant".to_string()),
            toolpath_convo::Role::System => Some("system".to_string()),
            toolpath_convo::Role::Other(s) => Some(s.clone()),
        };
        let content = if turn.text.is_empty() {
            None
        } else {
            Some(turn.text.clone())
        };
        Self {
            id: None,
            conversation_id,
            entry_uuid: turn.id.clone(),
            parent_uuid: turn.parent_id.clone(),
            entry_type,
            role,
            content,
            timestamp: turn.timestamp.clone(),
            raw_json,
            token_count: None,
            model: turn.model.clone(),
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Comment {
    pub id: Option<i64>,
    pub conversation_id: String,
    pub entry_uuid: Option<String>,
    pub author: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: Option<i64>,
}

impl Comment {
    pub fn new(
        conversation_id: String,
        content: String,
        author: Option<String>,
        entry_uuid: Option<String>,
    ) -> Self {
        Self {
            id: None,
            conversation_id,
            entry_uuid,
            author: author.unwrap_or_else(|| "anonymous".to_string()),
            content,
            created_at: Utc::now().timestamp(),
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ConversationShare {
    pub id: Option<i64>,
    pub conversation_id: String,
    pub share_token: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub access_count: i32,
    pub max_access_count: Option<i32>,
    pub password_hash: Option<String>,
}

impl ConversationShare {
    pub fn new(conversation_id: String, expires_in_days: Option<i32>) -> Self {
        let share_token = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now().timestamp();
        let expires_at = expires_in_days.map(|days| created_at + (days as i64 * 24 * 60 * 60));

        Self {
            id: None,
            conversation_id,
            share_token,
            title: None,
            description: None,
            created_at,
            expires_at,
            access_count: 0,
            max_access_count: None,
            password_hash: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires) = self.expires_at {
            Utc::now().timestamp() > expires
        } else {
            false
        }
    }

    pub fn is_access_limit_reached(&self) -> bool {
        if let Some(max) = self.max_access_count {
            self.access_count >= max
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
}

/// Lightweight attribution info for a conversation entry (used in REST API responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryAttribution {
    pub entry_uuid: String,
    pub user_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationWithEntries {
    pub conversation: Conversation,
    pub entries: Vec<ConversationEntry>,
    pub comments: Vec<Comment>,
    pub tags: Vec<Tag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributions: Vec<EntryAttribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: Option<String>,
    pub instance_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub entry_count: i32,
    pub is_public: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatchEntry {
    pub entry_uuid: String,
    pub role: Option<String>,
    pub snippet: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultConversation {
    pub id: String,
    pub title: Option<String>,
    pub instance_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub entry_count: i32,
    pub match_count: i32,
    pub matches: Vec<SearchMatchEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub total_pages: i64,
}

// === Auth models ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub display_name: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub is_admin: bool,
    pub is_disabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Public user info (no password hash)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
}

impl From<User> for UserInfo {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            is_admin: u.is_admin,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user_id: String,
    pub csrf_token: String,
    pub expires_at: i64,
    pub last_active_at: i64,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstancePermission {
    pub instance_id: String,
    pub user_id: String,
    pub role: String, // "owner" or "collaborator"
    pub granted_at: i64,
    pub granted_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInvitation {
    pub invite_token: String,
    pub instance_id: String,
    pub created_by: String,
    pub role: String,
    pub max_uses: Option<i32>,
    pub use_count: i32,
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

impl InstanceInvitation {
    pub fn is_expired(&self) -> bool {
        if let Some(expires) = self.expires_at {
            Utc::now().timestamp() > expires
        } else {
            false
        }
    }

    pub fn is_used_up(&self) -> bool {
        if let Some(max) = self.max_uses {
            self.use_count >= max
        } else {
            false
        }
    }
}

// === Server Invite models ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInvite {
    pub token: String,
    pub created_by: String,
    pub label: Option<String>,
    pub max_uses: Option<i32>,
    pub use_count: i32,
    pub expires_at: Option<i64>,
    pub revoked: bool,
    pub created_at: i64,
}

impl ServerInvite {
    pub fn is_expired(&self) -> bool {
        if let Some(expires) = self.expires_at {
            Utc::now().timestamp() > expires
        } else {
            false
        }
    }

    pub fn is_used_up(&self) -> bool {
        if let Some(max) = self.max_uses {
            self.use_count >= max
        } else {
            false
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.revoked && !self.is_expired() && !self.is_used_up()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInviteWithAcceptors {
    pub invite: ServerInvite,
    pub acceptors: Vec<InviteAcceptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteAcceptor {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputAttribution {
    pub id: Option<i64>,
    pub instance_id: String,
    pub user_id: String,
    pub display_name: String,
    pub timestamp: i64,
    pub entry_uuid: Option<String>,
    pub content_preview: Option<String>,
    pub task_id: Option<i64>,
}

/// Max characters to compare when content-matching attributions.
/// Both sides may be truncated independently, so we use prefix matching.
pub const ATTRIBUTION_CONTENT_PREFIX_LEN: usize = 100;

/// Normalize content for attribution matching: strip \r (terminal line-endings),
/// trim whitespace, take first N chars.
///
/// Terminal input arrives with \r\n line endings but Claude Code writes \n to JSONL.
/// Stripping \r before comparison ensures multi-line messages match correctly.
pub fn normalize_attribution_content(content: &str) -> String {
    content
        .replace('\r', "")
        .trim()
        .chars()
        .take(ATTRIBUTION_CONTENT_PREFIX_LEN)
        .collect::<String>()
}

/// Check if two content strings match for attribution purposes.
/// Uses prefix matching because either side may be truncated at different lengths.
///
/// This is the single source of truth for "does this input match this conversation entry?"
/// Both the in-process queue and the DB correlation path use this.
// === Chat models ===

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Option<i64>,
    pub uuid: String,
    pub scope: String, // "global" or an instance_id
    pub user_id: String,
    pub display_name: String,
    pub content: String,
    pub created_at: i64,
    pub forwarded_from: Option<String>, // original scope if forwarded
    pub topic: Option<String>,          // Zulip-style topic label
}

/// Summary of a chat topic for topic listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTopicSummary {
    pub topic: String,
    pub message_count: i64,
    pub latest_at: i64,
}

// === Task models ===

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Task {
    pub id: Option<i64>,
    pub uuid: String,
    pub title: String,
    pub body: Option<String>,
    pub status: String,
    pub priority: i32,
    pub instance_id: Option<String>,
    pub creator_id: Option<String>,
    pub creator_name: String,
    pub sort_order: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub is_deleted: bool,
    pub sent_text: Option<String>,
    pub conversation_id: Option<String>,
}

/// An inbox item for the fleet attention model.
/// One per instance — represents the current actionable event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxItem {
    pub instance_id: String,
    /// Event type: "completed_turn", "needs_input", or "error"
    pub event_type: String,
    /// Number of accumulated turns (incremented for repeated completed_turn events)
    pub turn_count: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDispatch {
    pub id: Option<i64>,
    pub task_id: i64,
    pub instance_id: String,
    pub sent_text: String,
    pub conversation_id: Option<String>,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskWithTags {
    #[serde(flatten)]
    pub task: Task,
    pub tags: Vec<Tag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dispatches: Vec<TaskDispatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    pub body: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i32>,
    pub instance_id: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub body: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i32>,
    pub instance_id: Option<String>,
    pub sort_order: Option<f64>,
    pub sent_text: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskListFilters {
    pub status: Option<String>,
    pub instance_id: Option<String>,
    pub tag: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateTaskItem {
    pub title: String,
    pub instance_id: Option<String>,
    pub created_at: Option<i64>,
}

pub fn attribution_content_matches(stored_content: &str, entry_content: &str) -> bool {
    let stored = normalize_attribution_content(stored_content);
    let entry = normalize_attribution_content(entry_content);
    if stored.is_empty() || entry.is_empty() {
        return false;
    }
    stored.starts_with(&entry) || entry.starts_with(&stored)
}

#[cfg(test)]
mod model_tests {
    use super::*;

    // ── Conversation ────────────────────────────────────────────────────

    #[test]
    fn test_conversation_new() {
        let c = Conversation::new("conv-1".into(), "inst-1".into());
        assert_eq!(c.id, "conv-1");
        assert_eq!(c.instance_id, "inst-1");
        assert!(c.session_id.is_none());
        assert!(c.title.is_none());
        assert!(!c.is_public);
        assert!(!c.is_deleted);
        assert!(c.created_at > 0);
        assert_eq!(c.created_at, c.updated_at);
    }

    #[test]
    fn test_conversation_with_session_id() {
        let c = Conversation::new("c".into(), "i".into()).with_session_id("sess-123".into());
        assert_eq!(c.session_id.as_deref(), Some("sess-123"));
    }

    // ── Comment ─────────────────────────────────────────────────────────

    #[test]
    fn test_comment_new_with_author() {
        let c = Comment::new(
            "conv-1".into(),
            "Great work!".into(),
            Some("alice".into()),
            None,
        );
        assert_eq!(c.conversation_id, "conv-1");
        assert_eq!(c.content, "Great work!");
        assert_eq!(c.author, "alice");
        assert!(c.entry_uuid.is_none());
        assert!(c.id.is_none());
        assert!(c.updated_at.is_none());
        assert!(c.created_at > 0);
    }

    #[test]
    fn test_comment_new_anonymous() {
        let c = Comment::new(
            "conv-1".into(),
            "Note".into(),
            None,
            Some("entry-uuid".into()),
        );
        assert_eq!(c.author, "anonymous");
        assert_eq!(c.entry_uuid.as_deref(), Some("entry-uuid"));
    }

    // ── ConversationShare ───────────────────────────────────────────────

    #[test]
    fn test_share_new() {
        let s = ConversationShare::new("conv-1".into(), None);
        assert_eq!(s.conversation_id, "conv-1");
        assert!(!s.share_token.is_empty());
        assert!(s.expires_at.is_none());
        assert_eq!(s.access_count, 0);
        assert!(s.max_access_count.is_none());
    }

    #[test]
    fn test_share_new_with_expiry() {
        let s = ConversationShare::new("conv-1".into(), Some(7));
        assert!(s.expires_at.is_some());
        let expected_delta = 7 * 24 * 60 * 60;
        let actual_delta = s.expires_at.unwrap() - s.created_at;
        assert_eq!(actual_delta, expected_delta);
    }

    #[test]
    fn test_share_is_expired_no_expiry() {
        let s = ConversationShare::new("conv-1".into(), None);
        assert!(!s.is_expired());
    }

    #[test]
    fn test_share_is_expired_future() {
        let s = ConversationShare::new("conv-1".into(), Some(30));
        assert!(!s.is_expired());
    }

    #[test]
    fn test_share_is_expired_past() {
        let mut s = ConversationShare::new("conv-1".into(), None);
        s.expires_at = Some(0); // epoch = definitely past
        assert!(s.is_expired());
    }

    #[test]
    fn test_share_access_limit_not_reached() {
        let mut s = ConversationShare::new("conv-1".into(), None);
        s.max_access_count = Some(10);
        s.access_count = 5;
        assert!(!s.is_access_limit_reached());
    }

    #[test]
    fn test_share_access_limit_reached() {
        let mut s = ConversationShare::new("conv-1".into(), None);
        s.max_access_count = Some(10);
        s.access_count = 10;
        assert!(s.is_access_limit_reached());
    }

    #[test]
    fn test_share_access_limit_none() {
        let s = ConversationShare::new("conv-1".into(), None);
        assert!(!s.is_access_limit_reached());
    }

    // ── InstanceInvitation ──────────────────────────────────────────────

    #[test]
    fn test_invitation_is_expired_no_expiry() {
        let inv = InstanceInvitation {
            invite_token: "tok".into(),
            instance_id: "inst".into(),
            created_by: "alice".into(),
            role: "collaborator".into(),
            max_uses: None,
            use_count: 0,
            expires_at: None,
            created_at: 0,
        };
        assert!(!inv.is_expired());
    }

    #[test]
    fn test_invitation_is_expired_past() {
        let inv = InstanceInvitation {
            invite_token: "tok".into(),
            instance_id: "inst".into(),
            created_by: "alice".into(),
            role: "collaborator".into(),
            max_uses: None,
            use_count: 0,
            expires_at: Some(0),
            created_at: 0,
        };
        assert!(inv.is_expired());
    }

    #[test]
    fn test_invitation_is_used_up() {
        let inv = InstanceInvitation {
            invite_token: "tok".into(),
            instance_id: "inst".into(),
            created_by: "alice".into(),
            role: "collaborator".into(),
            max_uses: Some(3),
            use_count: 3,
            expires_at: None,
            created_at: 0,
        };
        assert!(inv.is_used_up());
    }

    #[test]
    fn test_invitation_not_used_up() {
        let inv = InstanceInvitation {
            invite_token: "tok".into(),
            instance_id: "inst".into(),
            created_by: "alice".into(),
            role: "collaborator".into(),
            max_uses: Some(3),
            use_count: 1,
            expires_at: None,
            created_at: 0,
        };
        assert!(!inv.is_used_up());
    }

    #[test]
    fn test_invitation_unlimited_uses() {
        let inv = InstanceInvitation {
            invite_token: "tok".into(),
            instance_id: "inst".into(),
            created_by: "alice".into(),
            role: "collaborator".into(),
            max_uses: None,
            use_count: 1000,
            expires_at: None,
            created_at: 0,
        };
        assert!(!inv.is_used_up());
    }

    // ── ServerInvite ────────────────────────────────────────────────────

    fn make_server_invite() -> ServerInvite {
        ServerInvite {
            token: "tok".into(),
            created_by: "admin".into(),
            label: None,
            max_uses: None,
            use_count: 0,
            expires_at: None,
            revoked: false,
            created_at: Utc::now().timestamp(),
        }
    }

    #[test]
    fn test_server_invite_is_valid() {
        let inv = make_server_invite();
        assert!(inv.is_valid());
    }

    #[test]
    fn test_server_invite_revoked_not_valid() {
        let mut inv = make_server_invite();
        inv.revoked = true;
        assert!(!inv.is_valid());
    }

    #[test]
    fn test_server_invite_expired_not_valid() {
        let mut inv = make_server_invite();
        inv.expires_at = Some(0);
        assert!(!inv.is_valid());
    }

    #[test]
    fn test_server_invite_used_up_not_valid() {
        let mut inv = make_server_invite();
        inv.max_uses = Some(1);
        inv.use_count = 1;
        assert!(!inv.is_valid());
    }

    // ── UserInfo ────────────────────────────────────────────────────────

    #[test]
    fn test_user_to_user_info() {
        let user = User {
            id: "u1".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            password_hash: "secret_hash".into(),
            is_admin: true,
            is_disabled: false,
            created_at: 100,
            updated_at: 200,
        };
        let info: UserInfo = user.into();
        assert_eq!(info.id, "u1");
        assert_eq!(info.username, "alice");
        assert_eq!(info.display_name, "Alice");
        assert!(info.is_admin);
    }

    // ── normalize_attribution_content ───────────────────────────────────

    #[test]
    fn test_normalize_strips_cr() {
        assert_eq!(
            normalize_attribution_content("line1\r\nline2\r\n"),
            "line1\nline2"
        );
    }

    #[test]
    fn test_normalize_truncates() {
        let long = "a".repeat(200);
        let normalized = normalize_attribution_content(&long);
        assert_eq!(normalized.len(), ATTRIBUTION_CONTENT_PREFIX_LEN);
    }
}

#[cfg(test)]
mod attribution_tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(attribution_content_matches(
            "fix the auth bug",
            "fix the auth bug"
        ));
    }

    #[test]
    fn test_prefix_match_stored_longer() {
        // Stored has full content, entry was truncated
        assert!(attribution_content_matches(
            "fix the auth bug in login.rs and also update the tests",
            "fix the auth bug in login.rs"
        ));
    }

    #[test]
    fn test_prefix_match_entry_longer() {
        // Stored was truncated (content_preview is 100 chars), entry has full content
        assert!(attribution_content_matches(
            "fix the auth bug",
            "fix the auth bug in login.rs and also update the tests"
        ));
    }

    #[test]
    fn test_no_match_different_content() {
        assert!(!attribution_content_matches(
            "fix the auth bug",
            "add unit tests"
        ));
    }

    #[test]
    fn test_no_match_empty() {
        assert!(!attribution_content_matches("", "fix the auth bug"));
        assert!(!attribution_content_matches("fix the auth bug", ""));
        assert!(!attribution_content_matches("", ""));
    }

    #[test]
    fn test_whitespace_trimming() {
        assert!(attribution_content_matches(
            "  fix the auth bug  ",
            "fix the auth bug"
        ));
        assert!(attribution_content_matches(
            "fix the auth bug",
            "  fix the auth bug  "
        ));
    }

    #[test]
    fn test_no_false_swap_different_messages() {
        // The critical scenario: two users type different things close together.
        // "fix auth bug" should NOT match "add unit tests".
        let user_a_input = "fix the authentication bug in the login flow";
        let user_b_input = "add unit tests for the payment module";

        // Entry content from Claude's conversation
        let entry_a = "fix the authentication bug in the login flow";
        let entry_b = "add unit tests for the payment module";

        // A matches A, not B
        assert!(attribution_content_matches(user_a_input, entry_a));
        assert!(!attribution_content_matches(user_a_input, entry_b));

        // B matches B, not A
        assert!(attribution_content_matches(user_b_input, entry_b));
        assert!(!attribution_content_matches(user_b_input, entry_a));
    }

    #[test]
    fn test_long_content_truncation() {
        // Content longer than ATTRIBUTION_CONTENT_PREFIX_LEN
        let long_a = "a".repeat(200);
        let long_b = format!("{}bbb", "a".repeat(200));
        // Both truncate to the same first 100 chars
        assert!(attribution_content_matches(&long_a, &long_b));
    }

    #[test]
    fn test_normalize_preserves_meaningful_content() {
        let normalized = normalize_attribution_content("  hello world  ");
        assert_eq!(normalized, "hello world");
    }
}

#[cfg(test)]
mod serde_tests {
    use super::*;

    #[test]
    fn paginated_response_serde() {
        let resp = PaginatedResponse {
            items: vec!["a".to_string(), "b".to_string()],
            total: 10,
            page: 1,
            per_page: 2,
            total_pages: 5,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 10);
        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 2);
        assert_eq!(json["total_pages"], 5);
        assert_eq!(json["items"].as_array().unwrap().len(), 2);
        let rt: PaginatedResponse<String> = serde_json::from_value(json).unwrap();
        assert_eq!(rt.items, vec!["a", "b"]);
    }

    #[test]
    fn chat_message_serde() {
        let msg = ChatMessage {
            id: Some(42),
            uuid: "msg-uuid".into(),
            scope: "global".into(),
            user_id: "u-1".into(),
            display_name: "Alice".into(),
            content: "Hello!".into(),
            created_at: 1000,
            forwarded_from: Some("inst-1".into()),
            topic: Some("general".into()),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["uuid"], "msg-uuid");
        assert_eq!(json["scope"], "global");
        assert_eq!(json["forwarded_from"], "inst-1");
        assert_eq!(json["topic"], "general");
        let rt: ChatMessage = serde_json::from_value(json).unwrap();
        assert_eq!(rt.id, Some(42));
        assert_eq!(rt.content, "Hello!");
    }

    #[test]
    fn chat_message_none_fields() {
        let msg = ChatMessage {
            id: None,
            uuid: "u".into(),
            scope: "global".into(),
            user_id: "u".into(),
            display_name: "A".into(),
            content: "x".into(),
            created_at: 0,
            forwarded_from: None,
            topic: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json["forwarded_from"].is_null());
        assert!(json["topic"].is_null());
    }

    #[test]
    fn chat_topic_summary_serde() {
        let s = ChatTopicSummary {
            topic: "general".into(),
            message_count: 42,
            latest_at: 9999,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["topic"], "general");
        assert_eq!(json["message_count"], 42);
        let rt: ChatTopicSummary = serde_json::from_value(json).unwrap();
        assert_eq!(rt.latest_at, 9999);
    }

    #[test]
    fn task_serde() {
        let t = Task {
            id: Some(1),
            uuid: "task-uuid".into(),
            title: "Fix bug".into(),
            body: Some("Details here".into()),
            status: "open".into(),
            priority: 2,
            instance_id: Some("inst-1".into()),
            creator_id: Some("u-1".into()),
            creator_name: "Alice".into(),
            sort_order: 1.5,
            created_at: 100,
            updated_at: 200,
            completed_at: None,
            is_deleted: false,
            sent_text: None,
            conversation_id: None,
        };
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["title"], "Fix bug");
        assert_eq!(json["priority"], 2);
        assert_eq!(json["sort_order"], 1.5);
        let rt: Task = serde_json::from_value(json).unwrap();
        assert_eq!(rt.uuid, "task-uuid");
        assert!(rt.completed_at.is_none());
    }

    #[test]
    fn task_dispatch_serde() {
        let d = TaskDispatch {
            id: Some(10),
            task_id: 1,
            instance_id: "inst-1".into(),
            sent_text: "Do this".into(),
            conversation_id: Some("conv-1".into()),
            sent_at: 500,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["task_id"], 1);
        assert_eq!(json["sent_text"], "Do this");
        let rt: TaskDispatch = serde_json::from_value(json).unwrap();
        assert_eq!(rt.conversation_id, Some("conv-1".into()));
    }

    #[test]
    fn task_with_tags_serde() {
        let tw = TaskWithTags {
            task: Task {
                id: Some(1),
                uuid: "t".into(),
                title: "T".into(),
                body: None,
                status: "open".into(),
                priority: 0,
                instance_id: None,
                creator_id: None,
                creator_name: "A".into(),
                sort_order: 0.0,
                created_at: 0,
                updated_at: 0,
                completed_at: None,
                is_deleted: false,
                sent_text: None,
                conversation_id: None,
            },
            tags: vec![Tag {
                id: 1,
                name: "bug".into(),
                color: Some("#ff0000".into()),
            }],
            dispatches: vec![],
        };
        let json = serde_json::to_value(&tw).unwrap();
        // #[serde(flatten)] means task fields appear at top level
        assert_eq!(json["title"], "T");
        assert_eq!(json["tags"][0]["name"], "bug");
        // dispatches should be omitted when empty (skip_serializing_if)
        assert!(json.get("dispatches").is_none());
    }

    #[test]
    fn task_with_tags_with_dispatches() {
        let tw = TaskWithTags {
            task: Task {
                id: Some(1),
                uuid: "t".into(),
                title: "T".into(),
                body: None,
                status: "open".into(),
                priority: 0,
                instance_id: None,
                creator_id: None,
                creator_name: "A".into(),
                sort_order: 0.0,
                created_at: 0,
                updated_at: 0,
                completed_at: None,
                is_deleted: false,
                sent_text: None,
                conversation_id: None,
            },
            tags: vec![],
            dispatches: vec![TaskDispatch {
                id: None,
                task_id: 1,
                instance_id: "i".into(),
                sent_text: "go".into(),
                conversation_id: None,
                sent_at: 0,
            }],
        };
        let json = serde_json::to_value(&tw).unwrap();
        assert_eq!(json["dispatches"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn create_task_request_serde() {
        let req = CreateTaskRequest {
            title: "New task".into(),
            body: None,
            status: Some("open".into()),
            priority: Some(1),
            instance_id: None,
            tags: Some(vec!["bug".into()]),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["title"], "New task");
        assert_eq!(json["tags"][0], "bug");
        let rt: CreateTaskRequest = serde_json::from_value(json).unwrap();
        assert_eq!(rt.priority, Some(1));
    }

    #[test]
    fn update_task_request_default() {
        let req = UpdateTaskRequest::default();
        let json = serde_json::to_value(&req).unwrap();
        assert!(json["title"].is_null());
        assert!(json["status"].is_null());
    }

    #[test]
    fn task_list_filters_deserialize() {
        let json = serde_json::json!({
            "status": "open",
            "tag": "bug",
            "limit": 10
        });
        let f: TaskListFilters = serde_json::from_value(json).unwrap();
        assert_eq!(f.status, Some("open".into()));
        assert_eq!(f.tag, Some("bug".into()));
        assert_eq!(f.limit, Some(10));
        assert!(f.instance_id.is_none());
        assert!(f.search.is_none());
        assert!(f.offset.is_none());
    }

    #[test]
    fn migrate_task_item_serde() {
        let m = MigrateTaskItem {
            title: "Migrate me".into(),
            instance_id: Some("inst-1".into()),
            created_at: Some(12345),
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["title"], "Migrate me");
        let rt: MigrateTaskItem = serde_json::from_value(json).unwrap();
        assert_eq!(rt.created_at, Some(12345));
    }

    #[test]
    fn input_attribution_serde() {
        let attr = InputAttribution {
            id: Some(1),
            instance_id: "inst-1".into(),
            user_id: "u-1".into(),
            display_name: "Alice".into(),
            timestamp: 999,
            entry_uuid: Some("entry-1".into()),
            content_preview: Some("hello".into()),
            task_id: None,
        };
        let json = serde_json::to_value(&attr).unwrap();
        assert_eq!(json["instance_id"], "inst-1");
        assert_eq!(json["entry_uuid"], "entry-1");
        let rt: InputAttribution = serde_json::from_value(json).unwrap();
        assert_eq!(rt.timestamp, 999);
        assert!(rt.task_id.is_none());
    }

    #[test]
    fn conversation_summary_serde() {
        let cs = ConversationSummary {
            id: "conv-1".into(),
            title: Some("My convo".into()),
            instance_id: "inst-1".into(),
            created_at: 100,
            updated_at: 200,
            entry_count: 5,
            is_public: true,
        };
        let json = serde_json::to_value(&cs).unwrap();
        assert_eq!(json["id"], "conv-1");
        assert_eq!(json["entry_count"], 5);
        assert_eq!(json["is_public"], true);
        let rt: ConversationSummary = serde_json::from_value(json).unwrap();
        assert_eq!(rt.title, Some("My convo".into()));
    }

    #[test]
    fn search_result_conversation_serde() {
        let sr = SearchResultConversation {
            id: "conv-1".into(),
            title: None,
            instance_id: "inst-1".into(),
            created_at: 100,
            updated_at: 200,
            entry_count: 10,
            match_count: 3,
            matches: vec![SearchMatchEntry {
                entry_uuid: "e-1".into(),
                role: Some("assistant".into()),
                snippet: "found it".into(),
                timestamp: "2025-01-01T00:00:00Z".into(),
            }],
        };
        let json = serde_json::to_value(&sr).unwrap();
        assert_eq!(json["match_count"], 3);
        assert_eq!(json["matches"][0]["snippet"], "found it");
        let rt: SearchResultConversation = serde_json::from_value(json).unwrap();
        assert_eq!(rt.matches.len(), 1);
        assert_eq!(rt.matches[0].role, Some("assistant".into()));
    }

    #[test]
    fn entry_attribution_serde() {
        let ea = EntryAttribution {
            entry_uuid: "e-1".into(),
            user_id: "u-1".into(),
            display_name: "Alice".into(),
            task_id: Some(42),
        };
        let json = serde_json::to_value(&ea).unwrap();
        assert_eq!(json["task_id"], 42);
        let rt: EntryAttribution = serde_json::from_value(json).unwrap();
        assert_eq!(rt.entry_uuid, "e-1");
    }

    #[test]
    fn entry_attribution_skip_none_task_id() {
        let ea = EntryAttribution {
            entry_uuid: "e-1".into(),
            user_id: "u-1".into(),
            display_name: "Alice".into(),
            task_id: None,
        };
        let json = serde_json::to_value(&ea).unwrap();
        assert!(json.get("task_id").is_none());
    }

    #[test]
    fn conversation_with_entries_serde() {
        let cwe = ConversationWithEntries {
            conversation: Conversation::new("conv-1".into(), "inst-1".into()),
            entries: vec![],
            comments: vec![],
            tags: vec![Tag {
                id: 1,
                name: "important".into(),
                color: None,
            }],
            attributions: vec![],
        };
        let json = serde_json::to_value(&cwe).unwrap();
        assert_eq!(json["conversation"]["id"], "conv-1");
        assert_eq!(json["tags"][0]["name"], "important");
        // attributions empty → should be skipped
        assert!(json.get("attributions").is_none());
    }

    #[test]
    fn conversation_with_entries_with_attributions() {
        let cwe = ConversationWithEntries {
            conversation: Conversation::new("conv-1".into(), "inst-1".into()),
            entries: vec![],
            comments: vec![],
            tags: vec![],
            attributions: vec![EntryAttribution {
                entry_uuid: "e-1".into(),
                user_id: "u-1".into(),
                display_name: "Alice".into(),
                task_id: None,
            }],
        };
        let json = serde_json::to_value(&cwe).unwrap();
        assert_eq!(json["attributions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn user_password_hash_not_serialized() {
        let user = User {
            id: "u-1".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            password_hash: "super_secret".into(),
            is_admin: false,
            is_disabled: false,
            created_at: 0,
            updated_at: 0,
        };
        let json = serde_json::to_value(&user).unwrap();
        // password_hash has #[serde(skip_serializing)]
        assert!(json.get("password_hash").is_none());
    }

    #[test]
    fn session_serde() {
        let s = Session {
            token: "tok".into(),
            user_id: "u-1".into(),
            csrf_token: "csrf".into(),
            expires_at: 9999,
            last_active_at: 1000,
            user_agent: Some("test-agent".into()),
            ip_address: None,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["csrf_token"], "csrf");
        let rt: Session = serde_json::from_value(json).unwrap();
        assert_eq!(rt.expires_at, 9999);
        assert!(rt.ip_address.is_none());
    }

    #[test]
    fn instance_permission_serde() {
        let p = InstancePermission {
            instance_id: "inst-1".into(),
            user_id: "u-1".into(),
            role: "owner".into(),
            granted_at: 500,
            granted_by: Some("admin".into()),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["role"], "owner");
        let rt: InstancePermission = serde_json::from_value(json).unwrap();
        assert_eq!(rt.granted_by, Some("admin".into()));
    }

    #[test]
    fn server_invite_with_acceptors_serde() {
        let siwa = ServerInviteWithAcceptors {
            invite: ServerInvite {
                token: "tok".into(),
                created_by: "admin".into(),
                label: Some("Team invite".into()),
                max_uses: Some(5),
                use_count: 2,
                expires_at: None,
                revoked: false,
                created_at: 100,
            },
            acceptors: vec![InviteAcceptor {
                user_id: "u-1".into(),
                username: "alice".into(),
                display_name: "Alice".into(),
                created_at: 200,
            }],
        };
        let json = serde_json::to_value(&siwa).unwrap();
        assert_eq!(json["invite"]["label"], "Team invite");
        assert_eq!(json["acceptors"][0]["username"], "alice");
        let rt: ServerInviteWithAcceptors = serde_json::from_value(json).unwrap();
        assert_eq!(rt.acceptors.len(), 1);
    }

    #[test]
    fn conversation_share_serde() {
        let cs = ConversationShare::new("conv-1".into(), Some(7));
        let json = serde_json::to_value(&cs).unwrap();
        assert_eq!(json["conversation_id"], "conv-1");
        assert!(!json["share_token"].as_str().unwrap().is_empty());
        let rt: ConversationShare = serde_json::from_value(json).unwrap();
        assert_eq!(rt.conversation_id, "conv-1");
    }

    #[test]
    fn conversation_serde() {
        let c = Conversation::new("conv-1".into(), "inst-1".into());
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["id"], "conv-1");
        assert_eq!(json["is_public"], false);
        assert_eq!(json["is_deleted"], false);
        let rt: Conversation = serde_json::from_value(json).unwrap();
        assert_eq!(rt.instance_id, "inst-1");
    }

    #[test]
    fn conversation_entry_serde() {
        let ce = ConversationEntry {
            id: Some(1),
            conversation_id: "conv-1".into(),
            entry_uuid: "e-uuid".into(),
            parent_uuid: Some("p-uuid".into()),
            entry_type: "human".into(),
            role: Some("user".into()),
            content: Some("Hello".into()),
            timestamp: "2025-01-01T00:00:00Z".into(),
            raw_json: "{}".into(),
            token_count: Some(5),
            model: None,
        };
        let json = serde_json::to_value(&ce).unwrap();
        assert_eq!(json["entry_type"], "human");
        assert_eq!(json["token_count"], 5);
        let rt: ConversationEntry = serde_json::from_value(json).unwrap();
        assert_eq!(rt.entry_uuid, "e-uuid");
    }

    #[test]
    fn tag_serde() {
        let t = Tag {
            id: 1,
            name: "bug".into(),
            color: Some("#ff0000".into()),
        };
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["name"], "bug");
        let rt: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(rt.color, Some("#ff0000".into()));
    }

    #[test]
    fn instance_invitation_serde() {
        let inv = InstanceInvitation {
            invite_token: "tok".into(),
            instance_id: "inst-1".into(),
            created_by: "alice".into(),
            role: "collaborator".into(),
            max_uses: Some(3),
            use_count: 1,
            expires_at: Some(99999),
            created_at: 100,
        };
        let json = serde_json::to_value(&inv).unwrap();
        assert_eq!(json["role"], "collaborator");
        assert_eq!(json["max_uses"], 3);
        let rt: InstanceInvitation = serde_json::from_value(json).unwrap();
        assert_eq!(rt.use_count, 1);
    }

    #[test]
    fn comment_serde() {
        let c = Comment::new("conv-1".into(), "Nice".into(), Some("bob".into()), None);
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["author"], "bob");
        assert_eq!(json["content"], "Nice");
        let rt: Comment = serde_json::from_value(json).unwrap();
        assert_eq!(rt.conversation_id, "conv-1");
    }
}
