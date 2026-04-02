-- Initial schema for Workshop conversation persistence

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
);

CREATE INDEX IF NOT EXISTS idx_conv_session_id ON conversations(session_id);
CREATE INDEX IF NOT EXISTS idx_conv_instance_id ON conversations(instance_id);
CREATE INDEX IF NOT EXISTS idx_conv_created_at ON conversations(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_conv_deleted ON conversations(is_deleted);

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
);

CREATE INDEX IF NOT EXISTS idx_entry_conversation ON conversation_entries(conversation_id);
CREATE INDEX IF NOT EXISTS idx_entry_timestamp ON conversation_entries(timestamp);
CREATE INDEX IF NOT EXISTS idx_entry_uuid ON conversation_entries(entry_uuid);

CREATE TABLE IF NOT EXISTS comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    entry_uuid TEXT,
    author TEXT NOT NULL DEFAULT 'anonymous',
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_comment_conversation ON comments(conversation_id);
CREATE INDEX IF NOT EXISTS idx_comment_entry ON comments(entry_uuid);

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
);

CREATE INDEX IF NOT EXISTS idx_share_token ON conversation_shares(share_token);
CREATE INDEX IF NOT EXISTS idx_share_expires ON conversation_shares(expires_at);

-- Tags for future categorization
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    color TEXT
);

CREATE TABLE IF NOT EXISTS conversation_tags (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (conversation_id, tag_id)
);

-- Insert some default tags
INSERT OR IGNORE INTO tags (name, color) VALUES
    ('bug', '#ff4444'),
    ('feature', '#44ff44'),
    ('refactor', '#4444ff'),
    ('question', '#ffaa44'),
    ('documentation', '#aa44ff');