use anyhow::{Context, Result};
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::info;

use crate::config::WorkshopConfig;

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(config: &WorkshopConfig) -> Result<Self> {
        info!("🗄️  Connecting to database: {}", config.db_path.display());

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .connect(&config.db_url())
            .await
            .with_context(|| format!("Failed to connect to database: {}", config.db_url()))?;

        // Run migrations manually (Bazel doesn't package the migrations directory)
        info!("Running database migrations...");
        self::run_migrations(&pool).await?;

        // Set pragmas for performance
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA cache_size = -64000") // 64MB cache
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA temp_store = MEMORY")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;

        info!("✅ Database initialized successfully");

        Ok(Self { pool })
    }

    pub async fn get_stats(&self) -> Result<DbStats> {
        let row = sqlx::query(
            r#"
            SELECT
                (SELECT COUNT(*) FROM conversations WHERE is_deleted = 0) as conversation_count,
                (SELECT COUNT(DISTINCT session_id) FROM conversations WHERE is_deleted = 0) as session_count,
                (SELECT COUNT(*) FROM conversation_entries) as entry_count,
                (SELECT COUNT(*) FROM comments) as comment_count,
                (SELECT COUNT(*) FROM conversation_shares) as share_count,
                (SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()) as db_size
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(DbStats {
            conversations: row.try_get::<i64, _>("conversation_count").unwrap_or(0) as u64,
            sessions: row.try_get::<i64, _>("session_count").unwrap_or(0) as u64,
            entries: row.try_get::<i64, _>("entry_count").unwrap_or(0) as u64,
            comments: row.try_get::<i64, _>("comment_count").unwrap_or(0) as u64,
            shares: row.try_get::<i64, _>("share_count").unwrap_or(0) as u64,
            database_size_bytes: row.try_get::<i64, _>("db_size").unwrap_or(0) as u64,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DbStats {
    pub conversations: u64,
    pub sessions: u64,
    pub entries: u64,
    pub comments: u64,
    pub shares: u64,
    pub database_size_bytes: u64,
}

/// Current schema version - increment when adding migrations
const SCHEMA_VERSION: i64 = 11;

// Run migrations manually since Bazel doesn't package the migrations directory
pub(crate) async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // Create schema_version table first (if not exists)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL DEFAULT (unixepoch()),
            description TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Check current schema version
    let current_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    if current_version > SCHEMA_VERSION {
        anyhow::bail!(
            "Database schema version {} is newer than supported version {}. Please upgrade the application.",
            current_version,
            SCHEMA_VERSION
        );
    }

    if current_version == SCHEMA_VERSION {
        info!(
            "Database schema is up to date (version {})",
            current_version
        );
        return Ok(());
    }

    info!(
        "Migrating database from version {} to {}",
        current_version, SCHEMA_VERSION
    );

    // Initial schema
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            instance_id TEXT NOT NULL,
            title TEXT,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            is_public INTEGER DEFAULT 0,
            is_deleted INTEGER DEFAULT 0,
            metadata_json TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Create indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_conv_session_id ON conversations(session_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_conv_instance_id ON conversations(instance_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_conv_created_at ON conversations(created_at DESC)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_conv_deleted ON conversations(is_deleted)")
        .execute(pool)
        .await?;

    // Conversation entries table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conversation_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            entry_uuid TEXT UNIQUE NOT NULL,
            parent_uuid TEXT,
            entry_type TEXT NOT NULL,
            role TEXT,
            content TEXT,
            timestamp TEXT NOT NULL,
            raw_json TEXT NOT NULL,
            token_count INTEGER,
            model TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Indexes for conversation_entries
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_entry_conversation ON conversation_entries(conversation_id)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_entry_timestamp ON conversation_entries(timestamp)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_entry_uuid ON conversation_entries(entry_uuid)")
        .execute(pool)
        .await?;

    // Comments table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS comments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            entry_uuid TEXT,
            author TEXT NOT NULL DEFAULT 'anonymous',
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Indexes for comments
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_comment_conversation ON comments(conversation_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_comment_entry ON comments(entry_uuid)")
        .execute(pool)
        .await?;

    // Shares table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conversation_shares (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            share_token TEXT UNIQUE NOT NULL,
            title TEXT,
            description TEXT,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            expires_at INTEGER,
            access_count INTEGER DEFAULT 0,
            max_access_count INTEGER,
            password_hash TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Indexes for shares
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_share_token ON conversation_shares(share_token)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_share_expires ON conversation_shares(expires_at)")
        .execute(pool)
        .await?;

    // Tags tables
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            color TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conversation_tags (
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (conversation_id, tag_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Insert default tags
    sqlx::query("INSERT OR IGNORE INTO tags (name, color) VALUES ('bug', '#ff4444')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO tags (name, color) VALUES ('feature', '#44ff44')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO tags (name, color) VALUES ('refactor', '#4444ff')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO tags (name, color) VALUES ('question', '#ffaa44')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO tags (name, color) VALUES ('documentation', '#aa44ff')")
        .execute(pool)
        .await?;

    // FTS5 full-text search index on conversation_entries
    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS conversation_entries_fts USING fts5(
            content,
            entry_uuid UNINDEXED,
            conversation_id UNINDEXED,
            role UNINDEXED,
            content=conversation_entries,
            content_rowid=id,
            tokenize='porter unicode61 remove_diacritics 2'
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Triggers to keep FTS index in sync with conversation_entries
    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS conversation_entries_ai AFTER INSERT ON conversation_entries BEGIN
            INSERT INTO conversation_entries_fts(rowid, content, entry_uuid, conversation_id, role)
            VALUES (new.id, new.content, new.entry_uuid, new.conversation_id, new.role);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS conversation_entries_ad AFTER DELETE ON conversation_entries BEGIN
            INSERT INTO conversation_entries_fts(conversation_entries_fts, rowid, content, entry_uuid, conversation_id, role)
            VALUES ('delete', old.id, old.content, old.entry_uuid, old.conversation_id, old.role);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS conversation_entries_au AFTER UPDATE ON conversation_entries BEGIN
            INSERT INTO conversation_entries_fts(conversation_entries_fts, rowid, content, entry_uuid, conversation_id, role)
            VALUES ('delete', old.id, old.content, old.entry_uuid, old.conversation_id, old.role);
            INSERT INTO conversation_entries_fts(rowid, content, entry_uuid, conversation_id, role)
            VALUES (new.id, new.content, new.entry_uuid, new.conversation_id, new.role);
        END
        "#,
    )
    .execute(pool)
    .await?;

    // Rebuild FTS index to backfill any existing data
    sqlx::query("INSERT INTO conversation_entries_fts(conversation_entries_fts) VALUES('rebuild')")
        .execute(pool)
        .await?;

    // === Auth tables ===

    // Users table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            display_name TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            is_admin INTEGER NOT NULL DEFAULT 0,
            is_disabled INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_users_username ON users(username)")
        .execute(pool)
        .await?;

    // Sessions table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            csrf_token TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            last_active_at INTEGER NOT NULL DEFAULT (unixepoch()),
            user_agent TEXT,
            ip_address TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions(expires_at)")
        .execute(pool)
        .await?;

    // Instance permissions table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS instance_permissions (
            instance_id TEXT NOT NULL,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            role TEXT NOT NULL DEFAULT 'collaborator',
            granted_at INTEGER NOT NULL DEFAULT (unixepoch()),
            granted_by TEXT REFERENCES users(id),
            PRIMARY KEY (instance_id, user_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_instance_perms_user ON instance_permissions(user_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_instance_perms_instance ON instance_permissions(instance_id)",
    )
    .execute(pool)
    .await?;

    // Instance invitations table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS instance_invitations (
            invite_token TEXT PRIMARY KEY,
            instance_id TEXT NOT NULL,
            created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            role TEXT NOT NULL DEFAULT 'collaborator',
            max_uses INTEGER,
            use_count INTEGER NOT NULL DEFAULT 0,
            expires_at INTEGER,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_invitations_instance ON instance_invitations(instance_id)",
    )
    .execute(pool)
    .await?;

    // Input attributions table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS input_attributions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            instance_id TEXT NOT NULL,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            display_name TEXT NOT NULL,
            timestamp INTEGER NOT NULL DEFAULT (unixepoch()),
            entry_uuid TEXT,
            content_preview TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_attributions_instance ON input_attributions(instance_id, timestamp)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_attributions_entry ON input_attributions(entry_uuid)",
    )
    .execute(pool)
    .await?;

    // Server invites table (server-level invitation codes for registration)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS server_invites (
            token TEXT PRIMARY KEY,
            created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            label TEXT,
            max_uses INTEGER,
            use_count INTEGER NOT NULL DEFAULT 0,
            expires_at INTEGER,
            revoked INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_server_invites_created_by ON server_invites(created_by)",
    )
    .execute(pool)
    .await?;

    // Track which server invite a user registered with
    sqlx::query(
        "ALTER TABLE users ADD COLUMN server_invite_token TEXT REFERENCES server_invites(token)",
    )
    .execute(pool)
    .await
    .ok(); // .ok() swallows "duplicate column" on re-run

    // Server settings key-value store
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS server_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Add file_hash and file_mtime columns for import staleness detection
    sqlx::query("ALTER TABLE conversations ADD COLUMN file_hash TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE conversations ADD COLUMN file_mtime INTEGER")
        .execute(pool)
        .await
        .ok();
    // Add import_version to trigger re-import when import logic changes
    sqlx::query("ALTER TABLE conversations ADD COLUMN import_version INTEGER")
        .execute(pool)
        .await
        .ok();

    // Chat messages table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uuid TEXT UNIQUE NOT NULL,
            scope TEXT NOT NULL,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            display_name TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            forwarded_from TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chat_scope ON chat_messages(scope, created_at)")
        .execute(pool)
        .await?;

    // v5: Add topic column to chat_messages
    sqlx::query("ALTER TABLE chat_messages ADD COLUMN topic TEXT")
        .execute(pool)
        .await
        .ok(); // .ok() swallows "duplicate column" on re-run

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chat_scope_topic ON chat_messages(scope, topic, created_at)",
    )
    .execute(pool)
    .await?;

    // v6: Tasks table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uuid TEXT UNIQUE NOT NULL,
            title TEXT NOT NULL,
            body TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            priority INTEGER NOT NULL DEFAULT 0,
            instance_id TEXT,
            creator_id TEXT REFERENCES users(id),
            creator_name TEXT NOT NULL DEFAULT 'anonymous',
            sort_order REAL NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            completed_at INTEGER,
            is_deleted INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS task_tags (
            task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (task_id, tag_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_status_deleted ON tasks(status, is_deleted)")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_instance_status ON tasks(instance_id, status)",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_sort_order ON tasks(sort_order)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at DESC)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_task_tags_tag ON task_tags(tag_id)")
        .execute(pool)
        .await?;

    // v7: Add sent_text and conversation_id to tasks
    sqlx::query("ALTER TABLE tasks ADD COLUMN sent_text TEXT")
        .execute(pool)
        .await
        .ok(); // .ok() swallows "duplicate column" on re-run
    sqlx::query("ALTER TABLE tasks ADD COLUMN conversation_id TEXT")
        .execute(pool)
        .await
        .ok();

    // v8: Add task_id to input_attributions (structural task references)
    sqlx::query("ALTER TABLE input_attributions ADD COLUMN task_id INTEGER")
        .execute(pool)
        .await
        .ok(); // .ok() swallows "duplicate column" on re-run

    // v9: Task dispatches table — decouples dispatch records from task status
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS task_dispatches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            instance_id TEXT NOT NULL,
            sent_text TEXT NOT NULL,
            conversation_id TEXT,
            sent_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_task_dispatches_task ON task_dispatches(task_id)")
        .execute(pool)
        .await?;

    // Migrate existing sent tasks → dispatch records
    sqlx::query(
        r#"
        INSERT INTO task_dispatches (task_id, instance_id, sent_text, conversation_id, sent_at)
        SELECT id, COALESCE(instance_id, 'unknown'), COALESCE(sent_text, title),
               conversation_id, updated_at
        FROM tasks WHERE status = 'sent' AND is_deleted = 0
        "#,
    )
    .execute(pool)
    .await?;

    // Transition sent → in_progress
    sqlx::query("UPDATE tasks SET status = 'in_progress' WHERE status = 'sent' AND is_deleted = 0")
        .execute(pool)
        .await?;

    // v10: Per-user settings table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_settings (
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (user_id, key)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // v11: Instance inbox table (server-side attention model)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS instance_inbox (
            instance_id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            turn_count INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            metadata_json TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Record the schema version
    if current_version < SCHEMA_VERSION {
        sqlx::query("INSERT OR REPLACE INTO schema_version (version, description) VALUES (?, ?)")
            .bind(SCHEMA_VERSION)
            .bind("Add instance_inbox table for fleet attention model")
            .execute(pool)
            .await?;
        info!("Schema upgraded to version {}", SCHEMA_VERSION);
    }

    info!("Database migrations completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn get_stats_empty_db() {
        let pool = test_pool().await;
        let db = Database { pool };
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.conversations, 0);
        assert_eq!(stats.sessions, 0);
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.comments, 0);
        assert_eq!(stats.shares, 0);
        assert!(stats.database_size_bytes > 0);
    }

    #[tokio::test]
    async fn get_stats_with_data() {
        let pool = test_pool().await;

        // Insert a conversation
        sqlx::query(
            "INSERT INTO conversations (id, instance_id, created_at, updated_at) VALUES ('c1', 'i1', 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Insert an entry
        sqlx::query(
            "INSERT INTO conversation_entries (conversation_id, entry_uuid, entry_type, timestamp, raw_json) VALUES ('c1', 'e1', 'message', '2024-01-01', '{}')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let db = Database { pool };
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.conversations, 1);
        assert_eq!(stats.entries, 1);
    }

    #[tokio::test]
    async fn run_migrations_idempotent() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Run migrations twice — should not error
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn schema_version_recorded() {
        let pool = test_pool().await;
        let version: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn get_stats_counts_all_types() {
        let pool = test_pool().await;

        // Insert 2 conversations (one with session_id, one without)
        sqlx::query("INSERT INTO conversations (id, instance_id, session_id, created_at, updated_at) VALUES ('c1', 'i1', 'sess-1', 0, 0)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO conversations (id, instance_id, created_at, updated_at) VALUES ('c2', 'i1', 0, 0)")
            .execute(&pool).await.unwrap();

        // Insert 3 entries across 2 conversations
        for (cid, uuid) in [("c1", "e1"), ("c1", "e2"), ("c2", "e3")] {
            sqlx::query("INSERT INTO conversation_entries (conversation_id, entry_uuid, entry_type, timestamp, raw_json) VALUES (?, ?, 'message', '2024-01-01', '{}')")
                .bind(cid).bind(uuid)
                .execute(&pool).await.unwrap();
        }

        // Insert a comment
        sqlx::query("INSERT INTO comments (conversation_id, author, content, created_at) VALUES ('c1', 'user', 'hello', 0)")
            .execute(&pool).await.unwrap();

        // Insert a share
        sqlx::query("INSERT INTO conversation_shares (conversation_id, share_token, created_at) VALUES ('c1', 'tok-1', 0)")
            .execute(&pool).await.unwrap();

        let db = Database { pool };
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.conversations, 2);
        assert_eq!(stats.sessions, 1); // Only c1 has a session_id
        assert_eq!(stats.entries, 3);
        assert_eq!(stats.comments, 1);
        assert_eq!(stats.shares, 1);
    }

    #[tokio::test]
    async fn get_stats_excludes_deleted_conversations() {
        let pool = test_pool().await;

        sqlx::query("INSERT INTO conversations (id, instance_id, is_deleted, created_at, updated_at) VALUES ('c1', 'i1', 0, 0, 0)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO conversations (id, instance_id, is_deleted, created_at, updated_at) VALUES ('c2', 'i1', 1, 0, 0)")
            .execute(&pool).await.unwrap();

        let db = Database { pool };
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.conversations, 1); // Deleted conversation excluded
    }

    #[test]
    fn db_stats_serialization() {
        let stats = DbStats {
            conversations: 10,
            sessions: 5,
            entries: 100,
            comments: 3,
            shares: 2,
            database_size_bytes: 1024,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["conversations"], 10);
        assert_eq!(json["sessions"], 5);
        assert_eq!(json["entries"], 100);
        assert_eq!(json["comments"], 3);
        assert_eq!(json["shares"], 2);
        assert_eq!(json["database_size_bytes"], 1024);
    }

    #[tokio::test]
    async fn all_tables_exist_after_migration() {
        let pool = test_pool().await;

        // Verify core tables exist by querying each
        let tables = [
            "conversations",
            "conversation_entries",
            "comments",
            "conversation_shares",
        ];

        for table in tables {
            let count: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", table))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count.0, 0, "Table {} should exist and be empty", table);
        }

        // schema_version should have migration entries
        let sv_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schema_version")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            sv_count.0 > 0,
            "schema_version should have migration entries"
        );
    }
}
