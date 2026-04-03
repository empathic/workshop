use anyhow::Result;
use pty_manager::PtyOutput;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tracing::{debug, info, warn};
use uuid::Uuid;

use pty_manager::{PtyConfig, PtyHandle};

use crate::inference::ClaudeState;
use crate::instance_manager::InstanceKind;
use crate::process_driver::{DriverContext, DriverSignal, ProcessDriver};
use crate::repository::ConversationRepository;
use crate::virtual_terminal::{ClientType, VirtualTerminal, VtRecorder};
use crate::ws::ConversationEvent;
use crate::ws::{FirstInputData, PendingAttribution, StateBroadcast};

/// PTY output enriched with cursor position from the VirtualTerminal.
/// Produced by the VT processing task after feeding output to the parser.
#[derive(Debug, Clone)]
pub struct EnrichedOutput {
    pub data: Vec<u8>,
    pub cursor: (u16, u16),
    #[allow(dead_code)]
    pub timestamp: i64,
}

/// Commands that can be sent to an instance actor
#[derive(Debug)]
#[allow(dead_code)]
pub enum InstanceCommand {
    GetInfo {
        respond_to: oneshot::Sender<InstanceInfo>,
    },
    WriteInput {
        text: String,
        respond_to: oneshot::Sender<Result<usize>>,
    },
    Resize {
        rows: u16,
        cols: u16,
        respond_to: oneshot::Sender<Result<()>>,
    },
    SubscribeOutput {
        respond_to: oneshot::Sender<broadcast::Receiver<EnrichedOutput>>,
    },
    /// Get recent output with a byte limit
    /// Returns chunks from the end of the buffer up to max_bytes total.
    /// `client_rows` is the receiving terminal's height — used to size the
    /// scrollback-to-keyframe flush so no lines are lost or garbled.
    GetRecentOutput {
        max_bytes: usize,
        client_rows: u16,
        respond_to: oneshot::Sender<Vec<String>>,
    },
    SetSessionId {
        session_id: String,
        respond_to: oneshot::Sender<()>,
    },
    GetSessionId {
        respond_to: oneshot::Sender<Option<String>>,
    },
    GetPid {
        respond_to: oneshot::Sender<Option<u32>>,
    },
    GetConversationSnapshot {
        respond_to: oneshot::Sender<Vec<serde_json::Value>>,
    },
    SubscribeConversation {
        respond_to: oneshot::Sender<Option<broadcast::Receiver<ConversationEvent>>>,
    },
    SetCustomName {
        name: Option<String>,
        respond_to: oneshot::Sender<()>,
    },
    /// Update a client's viewport in the VirtualTerminal.
    /// Returns Some((rows, cols)) if effective dims changed.
    UpdateViewport {
        connection_id: String,
        rows: u16,
        cols: u16,
        client_type: ClientType,
        respond_to: oneshot::Sender<Option<(u16, u16)>>,
    },
    /// Set a client's terminal visibility.
    /// Returns Some((rows, cols)) if effective dims changed.
    SetClientActive {
        connection_id: String,
        active: bool,
        respond_to: oneshot::Sender<Option<(u16, u16)>>,
    },
    /// Remove a client from the VirtualTerminal.
    /// Returns Some((rows, cols)) if effective dims changed.
    RemoveClient {
        connection_id: String,
        respond_to: oneshot::Sender<Option<(u16, u16)>>,
    },
    Stop {
        respond_to: oneshot::Sender<Result<()>>,
    },
}

/// Information about a running instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub id: String,
    pub name: String,
    /// User-set display name (e.g. "Auth refactor"). Falls back to `name` if None.
    pub custom_name: Option<String>,
    pub command: String,
    pub kind: InstanceKind,
    pub working_dir: String,
    pub running: bool,
    pub created_at: String,
    /// The Claude conversation session ID (detected after instance starts)
    pub session_id: Option<String>,
    /// Current Claude state (for status indicator in sidebar)
    pub claude_state: Option<ClaudeState>,
}

/// Handle to communicate with an instance actor
#[derive(Clone)]
pub struct InstanceHandle {
    sender: mpsc::Sender<InstanceCommand>,
    info: Arc<RwLock<InstanceInfo>>,
}

#[allow(dead_code)]
impl InstanceHandle {
    pub async fn get_info(&self) -> InstanceInfo {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .sender
            .send(InstanceCommand::GetInfo { respond_to: tx })
            .await;
        match rx.await {
            Ok(info) => info,
            Err(_) => self.info.read().await.clone(),
        }
    }

    pub async fn write_input(&self, text: &str) -> Result<usize> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::WriteInput {
                text: text.to_string(),
                respond_to: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Instance actor is gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Instance actor didn't respond"))?
    }

    pub async fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::Resize {
                rows,
                cols,
                respond_to: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Instance actor is gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Instance actor didn't respond"))?
    }

    pub async fn subscribe_output(&self) -> Result<broadcast::Receiver<EnrichedOutput>> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::SubscribeOutput { respond_to: tx })
            .await
            .map_err(|_| anyhow::anyhow!("Instance actor is gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Instance actor didn't respond"))
    }

    /// Get recent output up to max_bytes total.
    /// `client_rows` is the receiving terminal's row count — used to size
    /// the scrollback flush so the client gets the full history.
    pub async fn get_recent_output(&self, max_bytes: usize, client_rows: u16) -> Vec<String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .sender
            .send(InstanceCommand::GetRecentOutput {
                max_bytes,
                client_rows,
                respond_to: tx,
            })
            .await;
        rx.await.unwrap_or_default()
    }

    pub async fn stop(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::Stop { respond_to: tx })
            .await
            .map_err(|_| anyhow::anyhow!("Instance actor is gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Instance actor didn't respond"))?
    }

    pub async fn set_session_id(&self, session_id: String) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::SetSessionId {
                session_id,
                respond_to: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Instance actor is gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Instance actor didn't respond"))?;
        Ok(())
    }

    pub async fn get_session_id(&self) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::GetSessionId { respond_to: tx })
            .await
            .ok()?;
        rx.await.ok().flatten()
    }

    pub async fn get_pid(&self) -> Option<u32> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::GetPid { respond_to: tx })
            .await
            .ok()?;
        rx.await.ok().flatten()
    }

    pub async fn set_custom_name(&self, name: Option<String>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::SetCustomName {
                name,
                respond_to: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Instance actor is gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Instance actor didn't respond"))?;
        Ok(())
    }

    pub async fn get_conversation_snapshot(&self) -> Vec<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .sender
            .send(InstanceCommand::GetConversationSnapshot { respond_to: tx })
            .await;
        rx.await.unwrap_or_default()
    }

    pub async fn subscribe_conversation(&self) -> Option<broadcast::Receiver<ConversationEvent>> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::SubscribeConversation { respond_to: tx })
            .await
            .ok()?;
        rx.await.ok().flatten()
    }

    /// Update a client's viewport in the VirtualTerminal.
    /// Returns Some((rows, cols)) if the effective dimensions changed.
    pub async fn update_viewport(
        &self,
        connection_id: &str,
        rows: u16,
        cols: u16,
        client_type: ClientType,
    ) -> Option<(u16, u16)> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::UpdateViewport {
                connection_id: connection_id.to_string(),
                rows,
                cols,
                client_type,
                respond_to: tx,
            })
            .await
            .ok()?;
        rx.await.ok().flatten()
    }

    /// Set a client's terminal visibility.
    /// Returns Some((rows, cols)) if the effective dimensions changed.
    pub async fn set_client_active(&self, connection_id: &str, active: bool) -> Option<(u16, u16)> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::SetClientActive {
                connection_id: connection_id.to_string(),
                active,
                respond_to: tx,
            })
            .await
            .ok()?;
        rx.await.ok().flatten()
    }

    /// Remove a client from the VirtualTerminal.
    /// Returns Some((rows, cols)) if the effective dimensions changed.
    pub async fn remove_client(&self, connection_id: &str) -> Option<(u16, u16)> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(InstanceCommand::RemoveClient {
                connection_id: connection_id.to_string(),
                respond_to: tx,
            })
            .await
            .ok()?;
        rx.await.ok().flatten()
    }

    /// Update viewport and resize PTY if effective dimensions changed.
    pub async fn update_viewport_and_resize(
        &self,
        connection_id: &str,
        rows: u16,
        cols: u16,
        client_type: ClientType,
    ) -> Result<()> {
        if let Some((eff_rows, eff_cols)) = self
            .update_viewport(connection_id, rows, cols, client_type)
            .await
        {
            self.resize(eff_rows, eff_cols).await?;
        }
        Ok(())
    }

    /// Set client active/inactive and resize PTY if effective dimensions changed.
    pub async fn set_active_and_resize(&self, connection_id: &str, active: bool) -> Result<()> {
        if let Some((eff_rows, eff_cols)) = self.set_client_active(connection_id, active).await {
            self.resize(eff_rows, eff_cols).await?;
        }
        Ok(())
    }

    /// Remove client and resize PTY if effective dimensions changed.
    pub async fn remove_client_and_resize(&self, connection_id: &str) -> Result<()> {
        if let Some((eff_rows, eff_cols)) = self.remove_client(connection_id).await {
            self.resize(eff_rows, eff_cols).await?;
        }
        Ok(())
    }

    pub fn id(&self) -> String {
        self.info.blocking_read().id.clone()
    }

    pub async fn id_async(&self) -> String {
        self.info.read().await.id.clone()
    }
}

/// Options for spawning a new instance actor
pub struct SpawnOptions {
    pub name: String,
    pub display_command: String,
    pub actual_command: String,
    pub args: Vec<String>,
    pub working_dir: String,
    pub kind: InstanceKind,
    /// Maximum output ring buffer size in bytes
    pub max_buffer_bytes: usize,
    /// Number of scrollback lines the server-side vt100 parser retains
    pub scrollback_lines: usize,
    /// Directory to write VT session recordings. None = recording disabled.
    pub vt_record_dir: Option<std::path::PathBuf>,
    /// Process-type-specific driver. Handles state detection and conversation tracking.
    pub driver: Box<dyn ProcessDriver>,
    /// Channel for broadcasting state changes to WebSocket clients.
    pub state_broadcast_tx: Option<StateBroadcast>,
    /// Channel for broadcasting lifecycle events (SessionRotated, etc.).
    pub lifecycle_tx: Option<broadcast::Sender<crate::ws::ServerMessage>>,
    /// Shared session-claiming map (for driver conversation watcher).
    pub claimed_sessions: Arc<RwLock<HashMap<String, String>>>,
    /// Shared first-input data (for session discovery).
    pub first_input_data: Arc<RwLock<HashMap<String, FirstInputData>>>,
    /// Shared pending attributions (for conversation formatting).
    pub pending_attributions: Arc<RwLock<HashMap<String, VecDeque<PendingAttribution>>>>,
    /// Repository for conversation persistence.
    pub repository: Option<Arc<ConversationRepository>>,
}

/// Handle VT commands.
/// Returns `None` if handled, `Some(cmd)` for PTY/metadata commands.
fn handle_vt_command(vt: &mut VirtualTerminal, cmd: InstanceCommand) -> Option<InstanceCommand> {
    match cmd {
        InstanceCommand::UpdateViewport {
            connection_id,
            rows,
            cols,
            client_type,
            respond_to,
        } => {
            let result = vt.update_viewport(&connection_id, rows, cols, client_type);
            let _ = respond_to.send(result);
            None
        }
        InstanceCommand::SetClientActive {
            connection_id,
            active,
            respond_to,
        } => {
            let result = vt.set_active(&connection_id, active);
            let _ = respond_to.send(result);
            None
        }
        InstanceCommand::RemoveClient {
            connection_id,
            respond_to,
        } => {
            let result = vt.remove_client(&connection_id);
            let _ = respond_to.send(result);
            None
        }
        InstanceCommand::GetRecentOutput {
            max_bytes,
            client_rows,
            respond_to,
        } => {
            let replay = vt.replay(client_rows);
            let data = if replay.len() > max_bytes {
                let mut start = replay.len() - max_bytes;
                // Skip past UTF-8 continuation bytes (10xxxxxx) so we
                // don't slice in the middle of a multi-byte character.
                while start < replay.len() && (replay[start] & 0xC0) == 0x80 {
                    start += 1;
                }
                replay[start..].to_vec()
            } else {
                replay
            };
            let _ = respond_to.send(vec![String::from_utf8_lossy(&data).to_string()]);
            None
        }
        other => Some(other),
    }
}

/// The instance actor that manages a single PTY session
struct InstanceActor {
    info: Arc<RwLock<InstanceInfo>>,
    pty: PtyHandle,
    receiver: mpsc::Receiver<InstanceCommand>,
    virtual_terminal: VirtualTerminal,
    enriched_tx: broadcast::Sender<EnrichedOutput>,
    recorder: Option<VtRecorder<std::fs::File>>,
    pty_output_rx: broadcast::Receiver<PtyOutput>,
    driver: Box<dyn ProcessDriver>,
    driver_rx: Option<mpsc::Receiver<DriverSignal>>,
    state_broadcast_tx: Option<StateBroadcast>,
    lifecycle_tx: Option<broadcast::Sender<crate::ws::ServerMessage>>,
    claimed_sessions: Arc<RwLock<HashMap<String, String>>>,
    first_input_data: Arc<RwLock<HashMap<String, FirstInputData>>>,
    pending_attributions: Arc<RwLock<HashMap<String, VecDeque<PendingAttribution>>>>,
    repository: Option<Arc<ConversationRepository>>,
}

impl InstanceActor {
    /// Spawn a new instance actor and return its handle
    pub async fn spawn(opts: SpawnOptions) -> Result<InstanceHandle> {
        let id = Uuid::new_v4().to_string();

        debug!(
            "Starting instance actor '{}' with command '{}' (actual: '{}', args: {:?}, working_dir: '{}')",
            opts.name, opts.display_command, opts.actual_command, opts.args, opts.working_dir
        );

        // Create PTY configuration
        let config = PtyConfig {
            command: opts.actual_command.clone(),
            args: opts.args.clone(),
            working_dir: Some(opts.working_dir.clone()),
            env: Vec::new(),
            rows: 24,
            cols: 80,
        };

        // Start PTY session using pty_manager
        let pty = pty_manager::pty::PtyActor::spawn(config).map_err(|e| {
            tracing::error!(
                "Failed to start PTY for '{}': command='{}' args={:?} working_dir='{}' - {}",
                opts.name,
                opts.actual_command,
                opts.args,
                opts.working_dir,
                e
            );
            anyhow::anyhow!("{}", e)
        })?;

        let info = Arc::new(RwLock::new(InstanceInfo {
            id: id.clone(),
            name: opts.name.clone(),
            custom_name: None,
            command: opts.display_command.clone(),
            kind: opts.kind,
            working_dir: opts.working_dir,
            running: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            session_id: None,
            claude_state: Some(ClaudeState::Initializing),
        }));

        let (sender, receiver) = mpsc::channel(32);
        let virtual_terminal =
            VirtualTerminal::new(24, 80, opts.max_buffer_bytes, opts.scrollback_lines);
        let (enriched_tx, _) = broadcast::channel::<EnrichedOutput>(64);

        let recorder = opts.vt_record_dir.as_ref().and_then(|dir| {
            if let Err(e) = std::fs::create_dir_all(dir) {
                tracing::warn!(
                    "VT recording disabled — cannot create {}: {e}",
                    dir.display()
                );
                return None;
            }
            let path = dir.join(format!("{id}.vtr"));
            match VtRecorder::open(&path, 24, 80, opts.scrollback_lines as u32) {
                Ok(r) => {
                    debug!("VT recording → {}", path.display());
                    Some(r)
                }
                Err(e) => {
                    tracing::warn!(
                        "VT recording disabled — cannot open {}: {e}",
                        path.display()
                    );
                    None
                }
            }
        });

        let pty_output_rx = pty.subscribe();

        let actor = InstanceActor {
            info: info.clone(),
            pty: pty.clone(),
            receiver,
            virtual_terminal,
            enriched_tx,
            recorder,
            pty_output_rx,
            driver: opts.driver,
            driver_rx: None,
            state_broadcast_tx: opts.state_broadcast_tx,
            lifecycle_tx: opts.lifecycle_tx,
            claimed_sessions: opts.claimed_sessions,
            first_input_data: opts.first_input_data,
            pending_attributions: opts.pending_attributions,
            repository: opts.repository,
        };

        // Spawn the actor task
        tokio::spawn(async move {
            actor.run().await;
        });

        Ok(InstanceHandle { sender, info })
    }

    /// Process PTY output: feed VT, feed driver, record, and broadcast enriched output.
    async fn process_pty_output(&mut self, event: PtyOutput) {
        if let Some(ref mut rec) = self.recorder {
            rec.output(&event.data);
        }
        self.virtual_terminal.process_output(&event.data);
        let cursor = self.virtual_terminal.cursor_position();

        // Feed driver for state detection
        if self.driver.on_output(&event.data).is_some() {
            self.broadcast_state().await;
        }

        let _ = self.enriched_tx.send(EnrichedOutput {
            data: event.data,
            cursor,
            timestamp: event.timestamp,
        });
    }

    /// Broadcast a state change: update InstanceInfo and send through state_broadcast_tx.
    async fn broadcast_state(&mut self) {
        // Map to ClaudeState for backward compatibility
        let claude_state = if let Some(cs) = self.driver.claude_state() {
            cs.clone()
        } else {
            // Best-effort mapping for non-Claude drivers
            ClaudeState::Idle
        };

        let instance_id = self.info.read().await.id.clone();
        self.info.write().await.claude_state = Some(claude_state.clone());

        if let Some(ref tx) = self.state_broadcast_tx {
            let terminal_stale = self.driver.is_terminal_stale();
            let _ = tx.send((instance_id, claude_state, terminal_stale));
        }
    }

    /// Apply a DriverEffect returned by the driver's on_signal method.
    async fn apply_effect(&mut self, effect: crate::process_driver::DriverEffect) {
        if effect.state_change.is_some() {
            self.broadcast_state().await;
        }

        if let Some(ref session_id) = effect.session_id {
            let instance_id = self.info.read().await.id.clone();
            let old_session = self.info.read().await.session_id.clone();
            self.info.write().await.session_id = Some(session_id.clone());
            info!("Instance '{}' session set to {}", instance_id, session_id);

            // If there was a previous session, broadcast rotation
            if let Some(old) = old_session
                && let Some(ref ltx) = self.lifecycle_tx
            {
                let _ = ltx.send(crate::ws::ServerMessage::SessionRotated {
                    instance_id,
                    from_session: old,
                    to_session: session_id.clone(),
                });
            }
        }
    }

    async fn run(mut self) {
        let name = self.info.read().await.name.clone();
        debug!("Instance actor '{}' started", name);

        // Start the driver — this may spawn background tasks and return a signal receiver.
        let driver_ctx = DriverContext {
            working_dir: self.info.read().await.working_dir.clone(),
            instance_id: self.info.read().await.id.clone(),
            instance_created_at: chrono::DateTime::parse_from_rfc3339(
                &self.info.read().await.created_at,
            )
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
            claimed_sessions: Arc::clone(&self.claimed_sessions),
            first_input_data: Arc::clone(&self.first_input_data),
            pending_attributions: Arc::clone(&self.pending_attributions),
            repository: self.repository.clone(),
        };
        self.driver_rx = self.driver.start(driver_ctx);

        let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(500));

        loop {
            tokio::select! {
                Some(cmd) = self.receiver.recv() => {
                    let cmd = match handle_vt_command(&mut self.virtual_terminal, cmd) {
                        Some(cmd) => cmd,
                        None => continue,
                    };
                    match cmd {
                        InstanceCommand::GetInfo { respond_to } => {
                            let _ = respond_to.send(self.info.read().await.clone());
                        }

                        InstanceCommand::WriteInput { text, respond_to } => {
                            debug!("Writing {} bytes to PTY", text.len());
                            if let Some(ref mut rec) = self.recorder {
                                rec.input(text.as_bytes());
                            }
                            // Feed driver for input-based state detection
                            if self.driver.on_input(&text).is_some() {
                                self.broadcast_state().await;
                            }
                            let result = self
                                .pty
                                .write_str(&text)
                                .await
                                .map_err(|e| anyhow::anyhow!("{}", e));
                            let _ = respond_to.send(result);
                        }

                        InstanceCommand::Resize {
                            rows,
                            cols,
                            respond_to,
                        } => {
                            if let Some(ref mut rec) = self.recorder {
                                rec.resize(rows, cols);
                            }
                            let result = self
                                .pty
                                .resize(rows, cols)
                                .await
                                .map_err(|e| anyhow::anyhow!("{}", e));
                            let _ = respond_to.send(result);
                        }

                        InstanceCommand::SubscribeOutput { respond_to } => {
                            let rx = self.enriched_tx.subscribe();
                            let _ = respond_to.send(rx);
                        }

                        InstanceCommand::SetSessionId {
                            session_id,
                            respond_to,
                        } => {
                            debug!("Setting session_id for instance '{}': {}", name, session_id);
                            self.info.write().await.session_id = Some(session_id);
                            let _ = respond_to.send(());
                        }

                        InstanceCommand::GetSessionId { respond_to } => {
                            let session_id = self.info.read().await.session_id.clone();
                            let _ = respond_to.send(session_id);
                        }

                        InstanceCommand::GetPid { respond_to } => {
                            let pid = match self.pty.state().await {
                                Ok(state) => state.pid,
                                Err(_) => None,
                            };
                            let _ = respond_to.send(pid);
                        }

                        InstanceCommand::GetConversationSnapshot { respond_to } => {
                            let snapshot = self.driver.conversation_snapshot().to_vec();
                            let _ = respond_to.send(snapshot);
                        }

                        InstanceCommand::SubscribeConversation { respond_to } => {
                            let rx = self.driver.subscribe_conversation();
                            let _ = respond_to.send(rx);
                        }

                        InstanceCommand::SetCustomName {
                            name: custom_name,
                            respond_to,
                        } => {
                            debug!(
                                "Setting custom_name for instance '{}': {:?}",
                                name, custom_name
                            );
                            self.info.write().await.custom_name = custom_name;
                            let _ = respond_to.send(());
                        }

                        InstanceCommand::Stop { respond_to } => {
                            debug!("Stopping instance '{}'", name);
                            self.info.write().await.running = false;
                            let result = self
                                .pty
                                .kill(Some("SIGTERM"))
                                .await
                                .map_err(|e| anyhow::anyhow!("{}", e));
                            let _ = respond_to.send(result);
                            break; // Exit the actor loop
                        }
                        _ => {} // VT commands handled by handle_vt_command
                    }
                }
                result = self.pty_output_rx.recv() => {
                    match result {
                        Ok(event) => self.process_pty_output(event).await,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("PTY output lagged by {n} messages");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                signal = async {
                    match self.driver_rx {
                        Some(ref mut rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match signal {
                        Some(signal) => {
                            let effect = self.driver.on_signal(signal);
                            self.apply_effect(effect).await;
                        }
                        None => {
                            // Driver signal channel closed — driver background task ended
                            debug!("Driver signal channel closed for instance '{}'", name);
                            self.driver_rx = None;
                        }
                    }
                }
                _ = tick_interval.tick() => {
                    if self.driver.tick().is_some() {
                        self.broadcast_state().await;
                    }
                }
            }
        }

        debug!("Instance actor '{}' stopped", name);
    }
}

/// Create a new instance and return its handle
pub async fn create_instance(opts: SpawnOptions) -> Result<InstanceHandle> {
    InstanceActor::spawn(opts).await
}

#[cfg(test)]
impl InstanceHandle {
    /// Spawn a test actor backed by a VirtualTerminal only (no real PTY).
    /// Returns the handle and an `mpsc::Sender<Vec<u8>>` for injecting output.
    /// The test actor processes injected bytes through its own VT (like real PTY output).
    pub(crate) fn spawn_test(
        rows: u16,
        cols: u16,
        max_delta_bytes: usize,
    ) -> (Self, mpsc::Sender<Vec<u8>>) {
        let (handle, output_tx, _convo) =
            Self::spawn_test_with_scrollback(rows, cols, max_delta_bytes, 0);
        (handle, output_tx)
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn spawn_test_with_scrollback(
        rows: u16,
        cols: u16,
        max_delta_bytes: usize,
        scrollback_lines: usize,
    ) -> (
        Self,
        mpsc::Sender<Vec<u8>>,
        Arc<RwLock<Vec<serde_json::Value>>>,
    ) {
        use crate::process_driver::ShellDriver;
        let _driver: Box<dyn ProcessDriver> = Box::new(ShellDriver);
        let info = Arc::new(RwLock::new(InstanceInfo {
            id: "test-instance".to_string(),
            name: "test".to_string(),
            custom_name: None,
            command: "echo test".to_string(),
            kind: InstanceKind::Unstructured {
                label: Some("echo".into()),
            },
            working_dir: "/tmp".to_string(),
            running: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            session_id: None,
            claude_state: None,
        }));
        let (sender, mut receiver) = mpsc::channel(32);
        let mut vt = VirtualTerminal::new(rows, cols, max_delta_bytes, scrollback_lines);
        let (enriched_tx, _) = broadcast::channel::<EnrichedOutput>(64);
        let enriched_tx_actor = enriched_tx.clone();
        let info_actor = info.clone();
        let conversation_turns: Arc<RwLock<Vec<serde_json::Value>>> =
            Arc::new(RwLock::new(Vec::new()));
        let convo_turns_actor = conversation_turns.clone();
        let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(64);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(cmd) = receiver.recv() => {
                        let cmd = match handle_vt_command(&mut vt, cmd) {
                            Some(cmd) => cmd,
                            None => continue,
                        };
                        match cmd {
                            InstanceCommand::Resize { respond_to, .. } => {
                                let _ = respond_to.send(Ok(()));
                            }
                            InstanceCommand::SubscribeOutput { respond_to } => {
                                let rx = enriched_tx_actor.subscribe();
                                let _ = respond_to.send(rx);
                            }
                            InstanceCommand::Stop { respond_to } => {
                                let _ = respond_to.send(Ok(()));
                                break;
                            }
                            InstanceCommand::SetSessionId {
                                session_id,
                                respond_to,
                            } => {
                                info_actor.write().await.session_id = Some(session_id);
                                let _ = respond_to.send(());
                            }
                            InstanceCommand::GetSessionId { respond_to } => {
                                let sid = info_actor.read().await.session_id.clone();
                                let _ = respond_to.send(sid);
                            }
                            InstanceCommand::WriteInput { text, respond_to } => {
                                // Accept writes silently (no real PTY in test)
                                let _ = respond_to.send(Ok(text.len()));
                            }
                            InstanceCommand::GetConversationSnapshot { respond_to } => {
                                let turns = convo_turns_actor.read().await.clone();
                                let _ = respond_to.send(turns);
                            }
                            InstanceCommand::SubscribeConversation { respond_to } => {
                                let _ = respond_to.send(None);
                            }
                            _ => {}
                        }
                    }
                    Some(data) = output_rx.recv() => {
                        vt.process_output(&data);
                        let cursor = vt.cursor_position();
                        let _ = enriched_tx_actor.send(EnrichedOutput {
                            data,
                            cursor,
                            timestamp: 0,
                        });
                    }
                    else => break,
                }
            }
        });

        (
            InstanceHandle { sender, info },
            output_tx,
            conversation_turns,
        )
    }

    /// Inject output into the test actor and yield so it processes the bytes.
    pub(crate) async fn inject_output(output_tx: &mpsc::Sender<Vec<u8>>, data: &[u8]) {
        let _ = output_tx.send(data.to_vec()).await;
        // Yield to let the actor task process the injected output
        tokio::task::yield_now().await;
    }

    /// Spawn a test actor with a custom driver.
    ///
    /// Unlike `spawn_test`/`spawn_test_with_scrollback` (which use ShellDriver),
    /// this routes all operations through the provided driver:
    /// - `on_output()` on injected bytes
    /// - `on_input()` on WriteInput
    /// - `conversation_snapshot()` for GetConversationSnapshot
    /// - `subscribe_conversation()` for SubscribeConversation
    /// - `on_signal()` for signals from `signal_rx`
    /// - `tick()` on a 500ms interval
    /// - `broadcast_state()` on driver state changes
    pub(crate) fn spawn_test_with_driver(
        driver: Box<dyn ProcessDriver>,
        signal_rx: Option<mpsc::Receiver<DriverSignal>>,
        state_broadcast_tx: Option<StateBroadcast>,
        lifecycle_tx: Option<broadcast::Sender<crate::ws::ServerMessage>>,
    ) -> (Self, mpsc::Sender<Vec<u8>>) {
        let info = Arc::new(RwLock::new(InstanceInfo {
            id: "test-instance".to_string(),
            name: "test".to_string(),
            custom_name: None,
            command: "echo test".to_string(),
            kind: InstanceKind::infer("echo test"),
            working_dir: "/tmp".to_string(),
            running: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            session_id: None,
            claude_state: Some(ClaudeState::Initializing),
        }));
        let (sender, mut receiver) = mpsc::channel(32);
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        let (enriched_tx, _) = broadcast::channel::<EnrichedOutput>(64);
        let enriched_tx_actor = enriched_tx.clone();
        let info_actor = info.clone();
        let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(64);
        let mut driver = driver;
        let mut signal_rx = signal_rx;
        let state_broadcast_tx = state_broadcast_tx;
        let lifecycle_tx = lifecycle_tx;

        tokio::spawn(async move {
            let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(500));

            loop {
                tokio::select! {
                    Some(cmd) = receiver.recv() => {
                        let cmd = match handle_vt_command(&mut vt, cmd) {
                            Some(cmd) => cmd,
                            None => continue,
                        };
                        match cmd {
                            InstanceCommand::WriteInput { text, respond_to } => {
                                if let Some(new_state) = driver.on_input(&text) {
                                    let _ = new_state; // consumed
                                    // Broadcast state
                                    let claude_state = driver.claude_state()
                                        .cloned()
                                        .unwrap_or(ClaudeState::Idle);
                                    let instance_id = info_actor.read().await.id.clone();
                                    info_actor.write().await.claude_state = Some(claude_state.clone());
                                    if let Some(ref tx) = state_broadcast_tx {
                                        let terminal_stale = driver.is_terminal_stale();
                                        let _ = tx.send((instance_id, claude_state, terminal_stale));
                                    }
                                }
                                let _ = respond_to.send(Ok(text.len()));
                            }
                            InstanceCommand::GetConversationSnapshot { respond_to } => {
                                let snapshot = driver.conversation_snapshot().to_vec();
                                let _ = respond_to.send(snapshot);
                            }
                            InstanceCommand::SubscribeConversation { respond_to } => {
                                let rx = driver.subscribe_conversation();
                                let _ = respond_to.send(rx);
                            }
                            InstanceCommand::GetInfo { respond_to } => {
                                let _ = respond_to.send(info_actor.read().await.clone());
                            }
                            InstanceCommand::Stop { respond_to } => {
                                let _ = respond_to.send(Ok(()));
                                break;
                            }
                            InstanceCommand::Resize { respond_to, .. } => {
                                let _ = respond_to.send(Ok(()));
                            }
                            InstanceCommand::SubscribeOutput { respond_to } => {
                                let rx = enriched_tx_actor.subscribe();
                                let _ = respond_to.send(rx);
                            }
                            InstanceCommand::SetSessionId { session_id, respond_to } => {
                                info_actor.write().await.session_id = Some(session_id);
                                let _ = respond_to.send(());
                            }
                            InstanceCommand::GetSessionId { respond_to } => {
                                let sid = info_actor.read().await.session_id.clone();
                                let _ = respond_to.send(sid);
                            }
                            _ => {}
                        }
                    }
                    Some(data) = output_rx.recv() => {
                        vt.process_output(&data);
                        let cursor = vt.cursor_position();
                        // Feed driver
                        if let Some(_new_state) = driver.on_output(&data) {
                            let claude_state = driver.claude_state()
                                .cloned()
                                .unwrap_or(ClaudeState::Idle);
                            let instance_id = info_actor.read().await.id.clone();
                            info_actor.write().await.claude_state = Some(claude_state.clone());
                            if let Some(ref tx) = state_broadcast_tx {
                                let terminal_stale = driver.is_terminal_stale();
                                let _ = tx.send((instance_id, claude_state, terminal_stale));
                            }
                        }
                        let _ = enriched_tx_actor.send(EnrichedOutput {
                            data,
                            cursor,
                            timestamp: 0,
                        });
                    }
                    signal = async {
                        match signal_rx {
                            Some(ref mut rx) => rx.recv().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match signal {
                            Some(signal) => {
                                let effect = driver.on_signal(signal);
                                // Apply effect: state broadcast
                                if effect.state_change.is_some() {
                                    let claude_state = driver.claude_state()
                                        .cloned()
                                        .unwrap_or(ClaudeState::Idle);
                                    let instance_id = info_actor.read().await.id.clone();
                                    info_actor.write().await.claude_state = Some(claude_state.clone());
                                    if let Some(ref tx) = state_broadcast_tx {
                                        let terminal_stale = driver.is_terminal_stale();
                                        let _ = tx.send((instance_id, claude_state, terminal_stale));
                                    }
                                }
                                // Apply effect: session_id
                                if let Some(ref session_id) = effect.session_id {
                                    let instance_id = info_actor.read().await.id.clone();
                                    let old_session = info_actor.read().await.session_id.clone();
                                    info_actor.write().await.session_id = Some(session_id.clone());
                                    if let Some(old) = old_session
                                        && let Some(ref ltx) = lifecycle_tx
                                    {
                                        let _ = ltx.send(crate::ws::ServerMessage::SessionRotated {
                                            instance_id,
                                            from_session: old,
                                            to_session: session_id.clone(),
                                        });
                                    }
                                }
                            }
                            None => {
                                signal_rx = None;
                            }
                        }
                    }
                    _ = tick_interval.tick() => {
                        if let Some(_new_state) = driver.tick() {
                            let claude_state = driver.claude_state()
                                .cloned()
                                .unwrap_or(ClaudeState::Idle);
                            let instance_id = info_actor.read().await.id.clone();
                            info_actor.write().await.claude_state = Some(claude_state.clone());
                            if let Some(ref tx) = state_broadcast_tx {
                                let terminal_stale = driver.is_terminal_stale();
                                let _ = tx.send((instance_id, claude_state, terminal_stale));
                            }
                        }
                    }
                    else => break,
                }
            }
        });

        (InstanceHandle { sender, info }, output_tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_update_viewport() {
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        let result = handle
            .update_viewport("client-1", 40, 120, ClientType::Web)
            .await;
        assert_eq!(result, Some((40, 120)));

        // Same dims again → no change
        let result = handle
            .update_viewport("client-1", 40, 120, ClientType::Web)
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_handle_set_client_active() {
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        handle
            .update_viewport("client-1", 40, 120, ClientType::Web)
            .await;
        handle
            .update_viewport("client-2", 24, 80, ClientType::Terminal)
            .await;

        // Hide smaller client → dims go up
        let result = handle.set_client_active("client-2", false).await;
        assert_eq!(result, Some((40, 120)));
    }

    #[tokio::test]
    async fn test_handle_remove_client() {
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        handle
            .update_viewport("client-1", 40, 120, ClientType::Web)
            .await;
        handle
            .update_viewport("client-2", 24, 80, ClientType::Terminal)
            .await;

        let result = handle.remove_client("client-2").await;
        assert_eq!(result, Some((40, 120)));
    }

    #[tokio::test]
    async fn test_handle_get_recent_output_replay() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        // Inject output through the actor
        InstanceHandle::inject_output(&output_tx, b"Hello from test\r\nLine 2").await;

        let output = handle.get_recent_output(4096, 24).await;
        assert_eq!(output.len(), 1);
        assert!(output[0].contains("Hello from test"));
        assert!(output[0].contains("Line 2"));
    }

    #[tokio::test]
    async fn test_handle_get_recent_output_truncation() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        InstanceHandle::inject_output(&output_tx, "A".repeat(200).as_bytes()).await;

        let full = handle.get_recent_output(100_000, 24).await;
        let full_len = full[0].len();
        assert!(full_len > 50);

        let truncated = handle.get_recent_output(50, 24).await;
        assert!(truncated[0].len() <= 50);
        assert!(truncated[0].len() < full_len);
    }

    #[tokio::test]
    async fn test_update_viewport_and_resize() {
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        // Dims change triggers resize (no-op in test actor)
        let result = handle
            .update_viewport_and_resize("conn-1", 40, 120, ClientType::Web)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_active_and_resize() {
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        handle
            .update_viewport("conn-1", 40, 120, ClientType::Web)
            .await;
        handle
            .update_viewport("conn-2", 24, 80, ClientType::Terminal)
            .await;

        // Deactivate smaller → resize to larger
        let result = handle.set_active_and_resize("conn-2", false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_client_and_resize_no_change() {
        let (handle, _output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        // Remove nonexistent → no dims change, no resize, still Ok
        let result = handle.remove_client_and_resize("nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_truncation_never_splits_utf8() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        // Write box-drawing characters (3 bytes each in UTF-8)
        let content = "─".repeat(40); // 120 bytes
        InstanceHandle::inject_output(&output_tx, content.as_bytes()).await;

        // The replay includes a keyframe prefix (ANSI escapes), so the
        // total is larger than 120 bytes.  Try various max_bytes values
        // that are likely to land mid-character.
        for max_bytes in 1..=150 {
            let output = handle.get_recent_output(max_bytes, 24).await;
            if output.is_empty() {
                continue;
            }
            assert!(
                !output[0].contains('\u{FFFD}'),
                "max_bytes={}: truncation produced replacement character in {:?}",
                max_bytes,
                &output[0][..output[0].len().min(40)],
            );
        }
    }

    #[tokio::test]
    async fn test_truncation_preserves_content() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        // Mix of ASCII and multi-byte: "hello──world" (5 + 6 + 5 = 16 bytes text)
        let content = "hello──world";
        InstanceHandle::inject_output(&output_tx, content.as_bytes()).await;

        // Full replay should contain the content
        let full = handle.get_recent_output(100_000, 24).await;
        assert!(full[0].contains("hello"));
        assert!(full[0].contains("──"));
        assert!(full[0].contains("world"));

        // Truncated replay should also be valid UTF-8 (no replacement chars)
        for max_bytes in 1..=60 {
            let output = handle.get_recent_output(max_bytes, 24).await;
            if output.is_empty() {
                continue;
            }
            assert!(
                !output[0].contains('\u{FFFD}'),
                "max_bytes={}: got replacement char",
                max_bytes,
            );
        }
    }

    #[tokio::test]
    async fn test_truncation_4byte_chars() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);

        // 4-byte emoji characters
        let content = "🦀".repeat(10); // 40 bytes
        InstanceHandle::inject_output(&output_tx, content.as_bytes()).await;

        for max_bytes in 1..=80 {
            let output = handle.get_recent_output(max_bytes, 24).await;
            if output.is_empty() {
                continue;
            }
            assert!(
                !output[0].contains('\u{FFFD}'),
                "max_bytes={}: truncation split a 4-byte character",
                max_bytes,
            );
        }
    }

    // ── Enriched output pipeline tests ───────────────────────────────

    #[tokio::test]
    async fn test_enriched_output_cursor_ascii() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        let mut rx = handle.subscribe_output().await.unwrap();

        InstanceHandle::inject_output(&output_tx, b"Hello").await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.data, b"Hello");
        assert_eq!(event.cursor, (0, 5));
    }

    #[tokio::test]
    async fn test_enriched_output_cursor_multiline() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        let mut rx = handle.subscribe_output().await.unwrap();

        InstanceHandle::inject_output(&output_tx, b"Hello\r\nWorld").await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.cursor, (1, 5));
    }

    #[tokio::test]
    async fn test_enriched_output_cursor_after_cup() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        let mut rx = handle.subscribe_output().await.unwrap();

        // CUP moves cursor to row 10, col 20 (1-indexed) → (9, 19) 0-indexed
        InstanceHandle::inject_output(&output_tx, b"\x1b[10;20H").await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.cursor, (9, 19));
    }

    #[tokio::test]
    async fn test_enriched_output_cursor_tracks_across_chunks() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        let mut rx = handle.subscribe_output().await.unwrap();

        InstanceHandle::inject_output(&output_tx, b"abc").await;
        let e1 = rx.recv().await.unwrap();
        assert_eq!(e1.cursor, (0, 3));

        InstanceHandle::inject_output(&output_tx, b"def").await;
        let e2 = rx.recv().await.unwrap();
        assert_eq!(e2.cursor, (0, 6));
    }

    #[tokio::test]
    async fn test_subscribe_output_receives_enriched() {
        let (handle, output_tx) = InstanceHandle::spawn_test(24, 80, 4096);
        let mut rx = handle.subscribe_output().await.unwrap();

        InstanceHandle::inject_output(&output_tx, b"Test output").await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.data, b"Test output");
        assert_eq!(event.cursor, (0, 11));
    }

    /// Simulate the multiplexed WebSocket duplication bug:
    ///
    /// 1. Focus subscribes to PTY output (like focus.rs:231)
    /// 2. PTY output arrives — focus task forwards it to the client
    /// 3. TerminalVisible handler generates a replay that ALSO includes this output
    /// 4. Client processes both — duplication
    ///
    /// This test proves the overlap exists at the instance_actor API level.
    #[tokio::test]
    async fn test_multiplexed_focus_replay_overlap() {
        let (handle, output_tx, _convo) =
            InstanceHandle::spawn_test_with_scrollback(4, 40, 4096, 500);

        // Fill VT with initial content so scrollback has data.
        for i in 0..12 {
            InstanceHandle::inject_output(&output_tx, format!("Init {i:02}\r\n").as_bytes()).await;
        }

        // Step 1: Focus subscribes to output.
        let mut output_rx = handle.subscribe_output().await.unwrap();

        // Step 2: "Late" output arrives after subscribe.
        // These are in BOTH the broadcast receiver AND the VT.
        for i in 0..6 {
            InstanceHandle::inject_output(&output_tx, format!("Late {i:02}\r\n").as_bytes()).await;
        }

        // Step 3: TerminalVisible handler generates replay from VT.
        let replay_chunks = handle.get_recent_output(usize::MAX, 4).await;
        let replay_data = replay_chunks.join("");

        // Collect the broadcast messages the focus task would forward.
        let mut broadcast_bytes = Vec::new();
        while let Ok(event) = output_rx.try_recv() {
            broadcast_bytes.extend_from_slice(&event.data);
        }
        assert!(
            !broadcast_bytes.is_empty(),
            "focus task should have received broadcast output"
        );

        // ── Clean client: replay only (what we want after the fix) ──
        let mut clean = vt100::Parser::new(4, 40, 1000);
        clean.process(replay_data.as_bytes());

        // ── Buggy client: replay THEN broadcast (current broken behavior) ──
        let mut buggy = vt100::Parser::new(4, 40, 1000);
        buggy.process(replay_data.as_bytes());
        buggy.process(&broadcast_bytes);

        // Count "Late 00" occurrences across scrollback + visible screen.
        let clean_count = count_occurrences(&mut clean, 40, "Late 00");
        let buggy_count = count_occurrences(&mut buggy, 40, "Late 00");

        assert_eq!(clean_count, 1, "clean client: 'Late 00' should appear once");
        // BUG: the buggy client sees "Late 00" twice — once from the replay
        // and once from the broadcast overlap pushing content into scrollback.
        assert!(
            buggy_count > 1,
            "buggy client: 'Late 00' should be duplicated, got {buggy_count}"
        );
    }

    /// After draining the broadcast receiver post-replay, the overlap is gone.
    #[tokio::test]
    async fn test_multiplexed_drain_prevents_duplication() {
        let (handle, output_tx, _convo) =
            InstanceHandle::spawn_test_with_scrollback(4, 40, 4096, 500);

        for i in 0..12 {
            InstanceHandle::inject_output(&output_tx, format!("Init {i:02}\r\n").as_bytes()).await;
        }

        let mut output_rx = handle.subscribe_output().await.unwrap();

        for i in 0..6 {
            InstanceHandle::inject_output(&output_tx, format!("Late {i:02}\r\n").as_bytes()).await;
        }

        // Generate replay.
        let replay_chunks = handle.get_recent_output(usize::MAX, 4).await;
        let replay_data = replay_chunks.join("");

        // DRAIN: discard broadcast messages already baked into the replay.
        while output_rx.try_recv().is_ok() {}

        // After the drain, no stale broadcasts remain.
        // Simulate client: write replay only (post-drain no extra output).
        let mut client = vt100::Parser::new(4, 40, 1000);
        client.process(replay_data.as_bytes());

        let count = count_occurrences(&mut client, 40, "Late 00");
        assert_eq!(count, 1, "'Late 00' should appear exactly once after drain");
    }

    // ── MockDriver + integration tests ───────────────────────────────

    use crate::process_driver::{DriverEffect, ProcessState};
    use std::sync::Mutex;

    /// What the MockDriver was asked to do.
    #[derive(Debug, Clone, PartialEq)]
    enum MockCall {
        OnOutput(Vec<u8>),
        OnInput(String),
        Tick,
        OnSignal(DriverSignal),
    }

    /// A configurable mock driver that records calls and returns canned values.
    struct MockDriver {
        calls: Arc<Mutex<Vec<MockCall>>>,
        /// If set, `on_output` returns this state.
        on_output_state: Arc<Mutex<Option<ProcessState>>>,
        /// If set, `on_input` returns this state.
        on_input_state: Arc<Mutex<Option<ProcessState>>>,
        /// If set, `on_signal` returns this effect.
        on_signal_effect: Arc<Mutex<Option<DriverEffect>>>,
        /// Conversation turns for `conversation_snapshot()`.
        conversation_turns: Vec<serde_json::Value>,
        /// Conversation broadcast sender.
        conversation_tx: broadcast::Sender<ConversationEvent>,
    }

    impl MockDriver {
        fn new() -> Self {
            let (conversation_tx, _) = broadcast::channel(16);
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                on_output_state: Arc::new(Mutex::new(None)),
                on_input_state: Arc::new(Mutex::new(None)),
                on_signal_effect: Arc::new(Mutex::new(None)),
                conversation_turns: vec![serde_json::json!({"role": "mock"})],
                conversation_tx,
            }
        }

        fn calls(&self) -> Arc<Mutex<Vec<MockCall>>> {
            Arc::clone(&self.calls)
        }
    }

    impl ProcessDriver for MockDriver {
        fn on_output(&mut self, data: &[u8]) -> Option<ProcessState> {
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::OnOutput(data.to_vec()));
            self.on_output_state.lock().unwrap().clone()
        }

        fn on_input(&mut self, data: &str) -> Option<ProcessState> {
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::OnInput(data.to_string()));
            self.on_input_state.lock().unwrap().clone()
        }

        fn tick(&mut self) -> Option<ProcessState> {
            self.calls.lock().unwrap().push(MockCall::Tick);
            None
        }

        fn start(&mut self, _: DriverContext) -> Option<mpsc::Receiver<DriverSignal>> {
            None // signal_rx is provided separately via spawn_test_with_driver
        }

        fn on_signal(&mut self, signal: DriverSignal) -> DriverEffect {
            self.calls.lock().unwrap().push(MockCall::OnSignal(signal));
            self.on_signal_effect
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(DriverEffect::none)
        }

        fn state(&self) -> ProcessState {
            ProcessState::Idle
        }

        fn claude_state(&self) -> Option<&ClaudeState> {
            // SAFETY: the Mutex is held briefly and we return a reference into it.
            // This works because we never move the MockDriver while a reference is live.
            // For tests only.
            None
        }

        fn is_terminal_stale(&self) -> bool {
            false
        }

        fn conversation_snapshot(&self) -> &[serde_json::Value] {
            &self.conversation_turns
        }

        fn subscribe_conversation(&self) -> Option<broadcast::Receiver<ConversationEvent>> {
            Some(self.conversation_tx.subscribe())
        }
    }

    #[tokio::test]
    async fn test_driver_on_output_called() {
        let mock = MockDriver::new();
        let calls = mock.calls();
        let (handle, output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), None, None, None);

        InstanceHandle::inject_output(&output_tx, b"hello").await;
        // Allow actor to process
        tokio::task::yield_now().await;

        let calls = calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, MockCall::OnOutput(d) if d == b"hello")),
            "on_output should have been called with 'hello', got: {:?}",
            *calls
        );

        drop(handle);
    }

    #[tokio::test]
    async fn test_driver_on_input_called() {
        let mock = MockDriver::new();
        let calls = mock.calls();
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), None, None, None);

        handle.write_input("world").await.unwrap();
        tokio::task::yield_now().await;

        let calls = calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, MockCall::OnInput(s) if s == "world")),
            "on_input should have been called with 'world', got: {:?}",
            *calls
        );

        drop(handle);
    }

    #[tokio::test]
    async fn test_state_broadcast_on_output_change() {
        let mock = MockDriver::new();
        *mock.on_output_state.lock().unwrap() = Some(ProcessState::Starting);
        let (state_tx, mut state_rx) = broadcast::channel(16);
        let (handle, output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), None, Some(state_tx), None);

        InstanceHandle::inject_output(&output_tx, b"boot").await;
        tokio::task::yield_now().await;

        // Should have received a state broadcast
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(100), state_rx.recv()).await;
        assert!(result.is_ok(), "should receive state broadcast");
        let (instance_id, _state, _stale) = result.unwrap().unwrap();
        assert_eq!(instance_id, "test-instance");

        drop(handle);
    }

    #[tokio::test]
    async fn test_driver_signal_dispatched() {
        let mock = MockDriver::new();
        let calls = mock.calls();
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), Some(signal_rx), None, None);

        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-1".into()))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        // Extra yields to ensure the actor processes the signal
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let calls = calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| matches!(c, MockCall::OnSignal(_))),
            "on_signal should have been called, got: {:?}",
            *calls
        );

        drop(handle);
    }

    #[tokio::test]
    async fn test_session_set_on_signal_effect() {
        let mock = MockDriver::new();
        *mock.on_signal_effect.lock().unwrap() = Some(DriverEffect {
            state_change: None,
            session_id: Some("sess-new".into()),
        });
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), Some(signal_rx), None, None);

        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-new".into()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let sid = handle.get_session_id().await;
        assert_eq!(sid, Some("sess-new".into()));

        drop(handle);
    }

    #[tokio::test]
    async fn test_session_rotation_lifecycle_broadcast() {
        let mock = MockDriver::new();
        // First signal sets session, second rotates.
        *mock.on_signal_effect.lock().unwrap() = Some(DriverEffect {
            state_change: None,
            session_id: Some("sess-old".into()),
        });
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (lifecycle_tx, mut lifecycle_rx) = broadcast::channel(16);
        let on_signal_effect = Arc::clone(&mock.on_signal_effect);
        let (handle, _output_tx) = InstanceHandle::spawn_test_with_driver(
            Box::new(mock),
            Some(signal_rx),
            None,
            Some(lifecycle_tx),
        );

        // First: set initial session
        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-old".into()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Second: rotate to new session
        *on_signal_effect.lock().unwrap() = Some(DriverEffect {
            state_change: None,
            session_id: Some("sess-new".into()),
        });
        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-new".into()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Should have received a SessionRotated lifecycle broadcast
        let msg =
            tokio::time::timeout(std::time::Duration::from_millis(100), lifecycle_rx.recv()).await;
        assert!(msg.is_ok(), "should receive lifecycle broadcast");
        match msg.unwrap().unwrap() {
            crate::ws::ServerMessage::SessionRotated {
                instance_id,
                from_session,
                to_session,
            } => {
                assert_eq!(instance_id, "test-instance");
                assert_eq!(from_session, "sess-old");
                assert_eq!(to_session, "sess-new");
            }
            other => panic!("expected SessionRotated, got: {:?}", other),
        }

        drop(handle);
    }

    #[tokio::test]
    async fn test_no_rotation_without_old_session() {
        let mock = MockDriver::new();
        *mock.on_signal_effect.lock().unwrap() = Some(DriverEffect {
            state_change: None,
            session_id: Some("sess-first".into()),
        });
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (lifecycle_tx, mut lifecycle_rx) = broadcast::channel(16);
        let (handle, _output_tx) = InstanceHandle::spawn_test_with_driver(
            Box::new(mock),
            Some(signal_rx),
            None,
            Some(lifecycle_tx),
        );

        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-first".into()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // No rotation should have been broadcast (this is the first session)
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(50), lifecycle_rx.recv()).await;
        assert!(
            result.is_err(),
            "no lifecycle broadcast expected for first session"
        );

        drop(handle);
    }

    #[tokio::test]
    async fn test_get_conversation_snapshot_routes_through_driver() {
        let mock = MockDriver::new();
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), None, None, None);

        let snapshot = handle.get_conversation_snapshot().await;
        assert_eq!(snapshot, vec![serde_json::json!({"role": "mock"})]);

        drop(handle);
    }

    #[tokio::test]
    async fn test_subscribe_conversation_routes_through_driver() {
        let mock = MockDriver::new();
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), None, None, None);

        let rx = handle.subscribe_conversation().await;
        assert!(
            rx.is_some(),
            "MockDriver should return Some from subscribe_conversation"
        );

        drop(handle);
    }

    #[tokio::test]
    async fn test_tick_called_by_actor() {
        let mock = MockDriver::new();
        let calls = mock.calls();
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), None, None, None);

        // Wait past the 500ms tick interval
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        tokio::task::yield_now().await;

        let calls = calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| matches!(c, MockCall::Tick)),
            "tick should have been called, got: {:?}",
            *calls
        );

        drop(handle);
    }

    #[tokio::test]
    async fn test_state_change_on_input() {
        let mock = MockDriver::new();
        *mock.on_input_state.lock().unwrap() = Some(ProcessState::Working { detail: None });
        let (state_tx, mut state_rx) = broadcast::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(mock), None, Some(state_tx), None);

        handle.write_input("go").await.unwrap();
        tokio::task::yield_now().await;

        let result =
            tokio::time::timeout(std::time::Duration::from_millis(100), state_rx.recv()).await;
        assert!(
            result.is_ok(),
            "should receive state broadcast from on_input"
        );

        drop(handle);
    }

    // ── ClaudeDriver integration tests ──────────────────────────────

    use crate::claude_driver::ClaudeDriver;

    fn make_turn(n: usize) -> serde_json::Value {
        serde_json::json!({"uuid": format!("turn-{n}"), "role": "user", "text": format!("turn {n}")})
    }

    #[tokio::test]
    async fn int_claude_subscribe_then_signal_delivers_broadcast() {
        // Hypotheses: C1, C3, E2
        // Subscribe first, then send snapshot signal → broadcast should deliver.
        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), Some(signal_rx), None, None);

        let mut rx = handle.subscribe_conversation().await.unwrap();

        let turns = vec![make_turn(0), make_turn(1)];
        signal_tx
            .send(DriverSignal::ConversationSnapshot(turns.clone()))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let event = rx.try_recv().expect("should receive broadcast");
        match event {
            ConversationEvent::Full {
                instance_id,
                turns: t,
            } => {
                assert_eq!(instance_id, "test-instance");
                assert_eq!(t.len(), 2);
                assert_eq!(t, turns);
            }
            _ => panic!("expected Full event, got {:?}", event),
        }

        drop(handle);
    }

    #[tokio::test]
    async fn int_claude_snapshot_persists_after_signal() {
        // Hypothesis: B1 integration — data stored even with no subscriber.
        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), Some(signal_rx), None, None);

        let turns = vec![make_turn(0)];
        signal_tx
            .send(DriverSignal::ConversationSnapshot(turns.clone()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let snapshot = handle.get_conversation_snapshot().await;
        assert_eq!(snapshot, turns);

        drop(handle);
    }

    #[tokio::test]
    async fn int_claude_session_discovered_sets_session_id() {
        // Hypothesis: E1 — SessionDiscovered sets info.session_id via DriverEffect.
        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), Some(signal_rx), None, None);

        assert_eq!(handle.get_session_id().await, None);

        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-abc".into()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        assert_eq!(handle.get_session_id().await, Some("sess-abc".into()));

        drop(handle);
    }

    #[tokio::test]
    async fn int_claude_delta_extends_and_broadcasts() {
        // Hypothesis: C2 — delta extends internal state, broadcasts Update.
        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), Some(signal_rx), None, None);

        let mut rx = handle.subscribe_conversation().await.unwrap();

        // Send initial snapshot
        signal_tx
            .send(DriverSignal::ConversationSnapshot(vec![make_turn(0)]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let event = rx.try_recv().expect("should receive Full");
        assert!(matches!(event, ConversationEvent::Full { .. }));

        // Send delta
        signal_tx
            .send(DriverSignal::ConversationDelta(vec![make_turn(1)]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let event = rx.try_recv().expect("should receive Update");
        match event {
            ConversationEvent::Update { turns, .. } => {
                assert_eq!(turns.len(), 1);
                assert_eq!(turns[0]["uuid"], "turn-1");
            }
            _ => panic!("expected Update event"),
        }

        // Full snapshot should have both turns
        let snapshot = handle.get_conversation_snapshot().await;
        assert_eq!(snapshot.len(), 2);

        drop(handle);
    }

    #[tokio::test]
    async fn int_claude_late_subscribe_misses_broadcast_gets_snapshot() {
        // Hypotheses: E2, C3 — late subscriber gets data via snapshot, not broadcast.
        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), Some(signal_rx), None, None);

        // Send signal BEFORE subscribing
        signal_tx
            .send(DriverSignal::ConversationSnapshot(vec![make_turn(0)]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Subscribe AFTER the broadcast
        let mut rx = handle.subscribe_conversation().await.unwrap();

        // Broadcast was before subscription — nothing in receiver
        assert!(
            rx.try_recv().is_err(),
            "late subscriber should not receive past broadcast"
        );

        // But data IS available via snapshot
        let snapshot = handle.get_conversation_snapshot().await;
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0]["uuid"], "turn-0");

        drop(handle);
    }

    #[tokio::test]
    async fn int_premature_session_id_blocks_mark_first_input() {
        // Hypotheses: A2, H4 integration — session_id set before input → mark_first_input skipped.
        use crate::ws::{GlobalStateManager, InputContext, create_state_broadcast};

        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        let (signal_tx, signal_rx) = mpsc::channel(16);
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), Some(signal_rx), None, None);

        let gsm = GlobalStateManager::new(create_state_broadcast());
        gsm.insert_test_tracker("test-instance", handle.clone())
            .await;

        // Set session_id BEFORE any input (via driver signal)
        signal_tx
            .send(DriverSignal::SessionDiscovered("sess-premature".into()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        assert_eq!(handle.get_session_id().await, Some("sess-premature".into()));

        // Now send input through the pipeline
        let ctx = InputContext {
            instance_id: "test-instance".to_string(),
            data: "hello\r".to_string(),
            connection_id: "test-conn".to_string(),
            user: None,
            task_id: None,
        };
        let repo: Option<&Arc<crate::repository::ConversationRepository>> = None;
        let _ = gsm.handle_input(ctx, repo).await;

        // first_input_data should NOT be populated (session_id gate blocked it)
        assert!(
            gsm.get_first_input_at("test-instance").await.is_none(),
            "mark_first_input should be skipped when session_id is already set via driver"
        );

        drop(handle);
    }

    #[tokio::test]
    async fn int_no_session_id_allows_mark_first_input() {
        // Hypothesis: A2 counterpart — session_id is None → mark_first_input runs.
        use crate::ws::{GlobalStateManager, InputContext, create_state_broadcast};

        let driver = ClaudeDriver::new().with_test_instance_id("test-instance");
        // No signal channel → no session discovery
        let (handle, _output_tx) =
            InstanceHandle::spawn_test_with_driver(Box::new(driver), None, None, None);

        let gsm = GlobalStateManager::new(create_state_broadcast());
        gsm.insert_test_tracker("test-instance", handle.clone())
            .await;

        let repo: Option<&Arc<crate::repository::ConversationRepository>> = None;

        // Send keystrokes through the pipeline
        for ch in "hello".chars() {
            let ctx = InputContext {
                instance_id: "test-instance".to_string(),
                data: ch.to_string(),
                connection_id: "test-conn".to_string(),
                user: None,
                task_id: None,
            };
            let _ = gsm.handle_input(ctx, repo).await;
        }
        let ctx = InputContext {
            instance_id: "test-instance".to_string(),
            data: "\r".to_string(),
            connection_id: "test-conn".to_string(),
            user: None,
            task_id: None,
        };
        let _ = gsm.handle_input(ctx, repo).await;

        assert!(
            gsm.get_first_input_at("test-instance").await.is_some(),
            "mark_first_input should run when session_id is None"
        );
        let prefixes = gsm.get_discovery_content_prefixes("test-instance").await;
        assert_eq!(prefixes, vec!["hello"]);

        drop(handle);
    }

    /// Count how many times `needle` appears across scrollback + visible screen.
    fn count_occurrences(parser: &mut vt100::Parser, cols: u16, needle: &str) -> usize {
        let mut all_text = String::new();

        // Visible screen
        let (rows, _) = parser.screen().size();
        for r in 0..rows {
            for c in 0..cols {
                if let Some(cell) = parser.screen().cell(r, c) {
                    let s = cell.contents();
                    if s.is_empty() {
                        all_text.push(' ');
                    } else {
                        all_text.push_str(s);
                    }
                }
            }
            all_text.push('\n');
        }

        // Scrollback
        parser.screen_mut().set_scrollback(usize::MAX);
        let depth = parser.screen().scrollback();
        for offset in (1..=depth).rev() {
            parser.screen_mut().set_scrollback(offset);
            for c in 0..cols {
                if let Some(cell) = parser.screen().cell(0, c) {
                    let s = cell.contents();
                    if s.is_empty() {
                        all_text.push(' ');
                    } else {
                        all_text.push_str(s);
                    }
                }
            }
            all_text.push('\n');
        }
        parser.screen_mut().set_scrollback(0);

        all_text.matches(needle).count()
    }
}
