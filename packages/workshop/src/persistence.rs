use anyhow::{Context, Result};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use toolpath_claude::ClaudeConvo;
use toolpath_convo::{ConversationProvider, WatcherEvent};
use tracing::{debug, error, info, warn};

use crate::models::{Conversation, ConversationEntry};
use crate::repository::ConversationRepository;

pub struct PersistenceService {
    repository: Arc<ConversationRepository>,
    buffer: Arc<Mutex<Vec<ConversationEntry>>>,
    flush_interval: Duration,
    batch_size: usize,
}

impl PersistenceService {
    pub fn new(repository: Arc<ConversationRepository>) -> Self {
        Self {
            repository,
            buffer: Arc::new(Mutex::new(Vec::new())),
            flush_interval: Duration::from_secs(5), // Flush every 5 seconds
            batch_size: 50,                         // Or when buffer reaches 50 entries
        }
    }

    pub async fn start(self: Arc<Self>) {
        info!(
            "Starting persistence service with flush interval: {:?}",
            self.flush_interval
        );

        // Spawn background task for periodic flushing
        let service = Arc::clone(&self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(service.flush_interval);
            loop {
                interval.tick().await;
                if let Err(e) = service.flush_buffer().await {
                    error!("Failed to flush persistence buffer: {}", e);
                }
            }
        });
    }

    pub async fn track_conversation(
        self: Arc<Self>,
        instance_id: String,
        session_id: String,
        project_path: String,
    ) {
        let conversation_id = uuid::Uuid::new_v4().to_string();

        // Create conversation record
        let conversation = Conversation::new(conversation_id.clone(), instance_id.clone())
            .with_session_id(session_id.clone());

        if let Err(e) = self.repository.create_conversation(&conversation).await {
            error!("Failed to create conversation record: {}", e);
            return;
        }

        info!(
            "📝 Tracking conversation {} for instance {}",
            conversation_id, instance_id
        );

        // Create a watcher for this conversation (MergingWatcher handles cross-poll tool result merge)
        let claude_convo = ClaudeConvo::new();
        let mut watcher = crate::ws::merging_watcher::MergingWatcher::new(
            claude_convo.clone(),
            project_path.clone(),
            session_id.clone(),
        );

        // Watch for new events
        let service = Arc::clone(&self);
        tokio::spawn(async move {
            let mut first_user_message = true;
            let mut check_interval = tokio::time::interval(Duration::from_secs(2));

            loop {
                check_interval.tick().await;

                match toolpath_convo::ConversationWatcher::poll(&mut watcher) {
                    Ok(events) => {
                        for event in events {
                            match event {
                                WatcherEvent::Turn(turn) | WatcherEvent::TurnUpdated(turn) => {
                                    // Extract title from first user message
                                    if first_user_message
                                        && let Some(title) =
                                            crate::handlers::extract_title_from_turn(&turn, 100)
                                    {
                                        first_user_message = false;
                                        if let Err(e) = service
                                            .repository
                                            .update_conversation_title(&conversation_id, &title)
                                            .await
                                        {
                                            error!("Failed to update conversation title: {}", e);
                                        }
                                    }

                                    // Add entry to buffer
                                    let entry_type = match &turn.role {
                                        toolpath_convo::Role::User => "user",
                                        toolpath_convo::Role::Assistant => "assistant",
                                        toolpath_convo::Role::System => "system",
                                        toolpath_convo::Role::Other(s) => s.as_str(),
                                    };
                                    let raw_json =
                                        serde_json::to_string(&*turn).unwrap_or_default();
                                    let db_entry = ConversationEntry::from_turn(
                                        conversation_id.clone(),
                                        &turn,
                                        entry_type.to_string(),
                                        raw_json,
                                    );

                                    service.add_to_buffer(db_entry).await;
                                }
                                WatcherEvent::Progress { .. } => {
                                    // Skip progress events for persistence
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Error getting new events: {}", e);
                        // Conversation might have ended
                        break;
                    }
                }
            }

            info!("Conversation {} tracking ended", conversation_id);
        });
    }

    pub(crate) async fn add_to_buffer(&self, entry: ConversationEntry) {
        let mut buffer = self.buffer.lock().await;
        buffer.push(entry);

        // Check if we should flush immediately
        if buffer.len() >= self.batch_size {
            drop(buffer); // Release lock before flushing
            if let Err(e) = self.flush_buffer().await {
                error!("Failed to flush buffer on batch size: {}", e);
            }
        }
    }

    async fn flush_buffer(&self) -> Result<()> {
        let mut buffer = self.buffer.lock().await;
        if buffer.is_empty() {
            return Ok(());
        }

        let entries: Vec<ConversationEntry> = buffer.drain(..).collect();
        let count = entries.len();

        // Release lock before database operation
        drop(buffer);

        self.repository
            .add_entries_batch(&entries)
            .await
            .context("Failed to persist conversation entries")?;

        debug!("Flushed {} conversation entries to database", count);
        Ok(())
    }

    pub async fn flush_all(&self) -> Result<()> {
        self.flush_buffer().await
    }
}

/// Service for monitoring a Claude instance and persisting its conversations
pub struct InstancePersistor {
    instance_id: String,
    project_path: String,
    persistence: Arc<PersistenceService>,
    active_sessions: Arc<Mutex<HashSet<String>>>,
}

impl InstancePersistor {
    pub fn new(
        instance_id: String,
        project_path: String,
        persistence: Arc<PersistenceService>,
    ) -> Self {
        Self {
            instance_id,
            project_path,
            persistence,
            active_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn start_monitoring(self: Arc<Self>) {
        info!(
            "🔍 Starting conversation monitoring for instance {}",
            self.instance_id
        );

        // Poll for new sessions with exponential backoff
        // Start at 2s, max 60s, timeout after 5 minutes without finding a session
        tokio::spawn(async move {
            const MIN_INTERVAL_SECS: u64 = 2;
            const MAX_INTERVAL_SECS: u64 = 60;
            const DISCOVERY_TIMEOUT_SECS: u64 = 300; // 5 minutes

            let mut interval_secs = MIN_INTERVAL_SECS;
            let started_at = std::time::Instant::now();
            let mut found_any_session = false;

            loop {
                tokio::time::sleep(Duration::from_secs(interval_secs)).await;

                match self.check_for_new_sessions().await {
                    Ok(found_new) => {
                        if found_new {
                            // Reset backoff when we find a session
                            interval_secs = MIN_INTERVAL_SECS;
                            found_any_session = true;
                        } else {
                            // Exponential backoff: double interval up to max
                            interval_secs = (interval_secs * 2).min(MAX_INTERVAL_SECS);
                        }
                    }
                    Err(e) => {
                        warn!("Error checking for new sessions: {}", e);
                        // On error, also apply backoff
                        interval_secs = (interval_secs * 2).min(MAX_INTERVAL_SECS);
                    }
                }

                // Timeout if no session found within 5 minutes
                if !found_any_session && started_at.elapsed().as_secs() > DISCOVERY_TIMEOUT_SECS {
                    info!(
                        "Session discovery timeout for instance {} (no session found in {}s)",
                        self.instance_id, DISCOVERY_TIMEOUT_SECS
                    );
                    break;
                }
            }
        });
    }

    /// Check for new sessions and return true if any were found
    async fn check_for_new_sessions(&self) -> Result<bool> {
        // Create provider on demand (ClaudeConvo is !Sync, cannot be stored in Arc)
        let claude_convo = ClaudeConvo::new();

        // List all conversations for this project (via trait)
        let sessions = claude_convo.list_conversations(&self.project_path)?;

        let mut active = self.active_sessions.lock().await;
        let mut found_new = false;

        for session_id in sessions {
            // Skip if we're already tracking this session
            if active.contains(&session_id) {
                continue;
            }

            // Check if this session is recent (within last 5 minutes)
            match claude_convo.load_metadata(&self.project_path, &session_id) {
                Ok(meta) => {
                    if let Some(started) = meta.started_at {
                        let five_minutes_ago = chrono::Utc::now() - chrono::Duration::minutes(5);
                        if started < five_minutes_ago {
                            continue; // Skip old sessions
                        }

                        info!(
                            "Found new session {} for instance {}",
                            session_id, self.instance_id
                        );

                        // Start tracking this session
                        active.insert(session_id.clone());
                        found_new = true;

                        self.persistence
                            .clone()
                            .track_conversation(
                                self.instance_id.clone(),
                                session_id,
                                self.project_path.clone(),
                            )
                            .await;
                    }
                }
                Err(e) => {
                    debug!("Failed to read metadata for session {}: {}", session_id, e);
                }
            }
        }

        Ok(found_new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::test_helpers;

    fn make_entry(conversation_id: &str, uuid: &str) -> ConversationEntry {
        ConversationEntry {
            id: None,
            conversation_id: conversation_id.to_string(),
            entry_uuid: uuid.to_string(),
            parent_uuid: None,
            entry_type: "user".to_string(),
            role: Some("user".to_string()),
            content: Some("test content".to_string()),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            raw_json: "{}".to_string(),
            token_count: None,
            model: None,
        }
    }

    #[tokio::test]
    async fn new_creates_service_with_defaults() {
        let repo = test_helpers::test_repository().await;
        let svc = PersistenceService::new(Arc::new(repo));
        assert_eq!(svc.batch_size, 50);
        assert_eq!(svc.flush_interval, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn flush_all_empty_buffer_is_noop() {
        let repo = test_helpers::test_repository().await;
        let svc = PersistenceService::new(Arc::new(repo));
        // Should succeed without error
        svc.flush_all().await.unwrap();
    }

    #[tokio::test]
    async fn add_to_buffer_and_flush_persists_entries() {
        let repo = Arc::new(test_helpers::test_repository().await);

        // Create a conversation first (foreign key constraint)
        let conv = crate::models::Conversation::new("conv-1".into(), "inst-1".into());
        repo.create_conversation(&conv).await.unwrap();

        let svc = PersistenceService::new(repo.clone());

        svc.add_to_buffer(make_entry("conv-1", "entry-1")).await;
        svc.add_to_buffer(make_entry("conv-1", "entry-2")).await;

        // Buffer should have 2 entries (below batch_size)
        assert_eq!(svc.buffer.lock().await.len(), 2);

        // Flush should persist them
        svc.flush_all().await.unwrap();

        // Buffer should now be empty
        assert_eq!(svc.buffer.lock().await.len(), 0);

        // Entries should be in the database
        let fetched = repo.get_conversation_with_entries("conv-1").await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().entries.len(), 2);
    }

    #[tokio::test]
    async fn flush_all_twice_is_safe() {
        let repo = Arc::new(test_helpers::test_repository().await);
        let conv = crate::models::Conversation::new("conv-1".into(), "inst-1".into());
        repo.create_conversation(&conv).await.unwrap();

        let svc = PersistenceService::new(repo.clone());
        svc.add_to_buffer(make_entry("conv-1", "entry-1")).await;
        svc.flush_all().await.unwrap();
        // Second flush on empty buffer should be fine
        svc.flush_all().await.unwrap();
    }

    #[tokio::test]
    async fn batch_size_triggers_auto_flush() {
        let repo = Arc::new(test_helpers::test_repository().await);
        let conv = crate::models::Conversation::new("conv-1".into(), "inst-1".into());
        repo.create_conversation(&conv).await.unwrap();

        let svc = PersistenceService {
            repository: repo.clone(),
            buffer: Arc::new(Mutex::new(Vec::new())),
            flush_interval: Duration::from_secs(5),
            batch_size: 3, // Small batch size for testing
        };

        // Add entries up to batch_size
        svc.add_to_buffer(make_entry("conv-1", "e1")).await;
        svc.add_to_buffer(make_entry("conv-1", "e2")).await;

        // Buffer should have 2 (below batch_size)
        assert_eq!(svc.buffer.lock().await.len(), 2);

        // Adding the 3rd should trigger auto-flush
        svc.add_to_buffer(make_entry("conv-1", "e3")).await;

        // Buffer should be empty after auto-flush
        assert_eq!(svc.buffer.lock().await.len(), 0);

        // All entries should be in the database
        let fetched = repo
            .get_conversation_with_entries("conv-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.entries.len(), 3);
    }
}
