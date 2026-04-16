use anyhow::Result;
use pty_manager::{PtyConfig, PtyHandle, PtyOutput};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use virtual_terminal::{ClientType, VirtualTerminal};

const MAX_DELTA_BYTES: usize = 1024 * 1024;

enum Command {
    WriteInput {
        text: String,
        respond_to: oneshot::Sender<Result<usize, pty_manager::PtyError>>,
    },
    Resize {
        rows: u16,
        cols: u16,
        respond_to: oneshot::Sender<Result<(), pty_manager::PtyError>>,
    },
    UpdateViewport {
        id: String,
        rows: u16,
        cols: u16,
        respond_to: oneshot::Sender<Option<(u16, u16)>>,
    },
    RemoveClient {
        id: String,
        respond_to: oneshot::Sender<Option<(u16, u16)>>,
    },
    GetReplay {
        client_rows: u16,
        respond_to: oneshot::Sender<Vec<u8>>,
    },
    SubscribeOutput {
        respond_to: oneshot::Sender<broadcast::Receiver<Vec<u8>>>,
    },
    GetPid {
        respond_to: oneshot::Sender<Option<u32>>,
    },
    Stop {
        respond_to: oneshot::Sender<Result<()>>,
    },
}

/// Handle to a running PTY session. Cheap to clone.
#[derive(Clone)]
pub struct Session {
    tx: mpsc::Sender<Command>,
    effective_dims: watch::Receiver<(u16, u16)>,
}

impl Session {
    /// Spawn a new PTY session.
    pub fn spawn(
        command: &str,
        args: &[String],
        working_dir: &str,
        rows: u16,
        cols: u16,
        scrollback_lines: usize,
        session_id: &str,
    ) -> Result<Self> {
        let config = PtyConfig {
            command: command.to_string(),
            args: args.to_vec(),
            working_dir: Some(working_dir.to_string()),
            env: vec![("MELD_SESSION_ID".into(), session_id.to_string())],
            rows,
            cols,
        };
        let pty = pty_manager::pty::PtyActor::spawn(config)?;
        let pty_output_rx = pty.subscribe();
        let (output_tx, _) = broadcast::channel::<Vec<u8>>(64);
        let vt = VirtualTerminal::new(rows, cols, MAX_DELTA_BYTES, scrollback_lines);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (dims_tx, dims_rx) = watch::channel((rows, cols));

        tokio::spawn(actor_loop(
            pty,
            pty_output_rx,
            vt,
            output_tx,
            cmd_rx,
            dims_tx,
        ));

        Ok(Self {
            tx: cmd_tx,
            effective_dims: dims_rx,
        })
    }

    pub async fn write_input(&self, text: &str) -> Result<usize> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Command::WriteInput {
                text: text.to_string(),
                respond_to: tx,
            })
            .await?;
        Ok(rx.await??)
    }

    pub async fn subscribe_output(&self) -> Result<broadcast::Receiver<Vec<u8>>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Command::SubscribeOutput { respond_to: tx })
            .await?;
        Ok(rx.await?)
    }

    /// Subscribe to effective PTY dimension changes.
    pub fn subscribe_dims(&self) -> watch::Receiver<(u16, u16)> {
        self.effective_dims.clone()
    }

    pub async fn update_viewport(&self, id: &str, rows: u16, cols: u16) -> Option<(u16, u16)> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(Command::UpdateViewport {
                id: id.to_string(),
                rows,
                cols,
                respond_to: tx,
            })
            .await;
        rx.await.ok().flatten()
    }

    pub async fn remove_client(&self, id: &str) -> Option<(u16, u16)> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(Command::RemoveClient {
                id: id.to_string(),
                respond_to: tx,
            })
            .await;
        rx.await.ok().flatten()
    }

    /// Update viewport and resize the PTY if effective dims changed.
    pub async fn update_viewport_and_resize(&self, id: &str, rows: u16, cols: u16) -> Result<()> {
        if let Some((eff_rows, eff_cols)) = self.update_viewport(id, rows, cols).await {
            self.resize(eff_rows, eff_cols).await?;
        }
        Ok(())
    }

    pub async fn remove_client_and_resize(&self, id: &str) -> Result<()> {
        if let Some((eff_rows, eff_cols)) = self.remove_client(id).await {
            self.resize(eff_rows, eff_cols).await?;
        }
        Ok(())
    }

    pub async fn get_replay(&self, client_rows: u16) -> Vec<u8> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(Command::GetReplay {
                client_rows,
                respond_to: tx,
            })
            .await;
        rx.await.unwrap_or_default()
    }

    pub async fn get_pid(&self) -> Option<u32> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(Command::GetPid { respond_to: tx }).await;
        rx.await.ok().flatten()
    }

    pub async fn stop(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(Command::Stop { respond_to: tx }).await;
        rx.await??;
        Ok(())
    }

    async fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(Command::Resize {
                rows,
                cols,
                respond_to: tx,
            })
            .await;
        rx.await??;
        Ok(())
    }
}

async fn actor_loop(
    pty: PtyHandle,
    mut pty_output_rx: broadcast::Receiver<PtyOutput>,
    mut vt: VirtualTerminal,
    output_tx: broadcast::Sender<Vec<u8>>,
    mut cmd_rx: mpsc::Receiver<Command>,
    dims_tx: watch::Sender<(u16, u16)>,
) {
    // pty_manager's PtyHandle holds an output_tx clone, so pty_output_rx never
    // closes on its own when the child exits — only the reader thread's clone
    // drops. And the pty actor only re-checks child exit after processing a
    // message, and replies with cached state *before* that check. So the first
    // tick after exit still reports running=true; detection needs ~2 ticks —
    // keep the interval tight so the exit latency stays imperceptible.
    let mut exit_check = tokio::time::interval(std::time::Duration::from_millis(100));
    exit_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            result = pty_output_rx.recv() => {
                match result {
                    Ok(pty_output) => {
                        vt.process_output(&pty_output.data);
                        let _ = output_tx.send(pty_output.data);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = exit_check.tick() => {
                match pty.state().await {
                    Ok(state) if !state.running => break,
                    Err(_) => break,
                    _ => {}
                }
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    Command::WriteInput { text, respond_to } => {
                        let _ = respond_to.send(pty.write_str(&text).await);
                    }
                    Command::Resize { rows, cols, respond_to } => {
                        let result = pty.resize(rows, cols).await;
                        vt.resize(rows, cols);
                        let _ = dims_tx.send((rows, cols));
                        let _ = respond_to.send(result);
                    }
                    Command::UpdateViewport { id, rows, cols, respond_to } => {
                        let result = vt.update_viewport(&id, rows, cols, ClientType::Terminal);
                        let _ = respond_to.send(result);
                    }
                    Command::RemoveClient { id, respond_to } => {
                        let result = vt.remove_client(&id);
                        let _ = respond_to.send(result);
                    }
                    Command::GetReplay { client_rows, respond_to } => {
                        let _ = respond_to.send(vt.replay(client_rows));
                    }
                    Command::SubscribeOutput { respond_to } => {
                        let _ = respond_to.send(output_tx.subscribe());
                    }
                    Command::GetPid { respond_to } => {
                        let pid = match pty.state().await {
                            Ok(state) => state.pid,
                            Err(_) => None,
                        };
                        let _ = respond_to.send(pid);
                    }
                    Command::Stop { respond_to } => {
                        let result = pty.kill(None).await
                            .map_err(|e| anyhow::anyhow!(e));
                        let _ = respond_to.send(result);
                        break;
                    }
                }
            }
        }
    }
}
