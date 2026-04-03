use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::executor::run_git;
use super::types::*;
use crate::AppState;

/// GET /api/instances/{id}/git/status
pub async fn get_git_status(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let instance = match state.instance_manager.get(&id).await {
        Some(inst) => inst,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let wd = &instance.working_dir;
    let output = match run_git(wd, &["status", "--porcelain=v2", "--branch"]).await {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    Json(parse_porcelain_status(&output)).into_response()
}

/// Parse `git status --porcelain=v2 --branch` output into structured data.
pub fn parse_porcelain_status(output: &str) -> GitStatusResponse {
    let mut branch = String::new();
    let mut ahead_behind: Option<(i64, i64)> = None;
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();

    for line in output.lines() {
        if let Some(name) = line.strip_prefix("# branch.head ") {
            branch = name.to_string();
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            // Parse "# branch.ab +N -M"
            let parts: Vec<&str> = ab.split_whitespace().collect();
            if parts.len() == 2 {
                let a = parts[0].trim_start_matches('+').parse::<i64>().unwrap_or(0);
                let b = parts[1].trim_start_matches('-').parse::<i64>().unwrap_or(0);
                ahead_behind = Some((a, b));
            }
        } else if let Some(path) = line.strip_prefix("? ") {
            // Untracked
            let path = path.to_string();
            untracked.push(GitFileStatus {
                path,
                status: "untracked".to_string(),
                old_path: None,
            });
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // Changed entry: "1 XY ..." or rename "2 XY ..."
            let is_rename = line.starts_with("2 ");
            let parts: Vec<&str> = line.splitn(if is_rename { 10 } else { 9 }, ' ').collect();
            if parts.len() < 2 {
                continue;
            }
            let xy = parts[1];
            let x = xy.chars().next().unwrap_or('.');
            let y = xy.chars().nth(1).unwrap_or('.');

            let file_path = parts.last().unwrap_or(&"").to_string();

            let old_path = if is_rename {
                let last = *parts.last().unwrap_or(&"");
                let rename_parts: Vec<&str> = last.splitn(2, '\t').collect();
                if rename_parts.len() == 2 {
                    Some(rename_parts[1].to_string())
                } else {
                    None
                }
            } else {
                None
            };

            // X = staged status
            if x != '.' && x != '?' {
                staged.push(GitFileStatus {
                    path: file_path.clone(),
                    status: porcelain_status_to_string(x),
                    old_path: old_path.clone(),
                });
            }
            // Y = unstaged (working tree) status
            if y != '.' && y != '?' {
                unstaged.push(GitFileStatus {
                    path: file_path,
                    status: porcelain_status_to_string(y),
                    old_path,
                });
            }
        }
    }

    GitStatusResponse {
        branch,
        staged,
        unstaged,
        untracked,
        ahead_behind,
    }
}

pub fn porcelain_status_to_string(c: char) -> String {
    match c {
        'M' => "modified".to_string(),
        'A' => "added".to_string(),
        'D' => "deleted".to_string(),
        'R' => "renamed".to_string(),
        'C' => "copied".to_string(),
        'T' => "type_changed".to_string(),
        'U' => "unmerged".to_string(),
        _ => format!("unknown({})", c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_porcelain_status_to_string() {
        assert_eq!(porcelain_status_to_string('M'), "modified");
        assert_eq!(porcelain_status_to_string('A'), "added");
        assert_eq!(porcelain_status_to_string('D'), "deleted");
        assert_eq!(porcelain_status_to_string('R'), "renamed");
        assert_eq!(porcelain_status_to_string('C'), "copied");
        assert_eq!(porcelain_status_to_string('T'), "type_changed");
        assert_eq!(porcelain_status_to_string('U'), "unmerged");
        assert_eq!(porcelain_status_to_string('X'), "unknown(X)");
    }

    #[test]
    fn test_parse_empty_output() {
        let result = parse_porcelain_status("");
        assert!(result.branch.is_empty());
        assert!(result.staged.is_empty());
        assert!(result.unstaged.is_empty());
        assert!(result.untracked.is_empty());
        assert!(result.ahead_behind.is_none());
    }

    #[test]
    fn test_parse_branch_head() {
        let output = "# branch.head main\n# branch.oid abc123\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.branch, "main");
    }

    #[test]
    fn test_parse_ahead_behind() {
        let output = "# branch.head feature\n# branch.ab +3 -1\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.ahead_behind, Some((3, 1)));
    }

    #[test]
    fn test_parse_ahead_only() {
        let output = "# branch.ab +5 -0\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.ahead_behind, Some((5, 0)));
    }

    #[test]
    fn test_parse_untracked_files() {
        let output = "? newfile.rs\n? src/other.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.untracked.len(), 2);
        assert_eq!(result.untracked[0].path, "newfile.rs");
        assert_eq!(result.untracked[0].status, "untracked");
        assert_eq!(result.untracked[1].path, "src/other.rs");
    }

    #[test]
    fn test_parse_staged_modified() {
        // Porcelain v2: "1 XY sub mH mI mW hH hI path"
        let output = "1 M. N... 100644 100644 100644 abc123 def456 src/main.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.staged.len(), 1);
        assert_eq!(result.staged[0].path, "src/main.rs");
        assert_eq!(result.staged[0].status, "modified");
        assert!(result.unstaged.is_empty());
    }

    #[test]
    fn test_parse_unstaged_modified() {
        let output = "1 .M N... 100644 100644 100644 abc123 def456 src/lib.rs\n";
        let result = parse_porcelain_status(output);
        assert!(result.staged.is_empty());
        assert_eq!(result.unstaged.len(), 1);
        assert_eq!(result.unstaged[0].path, "src/lib.rs");
        assert_eq!(result.unstaged[0].status, "modified");
    }

    #[test]
    fn test_parse_both_staged_and_unstaged() {
        let output = "1 MM N... 100644 100644 100644 abc123 def456 src/both.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.staged.len(), 1);
        assert_eq!(result.unstaged.len(), 1);
        assert_eq!(result.staged[0].path, "src/both.rs");
        assert_eq!(result.unstaged[0].path, "src/both.rs");
    }

    #[test]
    fn test_parse_staged_added() {
        let output = "1 A. N... 000000 100644 100644 0000000 abc123 new_file.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.staged.len(), 1);
        assert_eq!(result.staged[0].status, "added");
    }

    #[test]
    fn test_parse_staged_deleted() {
        let output = "1 D. N... 100644 000000 000000 abc123 0000000 old_file.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.staged.len(), 1);
        assert_eq!(result.staged[0].status, "deleted");
    }

    #[test]
    fn test_parse_full_status() {
        let output = "\
# branch.oid abc123def456
# branch.head feature/cool-thing
# branch.upstream origin/feature/cool-thing
# branch.ab +2 -1
1 M. N... 100644 100644 100644 abc123 def456 src/main.rs
1 .M N... 100644 100644 100644 111111 222222 src/lib.rs
1 A. N... 000000 100644 100644 0000000 333333 src/new.rs
? untracked.txt
? .env.local
";
        let result = parse_porcelain_status(output);
        assert_eq!(result.branch, "feature/cool-thing");
        assert_eq!(result.ahead_behind, Some((2, 1)));
        assert_eq!(result.staged.len(), 2); // M. and A.
        assert_eq!(result.unstaged.len(), 1); // .M
        assert_eq!(result.untracked.len(), 2);
    }

    #[test]
    fn test_parse_detached_head() {
        let output = "# branch.head (detached)\n# branch.oid abc123\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.branch, "(detached)");
        assert!(result.ahead_behind.is_none());
    }
}
