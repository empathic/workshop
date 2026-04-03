# Operations Guide

Running and maintaining a Workshop server.

## Endpoints

### Health Check

```sh
curl http://localhost:PORT/health
```

```json
{
  "status": "healthy",
  "instances": {"total": 2, "active": 2},
  "connections": 1,
  "uptime_secs": 3600
}
```

Status values:
- `healthy` — No errors recorded
- `degraded` — Errors have occurred (check metrics)

### Metrics

```sh
curl http://localhost:PORT/metrics
```

```json
{
  "uptime_secs": 3600,
  "connections": {"active": 1, "total": 5},
  "instances": {"active": 2, "total_created": 3, "stopped": 1},
  "messages": {"received": 1000, "sent": 5000, "dropped": 0},
  "errors": {"pty": 0, "websocket": 0},
  "performance": {"focus_switches": 10, "history_replays": 10, "history_bytes_sent": 640000}
}
```

### Key Metrics to Monitor

| Metric | Healthy Range | Action if Exceeded |
|--------|---------------|-------------------|
| `messages.dropped` | 0 | Client too slow, check network |
| `errors.pty` | 0 | Check system PTY limits |
| `errors.websocket` | Low | Check client connectivity |
| `connections.active` | Expected users | Investigate if much higher |

## Logging

### Log Levels

```sh
# Default
RUST_LOG=workshop=info cargo run -p workshop -- server

# Debug
RUST_LOG=workshop=debug cargo run -p workshop -- server

# Specific modules
RUST_LOG=workshop::ws=debug,workshop::inference=trace cargo run -p workshop -- server
```

Or use the `--debug` flag for debug-level logging:

```sh
work server --debug
```

### Key Log Messages

| Message | Meaning |
|---------|---------|
| `New multiplexed WebSocket connection` | Client connected |
| `WebSocket connection closed` | Client disconnected |
| `State changed` | Claude state transition |
| `PTY output lagged by N messages` | Backpressure detected |
| `Instance actor stopped` | Instance cleanly shut down |

## Data Directory

All runtime data lives in `~/.workshop/` (override with `--data-dir`):

```
~/.workshop/
├── config.toml          Configuration file
├── workshop.db          SQLite database
├── exports/             Exported conversations
└── logs/                Server logs
```

## Database Management

### Location

Default: `~/.workshop/workshop.db` (SQLite)

### Backup

```sh
# Manual backup
cp ~/.workshop/workshop.db ~/.workshop/backup-$(date +%Y%m%d).db

# Using API
curl -X POST http://localhost:PORT/api/admin/backup
```

### Reset

```sh
# With confirmation prompt
work server --reset-db
```

### Import Conversations

```sh
# Import all Claude Code conversations
work server --import-all

# Import from specific project
work server --import-from /path/to/project

# Via API (while running)
curl -X POST http://localhost:PORT/api/admin/import \
  -H "Content-Type: application/json" \
  -d '{"import_all": true}'
```

## Graceful Shutdown

Server handles `SIGTERM`:

1. Stops accepting new connections
2. Cancels all focus tasks
3. Stops all instances (sends SIGTERM to Claude processes)
4. Closes database connections
5. Exits

Recommended shutdown timeout: 30 seconds.

```sh
# Graceful stop
kill -TERM $(pgrep work)

# Wait for clean shutdown, then force if needed
sleep 30
kill -9 $(pgrep work)
```

## Error Scenarios and Recovery

### WebSocket Connection Drops

**Symptom:** Client disconnects unexpectedly

**Automatic Recovery:**
- Focus tasks cancelled via CancellationToken
- Connection metrics updated
- No manual intervention needed

**Manual Recovery:** Refresh browser

### Instance Process Exits

**Symptom:** Claude CLI process terminates unexpectedly

**Detection:**
- Instance marked as `running: false`
- Check `/metrics` for `instances.stopped` count

**Recovery:**
1. Review logs for exit reason
2. User can create new instance from UI
3. Check if Claude CLI itself has issues

### PTY Allocation Failure

**Symptom:** Instance creation fails with PTY error

**Cause:**
- System PTY limit reached (`/dev/pts` exhausted)
- File descriptor limit reached
- Permissions issue

**Recovery:**
1. Check file descriptor limit: `ulimit -n`
2. Check PTY availability: `ls /dev/pts | wc -l`
3. Increase limits in `/etc/security/limits.conf`
4. Restart server if needed

### High Memory Usage

**Symptom:** Server memory growing over time

**Cause:** Large output buffers from verbose Claude instances

**Investigation:**
1. Check `/metrics` for `performance.history_bytes_sent`
2. Count active instances: `instances.active`

**Recovery:**
1. Reduce `max_buffer_mb` in config if needed
2. Stop idle instances
3. Restart server to clear buffers

### State Detection Incorrect

**Symptom:** UI shows wrong Claude state (e.g., stuck on "Thinking")

**Cause:**
- Terminal pattern mismatch
- Conversation JSONL not updating

**Workaround:** State will self-correct on next conversation entry

**Long-term:** State detection uses conversation JSONL as authoritative source — terminal patterns are supplementary

### Backpressure / Message Drops

**Symptom:** `messages.dropped` > 0 in metrics

**Cause:** Client WebSocket can't keep up with output rate

**Impact:** Some terminal output missed (not critical — data still in buffer)

**Notification:** Client receives `OutputLagged` message to show indicator

**Recovery:**
1. Check client network connection
2. Reduce verbose output from Claude if possible
3. Consider filtering output client-side

## Troubleshooting Checklist

1. **Server won't start**
   - Check port availability: `lsof -i :PORT`
   - Check data directory permissions
   - Review logs with `RUST_LOG=debug`

2. **Instances won't create**
   - Check `claude` command exists: `which claude`
   - Check PTY limits: `ulimit -n`
   - Review PTY error count in metrics

3. **UI shows stale state**
   - Check WebSocket connection in browser devtools
   - Look for `OutputLagged` messages
   - State will self-correct on next conversation update

4. **High latency**
   - Check `messages.dropped` metric
   - Review `history_bytes_sent` for large replays
   - Consider reducing max history size

5. **Database corruption**
   - Stop server
   - Backup current database
   - Use `--reset-db` to start fresh
   - Re-import conversations with `--import-all`
