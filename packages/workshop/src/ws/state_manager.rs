//! Global State Manager
//!
//! Manages instance state tracking, session claiming, and presence across all WebSocket connections.

use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, info, warn};

use crate::inference::ClaudeState;
use crate::instance_actor::InstanceHandle;
use crate::models::normalize_attribution_content;
use crate::repository::ConversationRepository;

use super::protocol::{PresenceUser, ServerMessage, WsUser};

/// Everything a transport layer knows about an input event.
/// Both WS handlers build this and call `handle_input()`. Nothing else.
pub struct InputContext {
    pub instance_id: String,
    pub data: String,
    pub connection_id: String,
    /// Optional — TUI has no authenticated user
    pub user: Option<InputUser>,
    pub task_id: Option<i64>,
}

pub struct InputUser {
    pub user_id: String,
    pub display_name: String,
}

/// Entry in the presence map: one per WebSocket connection.
#[derive(Debug, Clone)]
struct PresenceEntry {
    user_id: String,
    display_name: String,
}

/// Terminal lock timeout in seconds (2 minutes)
pub const TERMINAL_LOCK_TIMEOUT_SECS: i64 = 120;

/// Terminal lock state for an instance
#[derive(Debug, Clone)]
pub struct TerminalLock {
    pub holder_connection_id: String,
    pub holder_user_id: String,
    pub holder_display_name: String,
    pub last_activity: DateTime<Utc>,
}

/// Data stored for the first input(s) to an instance before session discovery.
/// Accumulates content prefixes so the discovery loop can verify that a
/// candidate session actually contains input sent to *this* instance.
#[derive(Debug, Clone)]
pub struct FirstInputData {
    pub timestamp: DateTime<Utc>,
    /// Normalized content prefixes of inputs sent while session_id was None.
    /// Used for content-matching during session discovery.
    pub content_prefixes: Vec<String>,
    /// Accumulator for keystroke-by-keystroke input.  Characters accumulate
    /// here until a `\r`/`\n` arrives, at which point the completed line is
    /// flushed into `content_prefixes`.  Full-message input (containing a
    /// newline) bypasses the accumulator and stores directly.
    pending_line: String,
}

/// Maximum number of content prefixes to store per instance.
const FIRST_INPUT_PREFIX_CAP: usize = 20;

/// A pending attribution: recorded when a user sends input via WebSocket,
/// consumed when the conversation watcher sees the corresponding User entry.
/// Content-matched rather than timestamp-correlated.
#[derive(Debug, Clone)]
pub struct PendingAttribution {
    pub user_id: String,
    pub display_name: String,
    /// First 200 chars of the input, normalized (trimmed, whitespace-collapsed)
    pub content_prefix: String,
    pub timestamp: DateTime<Utc>,
    /// Optional task ID if this input was sent on behalf of a task
    pub task_id: Option<i64>,
}

/// Broadcast channel for state changes across all instances
/// Tuple: (instance_id, state, terminal_stale)
pub type StateBroadcast = broadcast::Sender<(String, ClaudeState, bool)>;

/// Broadcast channel for instance lifecycle events (created/stopped)
pub type LifecycleBroadcast = broadcast::Sender<ServerMessage>;

/// Conversation event broadcast from server-owned watcher to consumers.
#[derive(Debug, Clone)]
pub enum ConversationEvent {
    /// Full conversation snapshot (sent after initial load).
    Full {
        instance_id: String,
        turns: Vec<serde_json::Value>,
    },
    /// Incremental update (new turns appended).
    Update {
        instance_id: String,
        turns: Vec<serde_json::Value>,
    },
}

/// Create a new state broadcast channel
pub fn create_state_broadcast() -> StateBroadcast {
    let (tx, _) = broadcast::channel(256);
    tx
}

/// Per-instance tracking (lives in GlobalStateManager).
///
/// State detection and conversation tracking are now owned by each actor's
/// ProcessDriver. The tracker only holds metadata needed for presence,
/// session claiming, and lifecycle management.
pub(crate) struct InstanceTracker {
    pub handle: InstanceHandle,
    pub working_dir: String,
    pub created_at: DateTime<Utc>,
    pub is_claude: bool,
}

impl InstanceTracker {
    pub fn new(
        handle: InstanceHandle,
        working_dir: String,
        created_at: DateTime<Utc>,
        is_claude: bool,
    ) -> Self {
        Self {
            handle,
            working_dir,
            created_at,
            is_claude,
        }
    }
}

/// Manager for all instance state trackers
pub struct GlobalStateManager {
    trackers: RwLock<HashMap<String, InstanceTracker>>,
    broadcast_tx: StateBroadcast,
    lifecycle_tx: LifecycleBroadcast,
    /// Sessions that have been claimed by instances (session_id -> instance_id)
    /// Prevents multiple instances from claiming the same Claude session
    claimed_sessions: Arc<RwLock<HashMap<String, String>>>,
    /// First input data per instance (instance_id -> FirstInputData).
    /// Used for causation-based session discovery: the timestamp gates when
    /// discovery starts, and content prefixes verify the candidate session
    /// actually contains input sent to *this* instance.
    first_input_at: Arc<RwLock<HashMap<String, FirstInputData>>>,
    /// Presence tracking: instance_id -> (connection_id -> PresenceEntry)
    presence: RwLock<HashMap<String, HashMap<String, PresenceEntry>>>,
    /// Pending attributions: instance_id -> queue of (user, content_prefix).
    /// Pushed by the WebSocket input handler, consumed by the conversation watcher.
    /// Content-matched to conversation entries for reliable attribution.
    pending_attributions: Arc<RwLock<HashMap<String, VecDeque<PendingAttribution>>>>,
    /// Terminal locks: instance_id -> lock holder info
    terminal_locks: RwLock<HashMap<String, TerminalLock>>,
    /// Timestamp when each instance entered its current state
    state_entered_at: RwLock<HashMap<String, DateTime<Utc>>>,
}

impl GlobalStateManager {
    pub fn new(broadcast_tx: StateBroadcast) -> Self {
        let (lifecycle_tx, _) = broadcast::channel(64);
        Self {
            trackers: RwLock::new(HashMap::new()),
            broadcast_tx,
            lifecycle_tx,
            claimed_sessions: Arc::new(RwLock::new(HashMap::new())),
            first_input_at: Arc::new(RwLock::new(HashMap::new())),
            presence: RwLock::new(HashMap::new()),
            pending_attributions: Arc::new(RwLock::new(HashMap::new())),
            terminal_locks: RwLock::new(HashMap::new()),
            state_entered_at: RwLock::new(HashMap::new()),
        }
    }

    /// Expose the state broadcast sender (for passing to drivers via SpawnOptions).
    pub fn broadcast_tx(&self) -> &StateBroadcast {
        &self.broadcast_tx
    }

    /// Expose the lifecycle broadcast sender (for passing to drivers via SpawnOptions).
    pub fn lifecycle_tx(&self) -> &LifecycleBroadcast {
        &self.lifecycle_tx
    }

    /// Spawn a background task that subscribes to state broadcasts and:
    /// 1. Tracks `state_entered_at` timestamps (when state type changes)
    /// 2. Detects state transitions for inbox (completed_turn, needs_input, etc.)
    pub fn start_inbox_watcher(self: &Arc<Self>, repository: Arc<ConversationRepository>) {
        let mut state_rx = self.broadcast_tx.subscribe();
        let gsm = Arc::clone(self);
        tokio::spawn(async move {
            // Track previous state per instance for transition detection
            let mut prev_states: HashMap<String, ClaudeState> = HashMap::new();
            loop {
                match state_rx.recv().await {
                    Ok((instance_id, state, _stale)) => {
                        let prev = prev_states.get(&instance_id);

                        // Track state_entered_at: record timestamp when state type changes
                        let state_type_changed = prev
                            .map(|p| std::mem::discriminant(p) != std::mem::discriminant(&state))
                            .unwrap_or(true);
                        if state_type_changed {
                            gsm.set_state_entered_at(&instance_id, Utc::now()).await;
                        }

                        // Inbox logic: detect state transitions and upsert/clear inbox items
                        if let Some(prev) = prev {
                            let prev_active = matches!(
                                prev,
                                ClaudeState::Thinking
                                    | ClaudeState::Responding
                                    | ClaudeState::ToolExecuting { .. }
                            );
                            let now_idle = matches!(state, ClaudeState::Idle);
                            let now_waiting = matches!(state, ClaudeState::WaitingForInput { .. });
                            let now_active = matches!(
                                state,
                                ClaudeState::Thinking
                                    | ClaudeState::Responding
                                    | ClaudeState::ToolExecuting { .. }
                            );

                            // Active → Idle: completed a turn
                            if prev_active && now_idle {
                                match repository
                                    .upsert_inbox_item(&instance_id, "completed_turn", None)
                                    .await
                                {
                                    Ok(item) => {
                                        gsm.broadcast_lifecycle(ServerMessage::InboxUpdate {
                                            instance_id: instance_id.clone(),
                                            item: Some(item),
                                        });
                                    }
                                    Err(e) => {
                                        warn!("[INBOX] Failed to upsert completed_turn: {}", e)
                                    }
                                }
                            }

                            // → WaitingForInput: needs user action
                            if now_waiting && !matches!(prev, ClaudeState::WaitingForInput { .. }) {
                                let metadata = match &state {
                                    ClaudeState::WaitingForInput { prompt: Some(p) } => {
                                        Some(serde_json::json!({"prompt": p}).to_string())
                                    }
                                    _ => None,
                                };
                                match repository
                                    .upsert_inbox_item(
                                        &instance_id,
                                        "needs_input",
                                        metadata.as_deref(),
                                    )
                                    .await
                                {
                                    Ok(item) => {
                                        gsm.broadcast_lifecycle(ServerMessage::InboxUpdate {
                                            instance_id: instance_id.clone(),
                                            item: Some(item),
                                        });
                                    }
                                    Err(e) => {
                                        warn!("[INBOX] Failed to upsert needs_input: {}", e)
                                    }
                                }
                            }

                            // Was WaitingForInput, now isn't: user responded, clear needs_input
                            if matches!(prev, ClaudeState::WaitingForInput { .. }) && !now_waiting {
                                match repository
                                    .clear_inbox_by_type(&instance_id, "needs_input")
                                    .await
                                {
                                    Ok(true) => {
                                        gsm.broadcast_lifecycle(ServerMessage::InboxUpdate {
                                            instance_id: instance_id.clone(),
                                            item: None,
                                        });
                                    }
                                    Ok(false) => {} // No item to clear
                                    Err(e) => {
                                        warn!("[INBOX] Failed to clear needs_input: {}", e)
                                    }
                                }
                            }

                            // Was Idle, now active: user sent new work, clear completed_turn
                            if !prev_active && now_active {
                                match repository
                                    .clear_inbox_by_type(&instance_id, "completed_turn")
                                    .await
                                {
                                    Ok(true) => {
                                        gsm.broadcast_lifecycle(ServerMessage::InboxUpdate {
                                            instance_id: instance_id.clone(),
                                            item: None,
                                        });
                                    }
                                    Ok(false) => {} // No item to clear
                                    Err(e) => {
                                        warn!("[INBOX] Failed to clear completed_turn: {}", e)
                                    }
                                }
                            }
                        }

                        prev_states.insert(instance_id, state);
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("[INBOX] State broadcast lagged by {} messages", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("[INBOX] State broadcast channel closed, stopping inbox watcher");
                        break;
                    }
                }
            }
        });
    }

    /// Get the timestamp when an instance entered its current state.
    pub async fn get_state_entered_at(&self, instance_id: &str) -> Option<DateTime<Utc>> {
        self.state_entered_at.read().await.get(instance_id).copied()
    }

    /// Record when an instance entered its current state.
    pub async fn set_state_entered_at(&self, instance_id: &str, timestamp: DateTime<Utc>) {
        self.state_entered_at
            .write()
            .await
            .insert(instance_id.to_string(), timestamp);
    }

    /// Try to claim a session for an instance. Returns true if successful.
    /// Returns false if the session is already claimed by another instance.
    pub async fn try_claim_session(&self, session_id: &str, instance_id: &str) -> bool {
        let mut claimed = self.claimed_sessions.write().await;
        match claimed.get(session_id) {
            Some(owner) if owner == instance_id => true, // Already claimed by us
            Some(_) => false,                            // Claimed by another instance
            None => {
                claimed.insert(session_id.to_string(), instance_id.to_string());
                info!(
                    "[SESSION] Instance {} claimed session {}",
                    instance_id, session_id
                );
                true
            }
        }
    }

    /// Release a session claim (called when instance is unregistered)
    pub async fn release_session(&self, instance_id: &str) {
        let mut claimed = self.claimed_sessions.write().await;
        claimed.retain(|_, owner| owner != instance_id);
    }

    /// Get all sessions claimed by other instances (for filtering candidates)
    pub async fn get_claimed_sessions(&self) -> HashSet<String> {
        self.claimed_sessions.read().await.keys().cloned().collect()
    }

    /// Get a cloned Arc to the claimed_sessions map for use by DriverContext.
    pub fn claimed_sessions_arc(&self) -> Arc<RwLock<HashMap<String, String>>> {
        Arc::clone(&self.claimed_sessions)
    }

    /// Get a cloned Arc to the first_input_data map for use by DriverContext.
    pub fn first_input_data_arc(&self) -> Arc<RwLock<HashMap<String, FirstInputData>>> {
        Arc::clone(&self.first_input_at)
    }

    /// Get a cloned Arc to the pending_attributions map for use by DriverContext.
    pub fn pending_attributions_arc(
        &self,
    ) -> Arc<RwLock<HashMap<String, VecDeque<PendingAttribution>>>> {
        Arc::clone(&self.pending_attributions)
    }

    /// Record input for an instance before session discovery.
    /// First call: creates the entry with timestamp, returns true.
    /// Subsequent calls: accumulates content, returns false.
    ///
    /// Content handling depends on whether input arrives as individual
    /// keystrokes (terminal) or as a complete message (ConversationView):
    ///
    /// - **Full message** (contains `\r` or `\n`): normalized and stored
    ///   directly as a content prefix.
    /// - **Individual keystroke** (no newline): accumulated in a line buffer.
    ///   When a `\r`/`\n` arrives, the accumulated line is flushed as a
    ///   single content prefix.
    ///
    /// This ensures that both input paths produce the same content prefixes
    /// for session discovery matching.
    pub async fn mark_first_input(&self, instance_id: &str, content: &str) -> bool {
        let mut map = self.first_input_at.write().await;
        let is_first = !map.contains_key(instance_id);

        let data = map.entry(instance_id.to_string()).or_insert_with(|| {
            let now = Utc::now();
            info!(
                "[SESSION] Marked first input for instance {} at {}",
                instance_id, now
            );
            FirstInputData {
                timestamp: now,
                content_prefixes: vec![],
                pending_line: String::new(),
            }
        });

        if data.content_prefixes.len() >= FIRST_INPUT_PREFIX_CAP {
            return is_first;
        }

        // Check if content contains a newline (full message or end-of-line keystroke)
        let has_newline = content.contains('\r') || content.contains('\n');

        if has_newline {
            // Flush: prepend any accumulated pending_line to content, then
            // split on newlines and store each non-empty line as a prefix.
            let combined = if data.pending_line.is_empty() {
                content.to_string()
            } else {
                let mut s = std::mem::take(&mut data.pending_line);
                s.push_str(content);
                s
            };

            for part in combined.split(['\r', '\n']) {
                if data.content_prefixes.len() >= FIRST_INPUT_PREFIX_CAP {
                    break;
                }
                let prefix = normalize_attribution_content(part);
                if !prefix.is_empty() {
                    data.content_prefixes.push(prefix);
                }
            }
        } else {
            // Individual keystroke — accumulate in the line buffer.
            // Handle backspace/DEL by removing the last character, and
            // skip other control characters and escape sequences that
            // won't appear in the JSONL user entry text.
            let first_byte = content.as_bytes().first().copied().unwrap_or(0);
            if first_byte == 0x7f || first_byte == 0x08 {
                // Backspace (0x7f DEL or 0x08 BS): remove last char to
                // mirror what the PTY line discipline delivers to Claude.
                data.pending_line.pop();
            } else if first_byte >= 0x20 {
                data.pending_line.push_str(content);
            }
            // Other control chars (0x00-0x1f except 0x08) and escape
            // sequences (0x1b...) are silently dropped — they don't
            // appear in JSONL user entry text.
        }

        is_first
    }

    /// Get the timestamp of first input for an instance, if any.
    pub async fn get_first_input_at(&self, instance_id: &str) -> Option<DateTime<Utc>> {
        self.first_input_at
            .read()
            .await
            .get(instance_id)
            .map(|d| d.timestamp)
    }

    /// Get content prefixes stored for session discovery verification.
    /// Returns an empty vec if no inputs have been recorded.
    #[allow(dead_code)]
    pub async fn get_discovery_content_prefixes(&self, instance_id: &str) -> Vec<String> {
        self.first_input_at
            .read()
            .await
            .get(instance_id)
            .map(|d| d.content_prefixes.clone())
            .unwrap_or_default()
    }

    // =========================================================================
    // Pending attribution tracking (in-process, content-matched)
    // =========================================================================

    /// Record that a WebSocket user just sent input to an instance.
    /// The conversation watcher will consume this when the corresponding
    /// User entry appears in the conversation.
    pub async fn push_pending_attribution(
        &self,
        instance_id: &str,
        user_id: String,
        display_name: String,
        content: &str,
        task_id: Option<i64>,
    ) {
        let prefix = normalize_attribution_content(content);
        if prefix.is_empty() {
            return;
        }
        let mut map = self.pending_attributions.write().await;
        let queue = map
            .entry(instance_id.to_string())
            .or_insert_with(VecDeque::new);
        queue.push_back(PendingAttribution {
            user_id,
            display_name,
            content_prefix: prefix,
            timestamp: Utc::now(),
            task_id,
        });
        // Cap the queue to prevent unbounded growth
        while queue.len() > 50 {
            queue.pop_front();
        }
    }

    /// Try to consume a pending attribution that matches a conversation entry's content.
    /// Delegates to the free function in handlers::conversations::format.
    #[allow(dead_code)]
    pub async fn consume_pending_attribution(
        &self,
        instance_id: &str,
        entry_content: &str,
    ) -> Option<PendingAttribution> {
        crate::handlers::conversations::format::consume_pending_attribution(
            &self.pending_attributions,
            instance_id,
            entry_content,
        )
        .await
    }

    /// Expose pending_attributions for direct use by format_turn_with_attribution.
    pub fn pending_attributions_lock(
        &self,
    ) -> &RwLock<HashMap<String, VecDeque<PendingAttribution>>> {
        &self.pending_attributions
    }

    // =========================================================================
    // Terminal lock tracking
    // =========================================================================

    /// Try to acquire the terminal lock for an instance.
    /// Succeeds if unclaimed or if current holder's lock has expired.
    /// Returns true if lock was acquired.
    pub async fn try_acquire_terminal_lock(
        &self,
        instance_id: &str,
        connection_id: &str,
        user: &WsUser,
    ) -> bool {
        let mut locks = self.terminal_locks.write().await;
        let now = Utc::now();

        if let Some(existing) = locks.get(instance_id) {
            // Already held by this user (any connection)
            if existing.holder_user_id == user.user_id {
                // Update connection_id in case it's a different tab
                locks.insert(
                    instance_id.to_string(),
                    TerminalLock {
                        holder_connection_id: connection_id.to_string(),
                        holder_user_id: user.user_id.clone(),
                        holder_display_name: user.display_name.clone(),
                        last_activity: now,
                    },
                );
                return true;
            }

            // Check if expired
            let elapsed = (now - existing.last_activity).num_seconds();
            if elapsed < TERMINAL_LOCK_TIMEOUT_SECS {
                return false; // Still active, can't take it
            }
            // Expired — fall through to acquire
            info!(
                "[TERMINAL-LOCK] Lock expired for {} (held by {}, inactive {}s), granting to {}",
                instance_id, existing.holder_display_name, elapsed, user.display_name
            );
        }

        locks.insert(
            instance_id.to_string(),
            TerminalLock {
                holder_connection_id: connection_id.to_string(),
                holder_user_id: user.user_id.clone(),
                holder_display_name: user.display_name.clone(),
                last_activity: now,
            },
        );
        info!(
            "[TERMINAL-LOCK] {} acquired lock for instance {}",
            user.display_name, instance_id
        );
        true
    }

    /// Update last_activity on the terminal lock (called on every Input from holder).
    pub async fn touch_terminal_lock(&self, instance_id: &str, connection_id: &str) {
        let mut locks = self.terminal_locks.write().await;
        if let Some(lock) = locks.get_mut(instance_id) {
            // Touch if held by this connection OR same user (multi-tab)
            if lock.holder_connection_id == connection_id {
                lock.last_activity = Utc::now();
            }
        }
    }

    /// Release the terminal lock (voluntary release or disconnect cleanup).
    /// Returns true if a lock was actually released.
    pub async fn release_terminal_lock(&self, instance_id: &str, connection_id: &str) -> bool {
        let mut locks = self.terminal_locks.write().await;
        if let Some(lock) = locks.get(instance_id)
            && lock.holder_connection_id == connection_id
        {
            let holder = lock.holder_display_name.clone();
            locks.remove(instance_id);
            info!(
                "[TERMINAL-LOCK] {} released lock for instance {}",
                holder, instance_id
            );
            return true;
        }
        false
    }

    /// Get the current terminal lock state for an instance.
    pub async fn get_terminal_lock(&self, instance_id: &str) -> Option<TerminalLock> {
        self.terminal_locks.read().await.get(instance_id).cloned()
    }

    /// Reconcile terminal lock with presence:
    /// - If holder disconnected, clear the lock.
    /// - If only one user is present and no lock exists, auto-grant.
    ///   Returns true if lock state changed.
    pub async fn reconcile_terminal_lock_with_presence(&self, instance_id: &str) -> bool {
        let presence = self.presence.read().await;
        let instance_presence = presence.get(instance_id);
        let connection_ids: Vec<String> = instance_presence
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();
        let unique_users = instance_presence
            .map(Self::dedupe_presence)
            .unwrap_or_default();
        drop(presence);

        let mut locks = self.terminal_locks.write().await;

        // If holder's connection is no longer present, clear the lock
        if let Some(lock) = locks.get(instance_id)
            && !connection_ids.contains(&lock.holder_connection_id)
        {
            info!(
                "[TERMINAL-LOCK] Holder {} disconnected from {}, clearing lock",
                lock.holder_display_name, instance_id
            );
            locks.remove(instance_id);
            return true;
        }

        // Auto-grant to sole user if no lock exists
        if locks.get(instance_id).is_none() && unique_users.len() == 1 {
            // Find the connection_id for this sole user
            let sole_user = &unique_users[0];
            if let Some(conn_id) = connection_ids.first() {
                locks.insert(
                    instance_id.to_string(),
                    TerminalLock {
                        holder_connection_id: conn_id.clone(),
                        holder_user_id: sole_user.user_id.clone(),
                        holder_display_name: sole_user.display_name.clone(),
                        last_activity: Utc::now(),
                    },
                );
                debug!(
                    "[TERMINAL-LOCK] Auto-granted lock to sole user {} for instance {}",
                    sole_user.display_name, instance_id
                );
                return true;
            }
        }

        false
    }

    /// Register a new instance. The actor's ProcessDriver handles state detection
    /// and conversation tracking. The GSM only needs the tracker for presence,
    /// session claiming, and lifecycle.
    pub async fn register_instance(
        &self,
        instance_id: String,
        handle: InstanceHandle,
        working_dir: String,
        created_at: DateTime<Utc>,
        is_claude: bool,
    ) {
        let tracker = InstanceTracker::new(handle, working_dir, created_at, is_claude);
        self.trackers.write().await.insert(instance_id, tracker);
    }

    /// Unregister an instance
    pub async fn unregister_instance(&self, instance_id: &str) {
        self.trackers.write().await.remove(instance_id);
        // Release any claimed sessions
        self.release_session(instance_id).await;
        // Clean up first_input_at tracking
        self.first_input_at.write().await.remove(instance_id);
        // Clean up pending attributions
        self.pending_attributions.write().await.remove(instance_id);
        // Clean up terminal lock
        self.terminal_locks.write().await.remove(instance_id);
        // Clean up state_entered_at
        self.state_entered_at.write().await.remove(instance_id);
    }

    /// Get a handle for an instance
    pub async fn get_handle(&self, instance_id: &str) -> Option<InstanceHandle> {
        self.trackers
            .read()
            .await
            .get(instance_id)
            .map(|t| t.handle.clone())
    }

    /// Get all instance handles (for disconnect cleanup across all instances).
    pub async fn all_handles(&self) -> Vec<(String, InstanceHandle)> {
        self.trackers
            .read()
            .await
            .iter()
            .map(|(id, t)| (id.clone(), t.handle.clone()))
            .collect()
    }

    /// Get tracker info for an instance
    pub async fn get_tracker_info(
        &self,
        instance_id: &str,
    ) -> Option<(InstanceHandle, String, DateTime<Utc>, bool)> {
        self.trackers.read().await.get(instance_id).map(|t| {
            (
                t.handle.clone(),
                t.working_dir.clone(),
                t.created_at,
                t.is_claude,
            )
        })
    }

    /// Unified input handler — the single entry point for all terminal input.
    ///
    /// Both the multiplexed WebSocket handler (web) and the TUI proxy call
    /// this instead of duplicating the input ceremony. Performs:
    ///
    /// 1. Session discovery content tracking (`mark_first_input`)
    /// 2. Terminal lock keepalive (`touch_terminal_lock`)
    /// 3. Real-time attribution tracking (`push_pending_attribution`)
    /// 4. DB attribution persistence (`record_input_attribution`)
    /// 5. PTY write (`handle.write_input`)
    ///
    /// Steps 2–4 are skipped when `ctx.user` is `None` (TUI path).
    /// State detection (idle→thinking) is handled by the actor's ProcessDriver
    /// via `driver.on_input()` in the WriteInput command handler.
    pub async fn handle_input(
        &self,
        ctx: InputContext,
        repository: Option<&Arc<ConversationRepository>>,
    ) -> Result<(), String> {
        let handle = self
            .get_handle(&ctx.instance_id)
            .await
            .ok_or_else(|| "Instance handle not found".to_string())?;

        // 2. Session discovery content tracking
        if handle.get_session_id().await.is_none() {
            self.mark_first_input(&ctx.instance_id, &ctx.data).await;
        }

        // 3. Terminal lock keepalive
        self.touch_terminal_lock(&ctx.instance_id, &ctx.connection_id)
            .await;

        // 4–5. Attribution (only when user is authenticated)
        if let Some(ref user) = ctx.user {
            let trimmed = ctx.data.trim();
            if !trimmed.is_empty() && trimmed != "\r" && trimmed != "\n" {
                // 4. Push in-process pending attribution for real-time content matching
                self.push_pending_attribution(
                    &ctx.instance_id,
                    user.user_id.clone(),
                    user.display_name.clone(),
                    trimmed,
                    ctx.task_id,
                )
                .await;

                // 5. Persist to DB for historical audit
                if let Some(repo) = repository {
                    let attr = crate::models::InputAttribution {
                        id: None,
                        instance_id: ctx.instance_id.clone(),
                        user_id: user.user_id.clone(),
                        display_name: user.display_name.clone(),
                        timestamp: chrono::Utc::now().timestamp(),
                        entry_uuid: None,
                        content_preview: Some(trimmed.chars().take(100).collect()),
                        task_id: ctx.task_id,
                    };
                    let repo = repo.clone();
                    let inst_id = ctx.instance_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = repo.record_input_attribution(&attr).await {
                            warn!(instance = %inst_id, "Failed to record input attribution: {}", e);
                        }
                    });
                }
            }
        }

        // 6. PTY write
        handle
            .write_input(&ctx.data)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Subscribe to state broadcasts
    /// Returns receiver for (instance_id, state, terminal_stale) tuples
    pub fn subscribe(&self) -> broadcast::Receiver<(String, ClaudeState, bool)> {
        self.broadcast_tx.subscribe()
    }

    /// Subscribe to instance lifecycle broadcasts (InstanceCreated/InstanceStopped)
    pub fn subscribe_lifecycle(&self) -> broadcast::Receiver<ServerMessage> {
        self.lifecycle_tx.subscribe()
    }

    /// Broadcast an instance lifecycle event to all connected WebSocket clients
    pub fn broadcast_lifecycle(&self, msg: ServerMessage) {
        let _ = self.lifecycle_tx.send(msg);
    }

    // =========================================================================
    // Presence tracking
    // =========================================================================

    /// Add a user to an instance's presence set. Returns the updated presence list.
    pub async fn add_presence(
        &self,
        instance_id: &str,
        connection_id: &str,
        user: &WsUser,
    ) -> Vec<PresenceUser> {
        let mut presence = self.presence.write().await;
        let instance_presence = presence
            .entry(instance_id.to_string())
            .or_insert_with(HashMap::new);
        instance_presence.insert(
            connection_id.to_string(),
            PresenceEntry {
                user_id: user.user_id.clone(),
                display_name: user.display_name.clone(),
            },
        );
        Self::dedupe_presence(instance_presence)
    }

    /// Remove a connection from a specific instance's presence set.
    pub async fn remove_presence_from_instance(
        &self,
        instance_id: &str,
        connection_id: &str,
    ) -> Vec<PresenceUser> {
        let mut presence = self.presence.write().await;
        if let Some(instance_presence) = presence.get_mut(instance_id) {
            instance_presence.remove(connection_id);
            let result = Self::dedupe_presence(instance_presence);
            if instance_presence.is_empty() {
                presence.remove(instance_id);
            }
            result
        } else {
            vec![]
        }
    }

    /// Remove a connection from ALL instance presence sets. Returns affected instance_ids.
    pub async fn remove_presence_all(
        &self,
        connection_id: &str,
    ) -> Vec<(String, Vec<PresenceUser>)> {
        let mut presence = self.presence.write().await;
        let mut updates = Vec::new();
        let mut empty_instances = Vec::new();
        for (instance_id, instance_presence) in presence.iter_mut() {
            if instance_presence.remove(connection_id).is_some() {
                let users = Self::dedupe_presence(instance_presence);
                updates.push((instance_id.clone(), users));
                if instance_presence.is_empty() {
                    empty_instances.push(instance_id.clone());
                }
            }
        }
        for id in empty_instances {
            presence.remove(&id);
        }
        updates
    }

    /// Deduplicate presence by user_id (a user may have multiple connections).
    fn dedupe_presence(entries: &HashMap<String, PresenceEntry>) -> Vec<PresenceUser> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for entry in entries.values() {
            if seen.insert(entry.user_id.clone()) {
                result.push(PresenceUser {
                    user_id: entry.user_id.clone(),
                    display_name: entry.display_name.clone(),
                });
            }
        }
        result
    }
}

#[cfg(test)]
impl GlobalStateManager {
    /// Insert a minimal tracker for testing conversation data flow.
    /// Does NOT spawn any background tasks (watcher, state manager).
    pub(crate) async fn insert_test_tracker(&self, instance_id: &str, handle: InstanceHandle) {
        let tracker = InstanceTracker::new(handle, "/tmp/test".to_string(), Utc::now(), true);
        self.trackers
            .write()
            .await
            .insert(instance_id.to_string(), tracker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_mark_first_input_returns_true_then_false() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // First call returns true (full message with newline)
        assert!(state_mgr.mark_first_input("inst-1", "hello\r").await);

        // Second call returns false (already recorded)
        assert!(!state_mgr.mark_first_input("inst-1", "world\r").await);

        // Different instance returns true
        assert!(state_mgr.mark_first_input("inst-2", "hi\r").await);
    }

    #[tokio::test]
    async fn test_mark_first_input_full_messages() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Full messages (contain newlines) store directly as prefixes
        state_mgr
            .mark_first_input("inst-1", "first message\r")
            .await;
        state_mgr
            .mark_first_input("inst-1", "second message\r")
            .await;
        state_mgr
            .mark_first_input("inst-1", "third message\r")
            .await;

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), 3);
        assert!(prefixes[0].starts_with("first"));
        assert!(prefixes[1].starts_with("second"));
        assert!(prefixes[2].starts_with("third"));
    }

    #[tokio::test]
    async fn test_mark_first_input_keystroke_accumulation() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Individual keystrokes accumulate in pending_line
        state_mgr.mark_first_input("inst-1", "H").await;
        state_mgr.mark_first_input("inst-1", "e").await;
        state_mgr.mark_first_input("inst-1", "l").await;
        state_mgr.mark_first_input("inst-1", "l").await;
        state_mgr.mark_first_input("inst-1", "o").await;

        // No prefix stored yet (no newline to flush)
        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert!(prefixes.is_empty());

        // Newline flushes the accumulated line
        state_mgr.mark_first_input("inst-1", "\r").await;

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), 1);
        assert_eq!(prefixes[0], "Hello");
    }

    #[tokio::test]
    async fn test_mark_first_input_skips_control_chars() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Escape sequences and control chars should not accumulate
        state_mgr.mark_first_input("inst-1", "\x1b[A").await; // up arrow
        state_mgr.mark_first_input("inst-1", "\x7f").await; // backspace
        state_mgr.mark_first_input("inst-1", "H").await;
        state_mgr.mark_first_input("inst-1", "i").await;
        state_mgr.mark_first_input("inst-1", "\r").await;

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), 1);
        assert_eq!(prefixes[0], "Hi");
    }

    #[tokio::test]
    async fn test_mark_first_input_backspace_corrects_pending_line() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Type "helo", backspace (DEL), "lo" → should produce "hello"
        for ch in ["h", "e", "l", "o"] {
            state_mgr.mark_first_input("inst-1", ch).await;
        }
        state_mgr.mark_first_input("inst-1", "\x7f").await; // DEL
        state_mgr.mark_first_input("inst-1", "l").await;
        state_mgr.mark_first_input("inst-1", "o").await;
        state_mgr.mark_first_input("inst-1", "\r").await;

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), 1);
        assert_eq!(prefixes[0], "hello");
    }

    #[tokio::test]
    async fn test_mark_first_input_backspace_bs_char() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Same test but with BS (0x08) instead of DEL (0x7f)
        for ch in ["a", "b", "c"] {
            state_mgr.mark_first_input("inst-1", ch).await;
        }
        state_mgr.mark_first_input("inst-1", "\x08").await; // BS
        state_mgr.mark_first_input("inst-1", "\x08").await; // BS
        state_mgr.mark_first_input("inst-1", "x").await;
        state_mgr.mark_first_input("inst-1", "y").await;
        state_mgr.mark_first_input("inst-1", "\r").await;

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), 1);
        assert_eq!(prefixes[0], "axy");
    }

    #[tokio::test]
    async fn test_mark_first_input_caps_content_prefixes() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Push more than the cap (each with newline to force flush)
        for i in 0..25 {
            state_mgr
                .mark_first_input("inst-1", &format!("message {}\r", i))
                .await;
        }

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), super::FIRST_INPUT_PREFIX_CAP);
    }

    #[tokio::test]
    async fn test_mark_first_input_skips_empty_content() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // First call with whitespace-only: creates entry but no prefix
        assert!(state_mgr.mark_first_input("inst-1", "   \r").await);

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert!(prefixes.is_empty());

        // Timestamp should still be set
        assert!(state_mgr.get_first_input_at("inst-1").await.is_some());
    }

    #[tokio::test]
    async fn test_get_discovery_content_prefixes_no_entry() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        let prefixes = state_mgr
            .get_discovery_content_prefixes("nonexistent")
            .await;
        assert!(prefixes.is_empty());
    }

    #[tokio::test]
    async fn test_get_first_input_at_lifecycle() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Before any input
        assert!(state_mgr.get_first_input_at("inst-1").await.is_none());

        // After marking first input (keystroke without newline still creates timestamp)
        state_mgr.mark_first_input("inst-1", "h").await;
        let timestamp = state_mgr.get_first_input_at("inst-1").await;
        assert!(timestamp.is_some());

        // Timestamp should be recent
        let now = Utc::now();
        let diff = now - timestamp.unwrap();
        assert!(diff.num_seconds() < 2);
    }

    #[tokio::test]
    async fn test_first_input_at_cleanup_on_unregister() {
        let broadcast_tx = create_state_broadcast();
        let state_mgr = GlobalStateManager::new(broadcast_tx);

        // Mark first input
        state_mgr.mark_first_input("inst-1", "hello\r").await;
        assert!(state_mgr.get_first_input_at("inst-1").await.is_some());

        // Unregister cleans up first_input_at
        state_mgr.unregister_instance("inst-1").await;
        assert!(state_mgr.get_first_input_at("inst-1").await.is_none());
    }

    #[test]
    fn test_timestamp_narrowing_logic() {
        use chrono::TimeZone;

        let t1_created = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 1).unwrap();
        let t4_first_input = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 6).unwrap();
        let t3_other_session = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 4).unwrap();
        let t5_own_session = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 7).unwrap();

        // With created_at, BOTH sessions match (too broad)
        assert!(t3_other_session >= t1_created);
        assert!(t5_own_session >= t1_created);

        // With first_input_at, only OWN session matches (correct)
        assert!(t3_other_session < t4_first_input);
        assert!(t5_own_session >= t4_first_input);
    }

    #[tokio::test]
    async fn test_state_broadcast_channel_basic() {
        let (tx, mut rx) = broadcast::channel::<(String, ClaudeState, bool)>(16);

        tx.send(("inst-1".to_string(), ClaudeState::Thinking, false))
            .unwrap();

        let (id, state, stale) = rx.recv().await.unwrap();
        assert_eq!(id, "inst-1");
        assert!(matches!(state, ClaudeState::Thinking));
        assert!(!stale);
    }

    #[tokio::test]
    async fn test_state_broadcast_channel_multiple_receivers() {
        let (tx, mut rx1) = broadcast::channel::<(String, ClaudeState, bool)>(16);
        let mut rx2 = tx.subscribe();

        tx.send(("inst-1".to_string(), ClaudeState::Responding, true))
            .unwrap();

        let (id1, _, stale1) = rx1.recv().await.unwrap();
        let (id2, _, stale2) = rx2.recv().await.unwrap();

        assert_eq!(id1, "inst-1");
        assert_eq!(id2, "inst-1");
        assert!(stale1);
        assert!(stale2);
    }

    #[tokio::test]
    async fn test_state_broadcast_channel_lag_detection() {
        // Create a small channel that will lag
        let (tx, mut rx) = broadcast::channel::<(String, ClaudeState, bool)>(2);

        // Send more messages than the buffer can hold
        for i in 0..5 {
            let _ = tx.send((format!("inst-{}", i), ClaudeState::Idle, false));
        }

        // First recv should report lag
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(n)) => {
                assert!(n > 0, "Should report lagged messages");
            }
            Ok(_) => {
                // Might get the last message if timing is right
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_mpsc_channel_backpressure() {
        // Test that mpsc channel blocks sender when full
        let (tx, mut rx) = mpsc::channel::<ServerMessage>(2);

        // Fill the channel
        tx.send(ServerMessage::Output {
            instance_id: "inst-1".to_string(),
            data: "1".to_string(),
            cursor: None,
        })
        .await
        .unwrap();
        tx.send(ServerMessage::Output {
            instance_id: "inst-1".to_string(),
            data: "2".to_string(),
            cursor: None,
        })
        .await
        .unwrap();

        // Drain one to make room
        let msg = rx.recv().await.unwrap();
        match msg {
            ServerMessage::Output { data, .. } => assert_eq!(data, "1"),
            _ => panic!("Expected Output"),
        }

        // Now we can send again
        tx.send(ServerMessage::Output {
            instance_id: "inst-1".to_string(),
            data: "3".to_string(),
            cursor: None,
        })
        .await
        .unwrap();
    }

    // =========================================================================
    // Session claiming tests
    // =========================================================================

    #[tokio::test]
    async fn test_try_claim_session_unclaimed() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        assert!(state_mgr.try_claim_session("sess-1", "inst-1").await);
    }

    #[tokio::test]
    async fn test_try_claim_session_already_owned_by_self() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        assert!(state_mgr.try_claim_session("sess-1", "inst-1").await);
        // Same instance re-claiming → true
        assert!(state_mgr.try_claim_session("sess-1", "inst-1").await);
    }

    #[tokio::test]
    async fn test_try_claim_session_owned_by_other() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        assert!(state_mgr.try_claim_session("sess-1", "inst-1").await);
        // Different instance → false
        assert!(!state_mgr.try_claim_session("sess-1", "inst-2").await);
    }

    #[tokio::test]
    async fn test_release_session() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        state_mgr.try_claim_session("sess-1", "inst-1").await;
        state_mgr.try_claim_session("sess-2", "inst-1").await;

        state_mgr.release_session("inst-1").await;

        // Both sessions should be claimable by another instance now
        assert!(state_mgr.try_claim_session("sess-1", "inst-2").await);
        assert!(state_mgr.try_claim_session("sess-2", "inst-2").await);
    }

    #[tokio::test]
    async fn test_get_claimed_sessions() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        state_mgr.try_claim_session("sess-1", "inst-1").await;
        state_mgr.try_claim_session("sess-2", "inst-2").await;

        let claimed = state_mgr.get_claimed_sessions().await;
        assert!(claimed.contains("sess-1"));
        assert!(claimed.contains("sess-2"));
        assert_eq!(claimed.len(), 2);
    }

    #[tokio::test]
    async fn test_first_input_gate_prevents_cross_claim() {
        // Simulates the multi-instance race:
        // Instance A (no input) must not claim Instance B's session.
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        // Instance B marks first input
        assert!(state_mgr.mark_first_input("inst-b", "fix the bug").await);

        // Instance A has no first_input_at — should be gated from discovery
        assert!(state_mgr.get_first_input_at("inst-a").await.is_none());

        // Instance B has first_input_at — discovery proceeds
        assert!(state_mgr.get_first_input_at("inst-b").await.is_some());

        // B claims its session
        assert!(state_mgr.try_claim_session("sess-b", "inst-b").await);

        // Even if A were to try (bypassing the gate), it can't steal
        let claimed = state_mgr.get_claimed_sessions().await;
        assert!(claimed.contains("sess-b"));
        assert!(!state_mgr.try_claim_session("sess-b", "inst-a").await);
    }

    #[tokio::test]
    async fn test_two_instances_sequential_input_correct_claims() {
        // A gets input first, claims its session.
        // B gets input second, sees A's claim, claims its own session.
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        // A marks first input and claims session-a
        state_mgr.mark_first_input("inst-a", "hello").await;
        assert!(state_mgr.try_claim_session("sess-a", "inst-a").await);

        // B marks first input
        state_mgr.mark_first_input("inst-b", "world").await;

        // B sees sess-a is claimed
        let claimed = state_mgr.get_claimed_sessions().await;
        assert!(claimed.contains("sess-a"));

        // B claims sess-b (unclaimed)
        assert!(state_mgr.try_claim_session("sess-b", "inst-b").await);

        // Both sessions claimed by correct instances
        assert!(!state_mgr.try_claim_session("sess-a", "inst-b").await);
        assert!(!state_mgr.try_claim_session("sess-b", "inst-a").await);
    }

    // =========================================================================
    // Attribution queue tests
    // =========================================================================

    #[tokio::test]
    async fn test_push_and_consume_attribution() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        state_mgr
            .push_pending_attribution("inst-1", "u-1".into(), "Alice".into(), "fix the bug", None)
            .await;

        let attr = state_mgr
            .consume_pending_attribution("inst-1", "fix the bug")
            .await
            .unwrap();
        assert_eq!(attr.user_id, "u-1");
        assert_eq!(attr.display_name, "Alice");
    }

    #[tokio::test]
    async fn test_consume_attribution_no_match() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        state_mgr
            .push_pending_attribution("inst-1", "u-1".into(), "Alice".into(), "fix the bug", None)
            .await;

        let result = state_mgr
            .consume_pending_attribution("inst-1", "add new feature")
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_consume_attribution_fifo() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        state_mgr
            .push_pending_attribution("inst-1", "u-1".into(), "Alice".into(), "message one", None)
            .await;
        state_mgr
            .push_pending_attribution("inst-1", "u-2".into(), "Bob".into(), "message two", None)
            .await;

        // Consume first match
        let attr = state_mgr
            .consume_pending_attribution("inst-1", "message one")
            .await
            .unwrap();
        assert_eq!(attr.user_id, "u-1");

        // First is consumed, second remains
        let attr = state_mgr
            .consume_pending_attribution("inst-1", "message two")
            .await
            .unwrap();
        assert_eq!(attr.user_id, "u-2");
    }

    #[tokio::test]
    async fn test_push_attribution_empty_content_skipped() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        state_mgr
            .push_pending_attribution("inst-1", "u-1".into(), "Alice".into(), "   ", None)
            .await;

        // Nothing should be queued (empty after trim)
        let result = state_mgr
            .consume_pending_attribution("inst-1", "anything")
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_attribution_with_task_id() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        state_mgr
            .push_pending_attribution(
                "inst-1",
                "u-1".into(),
                "Alice".into(),
                "task content",
                Some(42),
            )
            .await;

        let attr = state_mgr
            .consume_pending_attribution("inst-1", "task content")
            .await
            .unwrap();
        assert_eq!(attr.task_id, Some(42));
    }

    #[tokio::test]
    async fn test_attribution_queue_cap() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        // Push more than the 50-entry cap
        for i in 0..60 {
            state_mgr
                .push_pending_attribution(
                    "inst-1",
                    "u-1".into(),
                    "Alice".into(),
                    &format!("message {}", i),
                    None,
                )
                .await;
        }

        // Oldest entries should be evicted; message 0 through 9 should be gone
        let result = state_mgr
            .consume_pending_attribution("inst-1", "message 0")
            .await;
        assert!(result.is_none());

        // Latest entries should still be present
        let result = state_mgr
            .consume_pending_attribution("inst-1", "message 59")
            .await;
        assert!(result.is_some());
    }

    // =========================================================================
    // Terminal lock tests
    // =========================================================================

    fn make_ws_user(user_id: &str, display_name: &str) -> WsUser {
        WsUser {
            user_id: user_id.to_string(),
            display_name: display_name.to_string(),
        }
    }

    #[tokio::test]
    async fn test_acquire_terminal_lock_unclaimed() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = make_ws_user("u-1", "Alice");

        assert!(
            state_mgr
                .try_acquire_terminal_lock("inst-1", "conn-1", &user)
                .await
        );

        let lock = state_mgr.get_terminal_lock("inst-1").await.unwrap();
        assert_eq!(lock.holder_user_id, "u-1");
    }

    #[tokio::test]
    async fn test_acquire_terminal_lock_same_user() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = make_ws_user("u-1", "Alice");

        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &user)
            .await;
        // Same user, different connection (new tab) → allowed
        assert!(
            state_mgr
                .try_acquire_terminal_lock("inst-1", "conn-2", &user)
                .await
        );

        let lock = state_mgr.get_terminal_lock("inst-1").await.unwrap();
        assert_eq!(lock.holder_connection_id, "conn-2");
    }

    #[tokio::test]
    async fn test_acquire_terminal_lock_different_user_denied() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let alice = make_ws_user("u-1", "Alice");
        let bob = make_ws_user("u-2", "Bob");

        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &alice)
            .await;
        // Different user while lock is active → denied
        assert!(
            !state_mgr
                .try_acquire_terminal_lock("inst-1", "conn-2", &bob)
                .await
        );
    }

    #[tokio::test]
    async fn test_release_terminal_lock() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = make_ws_user("u-1", "Alice");

        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &user)
            .await;
        let released = state_mgr.release_terminal_lock("inst-1", "conn-1").await;
        assert!(released);

        assert!(state_mgr.get_terminal_lock("inst-1").await.is_none());
    }

    #[tokio::test]
    async fn test_release_terminal_lock_wrong_connection() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = make_ws_user("u-1", "Alice");

        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &user)
            .await;
        // Wrong connection_id → not released
        let released = state_mgr
            .release_terminal_lock("inst-1", "conn-other")
            .await;
        assert!(!released);

        assert!(state_mgr.get_terminal_lock("inst-1").await.is_some());
    }

    // =========================================================================
    // Presence tracking tests
    // =========================================================================

    #[tokio::test]
    async fn test_add_presence() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = WsUser {
            user_id: "u-1".into(),
            display_name: "Alice".into(),
        };

        let users = state_mgr.add_presence("inst-1", "conn-1", &user).await;
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].user_id, "u-1");
    }

    #[tokio::test]
    async fn test_presence_deduplication() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = WsUser {
            user_id: "u-1".into(),
            display_name: "Alice".into(),
        };

        // Same user, two connections (two tabs)
        state_mgr.add_presence("inst-1", "conn-1", &user).await;
        let users = state_mgr.add_presence("inst-1", "conn-2", &user).await;
        // Should be deduplicated to 1
        assert_eq!(users.len(), 1);
    }

    #[tokio::test]
    async fn test_remove_presence() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let alice = WsUser {
            user_id: "u-1".into(),
            display_name: "Alice".into(),
        };
        let bob = WsUser {
            user_id: "u-2".into(),
            display_name: "Bob".into(),
        };

        state_mgr.add_presence("inst-1", "conn-1", &alice).await;
        state_mgr.add_presence("inst-1", "conn-2", &bob).await;

        let remaining = state_mgr
            .remove_presence_from_instance("inst-1", "conn-1")
            .await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].user_id, "u-2");
    }

    #[tokio::test]
    async fn test_remove_presence_all() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = WsUser {
            user_id: "u-1".into(),
            display_name: "Alice".into(),
        };

        state_mgr.add_presence("inst-1", "conn-1", &user).await;
        state_mgr.add_presence("inst-2", "conn-1", &user).await;

        let updates = state_mgr.remove_presence_all("conn-1").await;
        assert_eq!(updates.len(), 2);
        // Both instances should now have empty presence
        for (_, users) in &updates {
            assert!(users.is_empty());
        }
    }

    // =========================================================================
    // touch_terminal_lock tests
    // =========================================================================

    #[tokio::test]
    async fn test_touch_terminal_lock_updates_activity() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = make_ws_user("u-1", "Alice");

        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &user)
            .await;

        let before = state_mgr
            .get_terminal_lock("inst-1")
            .await
            .unwrap()
            .last_activity;

        // Sleep briefly so the timestamp differs
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        state_mgr.touch_terminal_lock("inst-1", "conn-1").await;

        let after = state_mgr
            .get_terminal_lock("inst-1")
            .await
            .unwrap()
            .last_activity;

        assert!(after >= before);
    }

    #[tokio::test]
    async fn test_touch_terminal_lock_wrong_connection_ignored() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let user = make_ws_user("u-1", "Alice");

        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &user)
            .await;

        let before = state_mgr
            .get_terminal_lock("inst-1")
            .await
            .unwrap()
            .last_activity;

        // Touch with wrong connection_id — should not update
        state_mgr.touch_terminal_lock("inst-1", "conn-other").await;

        let after = state_mgr
            .get_terminal_lock("inst-1")
            .await
            .unwrap()
            .last_activity;

        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn test_touch_terminal_lock_no_lock_noop() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        // Touch on non-existent lock — should not panic
        state_mgr.touch_terminal_lock("inst-1", "conn-1").await;
        assert!(state_mgr.get_terminal_lock("inst-1").await.is_none());
    }

    // =========================================================================
    // reconcile_terminal_lock_with_presence tests
    // =========================================================================

    #[tokio::test]
    async fn test_reconcile_clears_disconnected_holder() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let alice = make_ws_user("u-1", "Alice");

        // Alice joins and gets the lock
        state_mgr.add_presence("inst-1", "conn-1", &alice).await;
        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &alice)
            .await;

        // Alice disconnects
        state_mgr
            .remove_presence_from_instance("inst-1", "conn-1")
            .await;

        // Reconcile should clear the lock
        let changed = state_mgr
            .reconcile_terminal_lock_with_presence("inst-1")
            .await;
        assert!(changed);
        assert!(state_mgr.get_terminal_lock("inst-1").await.is_none());
    }

    #[tokio::test]
    async fn test_reconcile_auto_grants_sole_user() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let alice = make_ws_user("u-1", "Alice");

        // Alice joins but no lock is held
        state_mgr.add_presence("inst-1", "conn-1", &alice).await;

        // Reconcile should auto-grant to sole user
        let changed = state_mgr
            .reconcile_terminal_lock_with_presence("inst-1")
            .await;
        assert!(changed);

        let lock = state_mgr.get_terminal_lock("inst-1").await.unwrap();
        assert_eq!(lock.holder_user_id, "u-1");
    }

    #[tokio::test]
    async fn test_reconcile_no_change_when_holder_present() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let alice = make_ws_user("u-1", "Alice");
        let bob = make_ws_user("u-2", "Bob");

        // Alice and Bob both present, Alice holds lock
        state_mgr.add_presence("inst-1", "conn-1", &alice).await;
        state_mgr.add_presence("inst-1", "conn-2", &bob).await;
        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &alice)
            .await;

        // No change needed — holder is still present
        let changed = state_mgr
            .reconcile_terminal_lock_with_presence("inst-1")
            .await;
        assert!(!changed);
    }

    #[tokio::test]
    async fn test_reconcile_no_auto_grant_multiple_users() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let alice = make_ws_user("u-1", "Alice");
        let bob = make_ws_user("u-2", "Bob");

        // Two users present, no lock — should NOT auto-grant
        state_mgr.add_presence("inst-1", "conn-1", &alice).await;
        state_mgr.add_presence("inst-1", "conn-2", &bob).await;

        let changed = state_mgr
            .reconcile_terminal_lock_with_presence("inst-1")
            .await;
        assert!(!changed);
        assert!(state_mgr.get_terminal_lock("inst-1").await.is_none());
    }

    #[tokio::test]
    async fn test_reconcile_empty_instance_no_change() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        // No presence at all — no change
        let changed = state_mgr
            .reconcile_terminal_lock_with_presence("inst-1")
            .await;
        assert!(!changed);
    }

    // =========================================================================
    // Lifecycle broadcast tests
    // =========================================================================

    #[tokio::test]
    async fn test_subscribe_lifecycle_receives_events() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let mut rx = state_mgr.subscribe_lifecycle();

        state_mgr.broadcast_lifecycle(ServerMessage::InstanceStopped {
            instance_id: "inst-1".to_string(),
        });

        let msg = rx.recv().await.unwrap();
        match msg {
            ServerMessage::InstanceStopped { instance_id } => {
                assert_eq!(instance_id, "inst-1");
            }
            _ => panic!("Expected InstanceStopped"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_state_broadcast() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let mut rx = state_mgr.subscribe();

        // Manually broadcast via the underlying channel
        // (normally done by state manager forwarding)
        let _ = state_mgr
            .broadcast_tx
            .send(("inst-1".to_string(), ClaudeState::Idle, false));

        let (id, state, stale) = rx.recv().await.unwrap();
        assert_eq!(id, "inst-1");
        assert!(matches!(state, ClaudeState::Idle));
        assert!(!stale);
    }

    // =========================================================================
    // Keystroke → flush → prefix flow tests
    // =========================================================================

    #[tokio::test]
    async fn test_keystroke_then_enter_produces_prefix() {
        let gsm = GlobalStateManager::new(create_state_broadcast());
        for ch in "hello".chars() {
            gsm.mark_first_input("i1", &ch.to_string()).await;
        }
        assert!(gsm.get_discovery_content_prefixes("i1").await.is_empty());
        gsm.mark_first_input("i1", "\r").await;
        assert_eq!(
            gsm.get_discovery_content_prefixes("i1").await,
            vec!["hello"]
        );
    }

    #[tokio::test]
    async fn test_control_chars_filtered_from_pending_line() {
        let gsm = GlobalStateManager::new(create_state_broadcast());
        gsm.mark_first_input("i1", "\x1b").await; // escape
        gsm.mark_first_input("i1", "\x1b[A").await; // arrow up
        gsm.mark_first_input("i1", "h").await;
        gsm.mark_first_input("i1", "i").await;
        gsm.mark_first_input("i1", "\r").await;
        assert_eq!(gsm.get_discovery_content_prefixes("i1").await, vec!["hi"]);
    }

    #[tokio::test]
    async fn test_full_message_with_newline_stores_directly() {
        let gsm = GlobalStateManager::new(create_state_broadcast());
        gsm.mark_first_input("i1", "hello world\r").await;
        assert_eq!(
            gsm.get_discovery_content_prefixes("i1").await,
            vec!["hello world"]
        );
    }

    // =========================================================================
    // Unregister instance cleanup tests
    // =========================================================================

    // =========================================================================
    // Unified handle_input pipeline (TUI + web parity)
    // =========================================================================

    /// Simulate TUI-style per-keystroke input through handle_input
    /// (requires a registered instance with InstanceHandle).
    async fn handle_input_keystrokes(
        state_mgr: &GlobalStateManager,
        instance_id: &str,
        chars: &str,
    ) {
        for ch in chars.chars() {
            let ctx = InputContext {
                instance_id: instance_id.to_string(),
                data: ch.to_string(),
                connection_id: "test-conn".to_string(),
                user: None,
                task_id: None,
            };
            let _ = state_mgr.handle_input(ctx, None).await;
        }
        let ctx = InputContext {
            instance_id: instance_id.to_string(),
            data: "\r".to_string(),
            connection_id: "test-conn".to_string(),
            user: None,
            task_id: None,
        };
        let _ = state_mgr.handle_input(ctx, None).await;
    }

    /// Simulate web-style composed input through handle_input
    /// (text first, then \r).
    async fn handle_input_composed(
        state_mgr: &GlobalStateManager,
        instance_id: &str,
        text: &str,
        user: Option<InputUser>,
        task_id: Option<i64>,
    ) {
        let ctx = InputContext {
            instance_id: instance_id.to_string(),
            data: text.to_string(),
            connection_id: "test-conn".to_string(),
            user,
            task_id,
        };
        let _ = state_mgr.handle_input(ctx, None).await;
        let ctx = InputContext {
            instance_id: instance_id.to_string(),
            data: "\r".to_string(),
            connection_id: "test-conn".to_string(),
            user: None,
            task_id: None,
        };
        let _ = state_mgr.handle_input(ctx, None).await;
    }

    #[tokio::test]
    async fn test_handle_input_tui_keystrokes_produce_prefix() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle).await;

        handle_input_keystrokes(&state_mgr, "inst-1", "hello").await;

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), 1);
        assert_eq!(prefixes[0], "hello");
    }

    #[tokio::test]
    async fn test_handle_input_web_composed_produce_prefix() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle).await;

        handle_input_composed(&state_mgr, "inst-1", "hello", None, None).await;

        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes.len(), 1);
        assert_eq!(prefixes[0], "hello");
    }

    #[tokio::test]
    async fn test_handle_input_tui_and_web_produce_same_prefixes() {
        // TUI path (per-keystroke through handle_input)
        let state_mgr_tui = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr_tui.insert_test_tracker("inst-1", handle).await;

        handle_input_keystrokes(&state_mgr_tui, "inst-1", "fix the login bug").await;

        let tui_prefixes = state_mgr_tui.get_discovery_content_prefixes("inst-1").await;

        // Web path (composed through handle_input)
        let state_mgr_web = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr_web.insert_test_tracker("inst-1", handle).await;

        handle_input_composed(&state_mgr_web, "inst-1", "fix the login bug", None, None).await;

        let web_prefixes = state_mgr_web.get_discovery_content_prefixes("inst-1").await;

        assert_eq!(tui_prefixes, web_prefixes);
        assert_eq!(tui_prefixes[0], "fix the login bug");
    }

    #[tokio::test]
    async fn test_handle_input_stops_recording_after_session_claimed() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle).await;

        // First message goes through
        handle_input_composed(&state_mgr, "inst-1", "hello", None, None).await;

        let prefixes_before = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes_before.len(), 1);

        // Simulate session being claimed (set session_id on the handle)
        let handle = state_mgr.get_handle("inst-1").await.unwrap();
        handle
            .set_session_id("sess-123".to_string())
            .await
            .expect("set_session_id should succeed in test actor");

        // Second message should NOT be recorded (session already claimed)
        handle_input_composed(&state_mgr, "inst-1", "second message", None, None).await;

        let prefixes_after = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(
            prefixes_after.len(),
            1,
            "should not record after session claimed"
        );
    }

    #[tokio::test]
    async fn test_handle_input_with_user_records_attribution() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle).await;

        let user = InputUser {
            user_id: "u-1".to_string(),
            display_name: "Alice".to_string(),
        };
        handle_input_composed(&state_mgr, "inst-1", "fix the bug", Some(user), None).await;

        // Should have a pending attribution
        let attr = state_mgr
            .consume_pending_attribution("inst-1", "fix the bug")
            .await;
        assert!(
            attr.is_some(),
            "attribution should be recorded for web user"
        );
        let attr = attr.unwrap();
        assert_eq!(attr.user_id, "u-1");
        assert_eq!(attr.display_name, "Alice");
    }

    #[tokio::test]
    async fn test_handle_input_without_user_skips_attribution() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle).await;

        // TUI path: no user
        handle_input_composed(&state_mgr, "inst-1", "fix the bug", None, None).await;

        // Should NOT have a pending attribution
        let attr = state_mgr
            .consume_pending_attribution("inst-1", "fix the bug")
            .await;
        assert!(
            attr.is_none(),
            "attribution should NOT be recorded for TUI (no user)"
        );
    }

    #[tokio::test]
    async fn test_handle_input_touches_terminal_lock() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle).await;

        // Acquire a terminal lock for "test-conn"
        let user = make_ws_user("u-1", "Alice");
        state_mgr
            .try_acquire_terminal_lock("inst-1", "test-conn", &user)
            .await;

        let before = state_mgr
            .get_terminal_lock("inst-1")
            .await
            .unwrap()
            .last_activity;

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // handle_input uses connection_id "test-conn" which matches the lock holder
        let ctx = InputContext {
            instance_id: "inst-1".to_string(),
            data: "x".to_string(),
            connection_id: "test-conn".to_string(),
            user: None,
            task_id: None,
        };
        let _ = state_mgr.handle_input(ctx, None).await;

        let after = state_mgr
            .get_terminal_lock("inst-1")
            .await
            .unwrap()
            .last_activity;

        assert!(after > before, "terminal lock should be refreshed by input");
    }

    // =========================================================================
    // Hypothesis tests: H4, H5, H6
    // =========================================================================

    // H4: get_session_id() gates mark_first_input — premature session_id
    //     prevents discovery content tracking.

    #[tokio::test]
    async fn h4_session_id_set_before_input_blocks_mark_first_input() {
        // When session_id is already set before any input, handle_input
        // skips mark_first_input entirely (line 629-631). This means
        // first_input_data is never populated, and the watcher can never
        // discover the session via content matching.
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr
            .insert_test_tracker("inst-1", handle.clone())
            .await;

        // Set session_id BEFORE any input
        handle
            .set_session_id("sess-premature".to_string())
            .await
            .unwrap();

        // Now send input through the pipeline
        handle_input_composed(&state_mgr, "inst-1", "hello", None, None).await;

        // first_input_data should NOT be populated (session_id gate blocked it)
        assert!(
            state_mgr.get_first_input_at("inst-1").await.is_none(),
            "mark_first_input should be skipped when session_id is already set"
        );
        assert!(
            state_mgr
                .get_discovery_content_prefixes("inst-1")
                .await
                .is_empty(),
            "no content prefixes when session_id was set early"
        );
    }

    #[tokio::test]
    async fn h4_mark_first_input_runs_when_session_id_is_none() {
        // Normal flow: session_id is None, so mark_first_input runs.
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle).await;

        handle_input_composed(&state_mgr, "inst-1", "hello", None, None).await;

        assert!(
            state_mgr.get_first_input_at("inst-1").await.is_some(),
            "mark_first_input should run when session_id is None"
        );
        let prefixes = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes, vec!["hello"]);
    }

    #[tokio::test]
    async fn h4_watcher_gates_on_first_input_data() {
        // The conversation watcher checks first_input_data before
        // attempting session discovery. Without it, discovery is blocked.
        let first_input_data: Arc<RwLock<HashMap<String, FirstInputData>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // No entry for "inst-1" → watcher would skip discovery
        let has_first_input = first_input_data
            .read()
            .await
            .get("inst-1")
            .map(|d| d.timestamp)
            .is_some();
        assert!(
            !has_first_input,
            "no first_input_data → watcher should not attempt discovery"
        );

        // Add an entry → watcher would proceed
        first_input_data.write().await.insert(
            "inst-1".to_string(),
            FirstInputData {
                timestamp: Utc::now(),
                content_prefixes: vec!["hello".to_string()],
                pending_line: String::new(),
            },
        );
        let has_first_input = first_input_data
            .read()
            .await
            .get("inst-1")
            .map(|d| d.timestamp)
            .is_some();
        assert!(
            has_first_input,
            "with first_input_data → watcher proceeds to discovery"
        );
    }

    // H5: Stale claimed_sessions entry from prior instance

    #[tokio::test]
    async fn h5_stale_claim_blocks_new_instance() {
        // If an instance crashes without unregister_instance, its
        // claimed session remains and blocks a new instance.
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        // "inst-crashed" claims sess-1
        assert!(state_mgr.try_claim_session("sess-1", "inst-crashed").await);

        // New instance tries to claim the same session → blocked
        assert!(
            !state_mgr.try_claim_session("sess-1", "inst-new").await,
            "stale claim should block new instance"
        );
    }

    #[tokio::test]
    async fn h5_unregister_releases_claimed_session() {
        // unregister_instance properly cleans up claims, allowing
        // re-use by a new instance.
        let state_mgr = GlobalStateManager::new(create_state_broadcast());

        // Register + claim
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr
            .register_instance(
                "inst-old".to_string(),
                handle,
                "/tmp".to_string(),
                Utc::now(),
                true,
            )
            .await;
        assert!(state_mgr.try_claim_session("sess-1", "inst-old").await);

        // Unregister releases the session
        state_mgr.unregister_instance("inst-old").await;

        // New instance can now claim it
        assert!(
            state_mgr.try_claim_session("sess-1", "inst-new").await,
            "claim should succeed after unregister releases it"
        );
    }

    // H6: first_input_data keyed by wrong instance_id

    #[tokio::test]
    async fn h6_first_input_data_keyed_correctly_across_instances() {
        // Verify that input to inst-1 does NOT leak into inst-2's data.
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let (handle1, _) = InstanceHandle::spawn_test(24, 80, 4096);
        let (handle2, _) = InstanceHandle::spawn_test(24, 80, 4096);
        state_mgr.insert_test_tracker("inst-1", handle1).await;
        state_mgr.insert_test_tracker("inst-2", handle2).await;

        // Send input only to inst-1
        handle_input_composed(&state_mgr, "inst-1", "hello", None, None).await;

        // inst-1 should have data
        assert!(
            state_mgr.get_first_input_at("inst-1").await.is_some(),
            "inst-1 should have first_input_data"
        );
        let prefixes_1 = state_mgr.get_discovery_content_prefixes("inst-1").await;
        assert_eq!(prefixes_1, vec!["hello"]);

        // inst-2 should have nothing
        assert!(
            state_mgr.get_first_input_at("inst-2").await.is_none(),
            "inst-2 should NOT have first_input_data"
        );
        assert!(
            state_mgr
                .get_discovery_content_prefixes("inst-2")
                .await
                .is_empty(),
            "inst-2 should have no content prefixes"
        );
    }

    // =========================================================================
    // Cleanup tests
    // =========================================================================

    // =========================================================================
    // ClaudeDriver integration tests
    // =========================================================================

    use crate::claude_driver::ClaudeDriver;
    use crate::process_driver::DriverSignal;

    #[tokio::test]
    async fn int_handle_input_through_real_actor() {
        // Hypothesis: A1 final verification — keystroke accumulation through full pipeline.
        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), None, None, None);

        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        state_mgr
            .insert_test_tracker("test-instance", handle.clone())
            .await;

        // Send keystrokes: "hi\r"
        for ch in ['h', 'i'] {
            let ctx = InputContext {
                instance_id: "test-instance".to_string(),
                data: ch.to_string(),
                connection_id: "test-conn".to_string(),
                user: None,
                task_id: None,
            };
            let _ = state_mgr.handle_input(ctx, None).await;
        }
        let ctx = InputContext {
            instance_id: "test-instance".to_string(),
            data: "\r".to_string(),
            connection_id: "test-conn".to_string(),
            user: None,
            task_id: None,
        };
        let _ = state_mgr.handle_input(ctx, None).await;

        assert!(
            state_mgr
                .get_first_input_at("test-instance")
                .await
                .is_some(),
            "first_input_at should be set"
        );
        let prefixes = state_mgr
            .get_discovery_content_prefixes("test-instance")
            .await;
        assert_eq!(prefixes, vec!["hi"]);

        // Session ID should still be None (no signal sent)
        assert_eq!(handle.get_session_id().await, None);

        drop(handle);
    }

    #[tokio::test]
    async fn int_premature_session_persists_across_inputs() {
        // Hypothesis: A2 depth — session_id persists, mark_first_input permanently blocked.
        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), Some(signal_rx), None, None);

        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        state_mgr
            .insert_test_tracker("test-instance", handle.clone())
            .await;

        // Set session_id before any input
        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-1".into()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        assert_eq!(handle.get_session_id().await, Some("sess-1".into()));

        // Send several keystrokes
        for ch in ['a', 'b', 'c'] {
            let ctx = InputContext {
                instance_id: "test-instance".to_string(),
                data: ch.to_string(),
                connection_id: "test-conn".to_string(),
                user: None,
                task_id: None,
            };
            let _ = state_mgr.handle_input(ctx, None).await;
        }

        // Session ID should persist
        assert_eq!(
            handle.get_session_id().await,
            Some("sess-1".into()),
            "session_id should not be cleared by input"
        );

        // mark_first_input should still be blocked
        assert!(
            state_mgr
                .get_first_input_at("test-instance")
                .await
                .is_none(),
            "mark_first_input should remain blocked"
        );

        drop(handle);
    }

    #[tokio::test]
    async fn test_unregister_cleans_up_all_state() {
        let state_mgr = GlobalStateManager::new(create_state_broadcast());
        let alice = make_ws_user("u-1", "Alice");

        // Set up state for an instance
        state_mgr.try_claim_session("sess-1", "inst-1").await;
        state_mgr.mark_first_input("inst-1", "hello").await;
        state_mgr
            .push_pending_attribution("inst-1", "u-1".into(), "Alice".into(), "hello", None)
            .await;
        state_mgr
            .try_acquire_terminal_lock("inst-1", "conn-1", &alice)
            .await;

        // Unregister should clean up everything
        state_mgr.unregister_instance("inst-1").await;

        assert!(state_mgr.get_first_input_at("inst-1").await.is_none());
        assert!(state_mgr.get_terminal_lock("inst-1").await.is_none());
        // Session should be reclaimable
        assert!(state_mgr.try_claim_session("sess-1", "inst-2").await);
        // Attribution queue should be gone
        assert!(
            state_mgr
                .consume_pending_attribution("inst-1", "hello")
                .await
                .is_none()
        );
    }
}
