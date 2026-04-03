use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::AppState;
use crate::metrics;

/// Health check endpoint - returns server status
pub async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let instances = state.instance_manager.list().await;
    let active_instances = instances.iter().filter(|i| i.running).count() as u64;
    let metrics = state.metrics.snapshot();

    let status = if metrics.errors.pty == 0 && metrics.errors.websocket == 0 {
        "healthy"
    } else {
        "degraded"
    };

    Json(metrics::HealthStatus {
        status: status.to_string(),
        instances: metrics::InstanceHealth {
            total: instances.len() as u64,
            active: active_instances,
        },
        connections: metrics.connections.active,
        uptime_secs: metrics.uptime_secs,
    })
}

/// Metrics endpoint - returns detailed server metrics
pub async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.metrics.snapshot())
}

/// Liveness probe - returns 200 if the server is running
pub async fn health_live_handler() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "alive" }))
}

/// Readiness probe - returns 200 if the server is ready to accept requests
pub async fn health_ready_handler(State(state): State<AppState>) -> Response {
    let db_ok = state.db.pool.acquire().await.is_ok();

    if db_ok {
        Json(serde_json::json!({
            "status": "ready",
            "database": "connected"
        }))
        .into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "database": "disconnected"
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    async fn test_router() -> (Router, tempfile::TempDir) {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/health", get(health_handler))
            .route("/health/live", get(health_live_handler))
            .route("/health/ready", get(health_ready_handler))
            .route("/metrics", get(metrics_handler))
            .with_state(state);
        (router, tmp)
    }

    #[tokio::test]
    async fn test_health_returns_ok() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn test_health_live() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "alive");
    }

    #[tokio::test]
    async fn test_health_ready() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ready");
        assert_eq!(json["database"], "connected");
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["uptime_secs"].is_number());
        assert!(json["connections"]["active"].is_number());
    }
}
