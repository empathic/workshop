use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct GitCommit {
    pub hash: String,
    #[serde(rename = "shortHash")]
    pub short_hash: String,
    #[serde(rename = "authorName")]
    pub author_name: String,
    #[serde(rename = "authorEmail")]
    pub author_email: String,
    pub date: i64,
    pub message: String,
    pub body: String,
    pub refs: Vec<String>,
}

#[derive(Serialize)]
pub struct GitLogResponse {
    pub commits: Vec<GitCommit>,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

#[derive(Serialize)]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    /// True for remote-tracking branches (refs/remotes/...)
    pub remote: bool,
    #[serde(rename = "lastCommitHash")]
    pub last_commit_hash: String,
    #[serde(rename = "lastCommitDate")]
    pub last_commit_date: i64,
    #[serde(rename = "lastCommitMessage")]
    pub last_commit_message: String,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

#[derive(Serialize)]
pub struct InstanceBranchInfo {
    pub instance_id: String,
    pub instance_name: String,
    pub branch: String,
}

#[derive(Serialize)]
pub struct GitBranchesResponse {
    pub branches: Vec<GitBranch>,
    pub current: String,
    /// The remote's default branch (e.g. "main"), resolved from origin/HEAD.
    #[serde(rename = "defaultBranch", skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    #[serde(rename = "instanceBranches")]
    pub instance_branches: Vec<InstanceBranchInfo>,
}

#[derive(Serialize)]
pub struct GitFileStatus {
    pub path: String,
    pub status: String,
    #[serde(rename = "oldPath", skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

#[derive(Serialize)]
pub struct GitStatusResponse {
    pub branch: String,
    pub staged: Vec<GitFileStatus>,
    pub unstaged: Vec<GitFileStatus>,
    pub untracked: Vec<GitFileStatus>,
    #[serde(rename = "aheadBehind")]
    pub ahead_behind: Option<(i64, i64)>,
}

#[derive(Serialize)]
pub struct InlineHighlight {
    pub start: usize,
    pub end: usize,
}

#[derive(Serialize)]
pub struct GitDiffLine {
    #[serde(rename = "type")]
    pub line_type: String,
    pub content: String,
    #[serde(rename = "oldNum", skip_serializing_if = "Option::is_none")]
    pub old_num: Option<i64>,
    #[serde(rename = "newNum", skip_serializing_if = "Option::is_none")]
    pub new_num: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<Vec<InlineHighlight>>,
}

#[derive(Serialize)]
pub struct GitDiffHunk {
    pub header: String,
    pub lines: Vec<GitDiffLine>,
}

#[derive(Serialize)]
pub struct GitDiffFile {
    pub path: String,
    #[serde(rename = "oldPath", skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
    pub hunks: Vec<GitDiffHunk>,
}

#[derive(Serialize)]
pub struct GitDiffStats {
    pub additions: i64,
    pub deletions: i64,
    #[serde(rename = "filesChanged")]
    pub files_changed: i64,
}

#[derive(Serialize)]
pub struct GitDiffResponse {
    pub files: Vec<GitDiffFile>,
    pub stats: GitDiffStats,
    /// Which engine actually produced the diff: "structural", "patience", or "standard".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
}

#[derive(Deserialize)]
pub struct GitLogQuery {
    #[serde(default = "default_git_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub branch: Option<String>,
}

fn default_git_limit() -> i64 {
    50
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_git_limit_is_50() {
        assert_eq!(default_git_limit(), 50);
    }

    #[test]
    fn git_log_query_defaults() {
        let q: GitLogQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
        assert!(q.branch.is_none());
    }

    #[test]
    fn git_log_query_with_values() {
        let q: GitLogQuery =
            serde_json::from_str(r#"{"limit": 10, "offset": 5, "branch": "main"}"#).unwrap();
        assert_eq!(q.limit, 10);
        assert_eq!(q.offset, 5);
        assert_eq!(q.branch.as_deref(), Some("main"));
    }

    #[test]
    fn git_commit_serialization() {
        let commit = GitCommit {
            hash: "abc123".to_string(),
            short_hash: "abc".to_string(),
            author_name: "Alice".to_string(),
            author_email: "alice@example.com".to_string(),
            date: 1700000000,
            message: "Initial commit".to_string(),
            body: "".to_string(),
            refs: vec!["HEAD".to_string(), "main".to_string()],
        };
        let json = serde_json::to_value(&commit).unwrap();
        assert_eq!(json["shortHash"], "abc");
        assert_eq!(json["authorName"], "Alice");
        assert_eq!(json["authorEmail"], "alice@example.com");
        assert_eq!(json["refs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn git_branch_serialization() {
        let branch = GitBranch {
            name: "main".to_string(),
            current: true,
            remote: false,
            last_commit_hash: "abc123".to_string(),
            last_commit_date: 1700000000,
            last_commit_message: "init".to_string(),
            upstream: Some("origin/main".to_string()),
            ahead: 2,
            behind: 0,
        };
        let json = serde_json::to_value(&branch).unwrap();
        assert_eq!(json["lastCommitHash"], "abc123");
        assert_eq!(json["lastCommitDate"], 1700000000);
        assert_eq!(json["lastCommitMessage"], "init");
        assert!(json["current"].as_bool().unwrap());
        assert!(!json["remote"].as_bool().unwrap());
    }

    #[test]
    fn git_status_response_serialization() {
        let status = GitStatusResponse {
            branch: "main".to_string(),
            staged: vec![],
            unstaged: vec![GitFileStatus {
                path: "foo.rs".to_string(),
                status: "modified".to_string(),
                old_path: None,
            }],
            untracked: vec![],
            ahead_behind: Some((1, 0)),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["branch"], "main");
        assert_eq!(json["unstaged"].as_array().unwrap().len(), 1);
        assert_eq!(json["aheadBehind"][0], 1);
        assert_eq!(json["aheadBehind"][1], 0);
    }

    #[test]
    fn git_file_status_old_path_skipped_when_none() {
        let s = GitFileStatus {
            path: "bar.rs".to_string(),
            status: "added".to_string(),
            old_path: None,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("oldPath").is_none());
    }

    #[test]
    fn git_file_status_old_path_present_when_some() {
        let s = GitFileStatus {
            path: "bar.rs".to_string(),
            status: "renamed".to_string(),
            old_path: Some("baz.rs".to_string()),
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["oldPath"], "baz.rs");
    }

    #[test]
    fn git_diff_stats_serialization() {
        let stats = GitDiffStats {
            additions: 10,
            deletions: 5,
            files_changed: 3,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["filesChanged"], 3);
    }

    #[test]
    fn git_diff_response_engine_skipped_when_none() {
        let resp = GitDiffResponse {
            files: vec![],
            stats: GitDiffStats {
                additions: 0,
                deletions: 0,
                files_changed: 0,
            },
            engine: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("engine").is_none());
    }

    #[test]
    fn git_branches_response_serialization() {
        let resp = GitBranchesResponse {
            branches: vec![],
            current: "main".to_string(),
            default_branch: Some("main".to_string()),
            instance_branches: vec![InstanceBranchInfo {
                instance_id: "inst-1".to_string(),
                instance_name: "my-inst".to_string(),
                branch: "feature".to_string(),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["defaultBranch"], "main");
        assert_eq!(json["instanceBranches"][0]["instance_id"], "inst-1");
    }

    #[test]
    fn git_log_response_serialization() {
        let resp = GitLogResponse {
            commits: vec![],
            has_more: true,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["hasMore"].as_bool().unwrap());
    }

    #[test]
    fn inline_highlight_serialization() {
        let h = InlineHighlight { start: 0, end: 10 };
        let json = serde_json::to_value(&h).unwrap();
        assert_eq!(json["start"], 0);
        assert_eq!(json["end"], 10);
    }

    #[test]
    fn git_diff_line_serialization() {
        let line = GitDiffLine {
            line_type: "added".to_string(),
            content: "+hello".to_string(),
            old_num: None,
            new_num: Some(5),
            highlights: None,
        };
        let json = serde_json::to_value(&line).unwrap();
        assert_eq!(json["type"], "added");
        assert!(json.get("oldNum").is_none());
        assert_eq!(json["newNum"], 5);
        assert!(json.get("highlights").is_none());
    }

    #[test]
    fn git_diff_hunk_serialization() {
        let hunk = GitDiffHunk {
            header: "@@ -1,3 +1,4 @@".to_string(),
            lines: vec![],
        };
        let json = serde_json::to_value(&hunk).unwrap();
        assert_eq!(json["header"], "@@ -1,3 +1,4 @@");
    }

    #[test]
    fn git_diff_file_serialization() {
        let file = GitDiffFile {
            path: "src/main.rs".to_string(),
            old_path: None,
            status: "modified".to_string(),
            additions: 10,
            deletions: 3,
            hunks: vec![],
        };
        let json = serde_json::to_value(&file).unwrap();
        assert!(json.get("oldPath").is_none());
        assert_eq!(json["additions"], 10);
    }
}

#[derive(Deserialize)]
pub struct GitDiffQuery {
    pub commit: Option<String>,
    pub path: Option<String>,
    /// Diff engine: "structural", "patience", or "standard".
    pub engine: Option<String>,
    /// Base ref for branch-to-branch comparison (e.g. "main")
    pub base: Option<String>,
    /// Head ref for branch-to-branch comparison (e.g. "feature-branch")
    pub head: Option<String>,
    /// "twodot" or "threedot" (default). Controls `..` vs `...` range syntax.
    pub diff_mode: Option<String>,
    /// When true, return only file stats (no hunks) via --numstat
    #[serde(default)]
    pub stat_only: bool,
}
