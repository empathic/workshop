//! Server metrics for observability
//!
//! Provides runtime metrics for monitoring server health and performance.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Server-wide metrics
#[derive(Debug, Default)]
pub struct ServerMetrics {
    // Connection metrics
    /// Currently active WebSocket connections
    pub active_connections: AtomicU64,
    /// Total connections since server start
    pub total_connections: AtomicU64,

    // Instance metrics
    /// Currently running instances
    pub active_instances: AtomicU64,
    /// Total instances created since server start
    pub total_instances_created: AtomicU64,
    /// Instances that have been stopped
    pub instances_stopped: AtomicU64,

    // Message metrics
    /// WebSocket messages received from clients
    pub messages_received: AtomicU64,
    /// WebSocket messages sent to clients
    pub messages_sent: AtomicU64,
    /// Messages dropped due to backpressure
    pub messages_dropped: AtomicU64,

    // Error metrics
    /// PTY-related errors
    pub pty_errors: AtomicU64,
    /// WebSocket errors
    pub websocket_errors: AtomicU64,

    // Performance metrics
    /// Number of focus switches
    pub focus_switches: AtomicU64,
    /// Number of history replays
    pub history_replays: AtomicU64,
    /// Total bytes of history sent
    pub history_bytes_sent: AtomicU64,

    /// Server start time (for uptime calculation)
    start_time: Option<Instant>,
}

impl ServerMetrics {
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
            ..Default::default()
        }
    }

    // Connection tracking
    pub fn connection_opened(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.total_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    // Instance tracking
    pub fn instance_created(&self) {
        self.active_instances.fetch_add(1, Ordering::Relaxed);
        self.total_instances_created.fetch_add(1, Ordering::Relaxed);
    }

    pub fn instance_stopped(&self) {
        self.active_instances.fetch_sub(1, Ordering::Relaxed);
        self.instances_stopped.fetch_add(1, Ordering::Relaxed);
    }

    // Message tracking
    #[allow(dead_code)]
    pub fn message_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn message_dropped(&self) {
        self.messages_dropped.fetch_add(1, Ordering::Relaxed);
    }

    // Error tracking
    pub fn pty_error(&self) {
        self.pty_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0)
    }

    /// Create a snapshot of all metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            uptime_secs: self.uptime_secs(),
            connections: ConnectionMetrics {
                active: self.active_connections.load(Ordering::Relaxed),
                total: self.total_connections.load(Ordering::Relaxed),
            },
            instances: InstanceMetrics {
                active: self.active_instances.load(Ordering::Relaxed),
                total_created: self.total_instances_created.load(Ordering::Relaxed),
                stopped: self.instances_stopped.load(Ordering::Relaxed),
            },
            messages: MessageMetrics {
                received: self.messages_received.load(Ordering::Relaxed),
                sent: self.messages_sent.load(Ordering::Relaxed),
                dropped: self.messages_dropped.load(Ordering::Relaxed),
            },
            errors: ErrorMetrics {
                pty: self.pty_errors.load(Ordering::Relaxed),
                websocket: self.websocket_errors.load(Ordering::Relaxed),
            },
            performance: PerformanceMetrics {
                focus_switches: self.focus_switches.load(Ordering::Relaxed),
                history_replays: self.history_replays.load(Ordering::Relaxed),
                history_bytes_sent: self.history_bytes_sent.load(Ordering::Relaxed),
            },
        }
    }
}

/// Serializable snapshot of metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub uptime_secs: u64,
    pub connections: ConnectionMetrics,
    pub instances: InstanceMetrics,
    pub messages: MessageMetrics,
    pub errors: ErrorMetrics,
    pub performance: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetrics {
    pub active: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceMetrics {
    pub active: u64,
    pub total_created: u64,
    pub stopped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetrics {
    pub received: u64,
    pub sent: u64,
    pub dropped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMetrics {
    pub pty: u64,
    pub websocket: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub focus_switches: u64,
    pub history_replays: u64,
    pub history_bytes_sent: u64,
}

/// Health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub instances: InstanceHealth,
    pub connections: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceHealth {
    pub total: u64,
    pub active: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_tracking() {
        let metrics = ServerMetrics::new();

        metrics.connection_opened();
        metrics.connection_opened();
        assert_eq!(metrics.active_connections.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.total_connections.load(Ordering::Relaxed), 2);

        metrics.connection_closed();
        assert_eq!(metrics.active_connections.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.total_connections.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_instance_tracking() {
        let metrics = ServerMetrics::new();

        metrics.instance_created();
        metrics.instance_created();
        assert_eq!(metrics.active_instances.load(Ordering::Relaxed), 2);

        metrics.instance_stopped();
        assert_eq!(metrics.active_instances.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.instances_stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_snapshot() {
        let metrics = ServerMetrics::new();
        metrics.connection_opened();
        metrics.instance_created();
        metrics.message_sent();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.connections.active, 1);
        assert_eq!(snapshot.instances.active, 1);
        assert_eq!(snapshot.messages.sent, 1);
    }

    #[test]
    fn test_message_dropped() {
        let metrics = ServerMetrics::new();
        metrics.message_dropped();
        metrics.message_dropped();
        assert_eq!(metrics.messages_dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_pty_error() {
        let metrics = ServerMetrics::new();
        metrics.pty_error();
        assert_eq!(metrics.pty_errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_uptime_secs() {
        let metrics = ServerMetrics::new();
        // Uptime should be >= 0 (could be 0 if test is fast)
        assert!(metrics.uptime_secs() < 5);
    }

    #[test]
    fn test_uptime_default_no_start_time() {
        let metrics = ServerMetrics::default();
        assert_eq!(metrics.uptime_secs(), 0);
    }

    #[test]
    fn test_snapshot_all_fields() {
        let metrics = ServerMetrics::new();

        // Exercise every counter
        metrics.connection_opened();
        metrics.connection_opened();
        metrics.connection_closed();
        metrics.instance_created();
        metrics.instance_created();
        metrics.instance_created();
        metrics.instance_stopped();
        metrics.message_sent();
        metrics.message_dropped();
        metrics.pty_error();
        metrics.websocket_errors.fetch_add(2, Ordering::Relaxed);
        metrics.messages_received.fetch_add(10, Ordering::Relaxed);
        metrics.focus_switches.fetch_add(3, Ordering::Relaxed);
        metrics.history_replays.fetch_add(1, Ordering::Relaxed);
        metrics
            .history_bytes_sent
            .fetch_add(4096, Ordering::Relaxed);

        let snap = metrics.snapshot();
        assert_eq!(snap.connections.active, 1);
        assert_eq!(snap.connections.total, 2);
        assert_eq!(snap.instances.active, 2);
        assert_eq!(snap.instances.total_created, 3);
        assert_eq!(snap.instances.stopped, 1);
        assert_eq!(snap.messages.received, 10);
        assert_eq!(snap.messages.sent, 1);
        assert_eq!(snap.messages.dropped, 1);
        assert_eq!(snap.errors.pty, 1);
        assert_eq!(snap.errors.websocket, 2);
        assert_eq!(snap.performance.focus_switches, 3);
        assert_eq!(snap.performance.history_replays, 1);
        assert_eq!(snap.performance.history_bytes_sent, 4096);
    }

    #[test]
    fn test_snapshot_serialization() {
        let metrics = ServerMetrics::new();
        metrics.connection_opened();
        let snap = metrics.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: MetricsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.connections.active, 1);
        assert_eq!(parsed.connections.total, 1);
    }

    #[test]
    fn test_health_status_serialization() {
        let status = HealthStatus {
            status: "ok".to_string(),
            instances: InstanceHealth {
                total: 5,
                active: 3,
            },
            connections: 10,
            uptime_secs: 3600,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["instances"]["total"], 5);
        assert_eq!(json["instances"]["active"], 3);
        assert_eq!(json["connections"], 10);
        assert_eq!(json["uptime_secs"], 3600);
    }
}
