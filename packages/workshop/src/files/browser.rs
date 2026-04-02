use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};

use super::types::*;
use crate::AppState;

/// List files in a directory within the instance's working directory
pub async fn list_instance_files(
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
            return Json(DirectoryListing {
                path: query.path,
                entries: vec![],
                error: Some(format!("Invalid working directory: {}", e)),
            })
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
            return Json(DirectoryListing {
                path: query.path,
                entries: vec![],
                error: Some(format!("Path not found: {}", e)),
            })
            .into_response();
        }
    };

    // Security check: target must be within working_dir
    if !canonical_target.starts_with(&canonical_working) {
        return Json(DirectoryListing {
            path: query.path,
            entries: vec![],
            error: Some("Access denied: path outside working directory".to_string()),
        })
        .into_response();
    }

    // Read directory
    let entries = match std::fs::read_dir(&canonical_target) {
        Ok(entries) => entries,
        Err(e) => {
            return Json(DirectoryListing {
                path: query.path,
                entries: vec![],
                error: Some(format!("Cannot read directory: {}", e)),
            })
            .into_response();
        }
    };

    let mut file_entries: Vec<FileEntry> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files (starting with .)
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();

        let symlink_meta = std::fs::symlink_metadata(&path).ok();
        let is_symlink = symlink_meta
            .as_ref()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);

        let symlink_target = if is_symlink {
            std::fs::read_link(&path)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        let metadata = entry.metadata().ok();

        let is_directory = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = metadata
            .as_ref()
            .and_then(|m| if m.is_file() { Some(m.len()) } else { None });
        let modified_at = metadata.and_then(|m| {
            m.modified().ok().map(|t| {
                let datetime: DateTime<Utc> = t.into();
                datetime.to_rfc3339()
            })
        });

        file_entries.push(FileEntry {
            name,
            path: path.to_string_lossy().to_string(),
            is_directory,
            is_symlink,
            symlink_target,
            size,
            modified_at,
        });
    }

    // Sort: directories first, then alphabetically
    file_entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Json(DirectoryListing {
        path: canonical_target.to_string_lossy().to_string(),
        entries: file_entries,
        error: None,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_list_files_instance_not_found() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/instances/{id}/files", get(list_instance_files))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/instances/nonexistent/files?path=.")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_files_success() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;

        // Use a clean subdirectory to avoid test state artifacts
        let project_dir = tmp.path().join("project");
        std::fs::create_dir(&project_dir).unwrap();

        // Create files and a directory
        std::fs::write(project_dir.join("readme.txt"), "hello").unwrap();
        std::fs::write(project_dir.join("main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir(project_dir.join("src")).unwrap();

        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("browser-test".to_string()),
            Some(project_dir.to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route("/instances/{id}/files", get(list_instance_files))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/instances/{}/files?path=.", instance.id))
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
        assert!(json["error"].is_null());

        let entries = json["entries"].as_array().unwrap();
        // Should have 3 entries (readme.txt, main.rs, src/)
        assert_eq!(entries.len(), 3);

        // Directories should come first
        assert!(entries[0]["isDirectory"].as_bool().unwrap());
        assert_eq!(entries[0]["name"], "src");
    }

    #[tokio::test]
    async fn test_list_files_hidden_files_skipped() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;

        let project_dir = tmp.path().join("hidden_project");
        std::fs::create_dir(&project_dir).unwrap();

        std::fs::write(project_dir.join("visible.txt"), "hello").unwrap();
        std::fs::write(project_dir.join(".hidden"), "secret").unwrap();

        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("hidden-test".to_string()),
            Some(project_dir.to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route("/instances/{id}/files", get(list_instance_files))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/instances/{}/files?path=.", instance.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let entries = json["entries"].as_array().unwrap();

        // Only visible.txt should appear
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["name"], "visible.txt");
    }

    #[tokio::test]
    async fn test_list_files_sorting() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;

        let project_dir = tmp.path().join("sort_project");
        std::fs::create_dir(&project_dir).unwrap();

        // Create files and directories
        std::fs::write(project_dir.join("zebra.txt"), "z").unwrap();
        std::fs::write(project_dir.join("alpha.txt"), "a").unwrap();
        std::fs::create_dir(project_dir.join("beta_dir")).unwrap();

        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("sort-test".to_string()),
            Some(project_dir.to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route("/instances/{id}/files", get(list_instance_files))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/instances/{}/files?path=.", instance.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let entries = json["entries"].as_array().unwrap();

        // Directories first, then alphabetical
        assert_eq!(entries[0]["name"], "beta_dir");
        assert!(entries[0]["isDirectory"].as_bool().unwrap());
        assert_eq!(entries[1]["name"], "alpha.txt");
        assert_eq!(entries[2]["name"], "zebra.txt");
    }

    #[tokio::test]
    async fn test_list_files_nonexistent_path() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;

        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("nopath-test".to_string()),
            Some(tmp.path().to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route("/instances/{id}/files", get(list_instance_files))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/instances/{}/files?path=nonexistent_subdir",
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
        // Error should be set
        assert!(json["error"].as_str().is_some());
        assert!(json["entries"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_list_files_file_metadata() {
        let (state, tmp) = crate::test_helpers::test_app_state().await;

        let project_dir = tmp.path().join("meta_project");
        std::fs::create_dir(&project_dir).unwrap();

        let content = "Hello, world!";
        std::fs::write(project_dir.join("test.txt"), content).unwrap();

        let instance = crate::test_helpers::create_test_instance(
            &state.instance_manager,
            Some("meta-test".to_string()),
            Some(project_dir.to_string_lossy().to_string()),
            Some("echo hello".to_string()),
        )
        .await
        .unwrap();

        let app = Router::new()
            .route("/instances/{id}/files", get(list_instance_files))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/instances/{}/files?path=.", instance.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let entries = json["entries"].as_array().unwrap();

        let entry = &entries[0];
        assert_eq!(entry["name"], "test.txt");
        assert_eq!(entry["size"], content.len() as u64);
        assert!(entry["modifiedAt"].as_str().is_some());
        assert!(!entry["isDirectory"].as_bool().unwrap());
        assert!(!entry["isSymlink"].as_bool().unwrap());
    }
}
