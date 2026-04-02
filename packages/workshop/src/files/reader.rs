use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::types::FilePathQuery;
use crate::AppState;

/// Security check: returns true if `target` is within `working_dir` after canonicalization.
#[cfg(test)]
fn is_path_within(working_dir: &std::path::Path, target: &std::path::Path) -> bool {
    target.starts_with(working_dir)
}

/// Get the content of a file within the instance's working directory
pub async fn get_instance_file_content(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Response {
    let instance = match state.instance_manager.get(&id).await {
        Some(inst) => inst,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let working_dir = std::path::Path::new(&instance.working_dir);
    let requested_path = std::path::Path::new(&query.path);

    // Security: ensure the requested path is within working_dir
    let canonical_working = match working_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid working directory: {}", e) })),
            )
                .into_response();
        }
    };

    let target_path = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        working_dir.join(requested_path)
    };

    let canonical_target = match target_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("File not found: {}", e) })),
            )
                .into_response();
        }
    };

    // Security check: target must be within working_dir
    if !canonical_target.starts_with(&canonical_working) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Access denied: symlink points outside project directory" })),
        )
            .into_response();
    }

    if canonical_target.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Path is a directory, not a file" })),
        )
            .into_response();
    }

    // Check file size (limit to 1MB to prevent memory issues)
    const MAX_FILE_SIZE: u64 = 1024 * 1024; // 1MB
    if let Ok(metadata) = std::fs::metadata(&canonical_target)
        && metadata.len() > MAX_FILE_SIZE
    {
        return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("File too large ({}MB). Max size is 1MB.", metadata.len() / (1024 * 1024))
                })),
            )
                .into_response();
    }

    match std::fs::read_to_string(&canonical_target) {
        Ok(content) => Json(serde_json::json!({ "content": content })).into_response(),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::InvalidData {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "File is not valid UTF-8 text" })),
                )
                    .into_response();
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Cannot read file: {}", e) })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    async fn test_reader_router() -> (Router, tempfile::TempDir) {
        let (state, tmp) = crate::test_helpers::test_app_state().await;

        // Create a real instance pointing to our temp dir
        crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("reader-test".to_string()),
            Some(tmp.path().to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let router = Router::new()
            .route(
                "/instances/{id}/files/content",
                get(get_instance_file_content),
            )
            .with_state(state);
        (router, tmp)
    }

    #[test]
    fn is_path_within_valid() {
        let base = std::path::Path::new("/tmp/project");
        assert!(is_path_within(
            base,
            std::path::Path::new("/tmp/project/src/main.rs")
        ));
        assert!(is_path_within(base, std::path::Path::new("/tmp/project")));
    }

    #[test]
    fn is_path_within_escape() {
        let base = std::path::Path::new("/tmp/project");
        assert!(!is_path_within(base, std::path::Path::new("/tmp/other")));
        assert!(!is_path_within(base, std::path::Path::new("/etc/passwd")));
    }

    #[tokio::test]
    async fn test_read_file_instance_not_found() {
        let (app, _tmp) = test_reader_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/instances/nonexistent/files/content?path=test.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_read_file_success() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;

        // Create a file in the temp dir
        let file_path = tmp.path().join("hello.txt");
        std::fs::write(&file_path, "Hello, world!").unwrap();

        // Create instance
        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("read-test".to_string()),
            Some(tmp.path().to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route(
                "/instances/{id}/files/content",
                get(get_instance_file_content),
            )
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/instances/{}/files/content?path=hello.txt",
                        instance.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["content"], "Hello, world!");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("notfound-test".to_string()),
            Some(tmp.path().to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route(
                "/instances/{id}/files/content",
                get(get_instance_file_content),
            )
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/instances/{}/files/content?path=nonexistent.txt",
                        instance.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_read_directory_returns_bad_request() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        // Create a subdirectory
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("dir-test".to_string()),
            Some(tmp.path().to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route(
                "/instances/{id}/files/content",
                get(get_instance_file_content),
            )
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/instances/{}/files/content?path=subdir",
                        instance.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_read_file_too_large() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        // Create a file larger than 1MB
        let large_content = "x".repeat(1024 * 1024 + 1);
        std::fs::write(tmp.path().join("large.txt"), &large_content).unwrap();

        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("large-test".to_string()),
            Some(tmp.path().to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route(
                "/instances/{id}/files/content",
                get(get_instance_file_content),
            )
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/instances/{}/files/content?path=large.txt",
                        instance.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
