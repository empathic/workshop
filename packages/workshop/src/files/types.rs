use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "isDirectory")]
    pub is_directory: bool,
    #[serde(rename = "isSymlink")]
    pub is_symlink: bool,
    #[serde(rename = "symlinkTarget", skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
    pub size: Option<u64>,
    #[serde(rename = "modifiedAt")]
    pub modified_at: Option<String>,
}

#[derive(Serialize)]
pub struct DirectoryListing {
    pub path: String,
    pub entries: Vec<FileEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct FilePathQuery {
    pub path: String,
}

#[derive(Deserialize)]
pub struct FileSearchQuery {
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    100
}

#[derive(Serialize)]
pub struct FileSearchResult {
    pub name: String,
    pub path: String,
    #[serde(rename = "relativePath")]
    pub relative_path: String,
    #[serde(rename = "isDirectory")]
    pub is_directory: bool,
    pub score: i32,
}

#[derive(Serialize)]
pub struct FileSearchResponse {
    pub query: String,
    pub results: Vec<FileSearchResult>,
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_search_limit_is_100() {
        assert_eq!(default_search_limit(), 100);
    }

    #[test]
    fn file_search_query_defaults() {
        let q: FileSearchQuery = serde_json::from_str(r#"{"q": "hello"}"#).unwrap();
        assert_eq!(q.q, "hello");
        assert_eq!(q.limit, 100);
    }

    #[test]
    fn file_search_query_with_limit() {
        let q: FileSearchQuery = serde_json::from_str(r#"{"q": "hello", "limit": 50}"#).unwrap();
        assert_eq!(q.limit, 50);
    }

    #[test]
    fn file_entry_serialization() {
        let entry = FileEntry {
            name: "test.rs".to_string(),
            path: "/src/test.rs".to_string(),
            is_directory: false,
            is_symlink: false,
            symlink_target: None,
            size: Some(1024),
            modified_at: Some("2024-01-01".to_string()),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["name"], "test.rs");
        assert_eq!(json["isDirectory"], false);
        assert_eq!(json["isSymlink"], false);
        assert!(json.get("symlinkTarget").is_none());
        assert_eq!(json["size"], 1024);
        assert_eq!(json["modifiedAt"], "2024-01-01");
    }

    #[test]
    fn file_entry_symlink_target_present() {
        let entry = FileEntry {
            name: "link".to_string(),
            path: "/link".to_string(),
            is_directory: false,
            is_symlink: true,
            symlink_target: Some("/target".to_string()),
            size: None,
            modified_at: None,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["symlinkTarget"], "/target");
        assert!(json["isSymlink"].as_bool().unwrap());
    }

    #[test]
    fn directory_listing_serialization() {
        let listing = DirectoryListing {
            path: "/src".to_string(),
            entries: vec![],
            error: None,
        };
        let json = serde_json::to_value(&listing).unwrap();
        assert_eq!(json["path"], "/src");
        assert!(json["entries"].as_array().unwrap().is_empty());
        assert!(json.get("error").is_none());
    }

    #[test]
    fn directory_listing_with_error() {
        let listing = DirectoryListing {
            path: "/nonexistent".to_string(),
            entries: vec![],
            error: Some("not found".to_string()),
        };
        let json = serde_json::to_value(&listing).unwrap();
        assert_eq!(json["error"], "not found");
    }

    #[test]
    fn file_path_query_deserialization() {
        let q: FilePathQuery = serde_json::from_str(r#"{"path": "/src/main.rs"}"#).unwrap();
        assert_eq!(q.path, "/src/main.rs");
    }

    #[test]
    fn file_search_result_serialization() {
        let result = FileSearchResult {
            name: "main.rs".to_string(),
            path: "/abs/src/main.rs".to_string(),
            relative_path: "src/main.rs".to_string(),
            is_directory: false,
            score: 95,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["relativePath"], "src/main.rs");
        assert_eq!(json["score"], 95);
    }

    #[test]
    fn file_search_response_serialization() {
        let resp = FileSearchResponse {
            query: "main".to_string(),
            results: vec![],
            truncated: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["query"], "main");
        assert!(!json["truncated"].as_bool().unwrap());
    }
}
