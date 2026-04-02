use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::executor::run_git;
use super::types::*;
use crate::AppState;

/// GET /api/instances/{id}/git/branches
pub async fn get_git_branches(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let instance = match state.instance_manager.get(&id).await {
        Some(inst) => inst,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let wd = &instance.working_dir;

    // Get current branch
    let current = match run_git(wd, &["rev-parse", "--abbrev-ref", "HEAD"]).await {
        Ok(s) => s.trim().to_string(),
        Err(_) => String::new(),
    };

    // Get branch list with details
    let format = "%(HEAD)%00%(refname:short)%00%(objectname:short)%00%(committerdate:unix)%00%(subject)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(refname)";
    let format_arg = format!("--format={}", format);
    let output = match run_git(wd, &["branch", "-a", &format_arg]).await {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let branches = parse_branch_list(&output);

    // Get instance branches — which branch is each instance on?
    let all_instances = state.instance_manager.list().await;
    let mut instance_branches = Vec::new();
    for inst in &all_instances {
        if inst.working_dir == instance.working_dir
            && let Ok(branch) =
                run_git(&inst.working_dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await
        {
            instance_branches.push(InstanceBranchInfo {
                instance_id: inst.id.clone(),
                instance_name: inst
                    .custom_name
                    .clone()
                    .unwrap_or_else(|| inst.name.clone()),
                branch: branch.trim().to_string(),
            });
        }
    }

    // Resolve the remote's default branch from origin/HEAD
    let default_branch = run_git(wd, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        .await
        .ok()
        .and_then(|s| {
            let s = s.trim();
            // "refs/remotes/origin/main" → "main"
            s.strip_prefix("refs/remotes/origin/")
                .map(|b| b.to_string())
        });

    Json(GitBranchesResponse {
        branches,
        current,
        default_branch,
        instance_branches,
    })
    .into_response()
}

/// Parse `git branch -a --format=...` output into structured branch data.
///
/// Expected NUL-separated fields per line:
/// `%(HEAD)\0%(refname:short)\0%(objectname:short)\0%(committerdate:unix)\0%(subject)\0%(upstream:short)\0%(upstream:track,nobracket)\0%(refname)`
pub fn parse_branch_list(output: &str) -> Vec<GitBranch> {
    let mut branches = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.splitn(8, '\0').collect();
        if fields.len() < 5 {
            continue;
        }

        let is_current = fields[0].trim() == "*";
        let name = fields[1].trim().to_string();
        let hash = fields[2].trim().to_string();
        let date = fields[3].trim().parse::<i64>().unwrap_or(0);
        let message = fields[4].trim().to_string();
        let upstream = fields.get(5).and_then(|s| {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        });
        let track = fields.get(6).unwrap_or(&"").trim();
        let full_refname = fields.get(7).unwrap_or(&"").trim();
        let is_remote = full_refname.starts_with("refs/remotes/");

        let (ahead, behind) = parse_ahead_behind(track);

        branches.push(GitBranch {
            name,
            current: is_current,
            remote: is_remote,
            last_commit_hash: hash,
            last_commit_date: date,
            last_commit_message: message,
            upstream,
            ahead,
            behind,
        });
    }
    branches
}

pub fn parse_ahead_behind(track: &str) -> (i64, i64) {
    let mut ahead = 0i64;
    let mut behind = 0i64;
    for part in track.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_ahead_behind ──────────────────────────────────────────────

    #[test]
    fn test_ahead_behind_both() {
        assert_eq!(parse_ahead_behind("ahead 2, behind 3"), (2, 3));
    }

    #[test]
    fn test_ahead_only() {
        assert_eq!(parse_ahead_behind("ahead 5"), (5, 0));
    }

    #[test]
    fn test_behind_only() {
        assert_eq!(parse_ahead_behind("behind 7"), (0, 7));
    }

    #[test]
    fn test_empty_track() {
        assert_eq!(parse_ahead_behind(""), (0, 0));
    }

    #[test]
    fn test_gone_track() {
        // When upstream is deleted, git shows "gone"
        assert_eq!(parse_ahead_behind("gone"), (0, 0));
    }

    #[test]
    fn test_ahead_behind_large_numbers() {
        assert_eq!(parse_ahead_behind("ahead 1234, behind 5678"), (1234, 5678));
    }

    // ── parse_branch_list ───────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn make_branch_line(
        head: &str,
        name: &str,
        hash: &str,
        date: &str,
        msg: &str,
        upstream: &str,
        track: &str,
        refname: &str,
    ) -> String {
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            head, name, hash, date, msg, upstream, track, refname
        )
    }

    #[test]
    fn test_parse_empty_branch_list() {
        assert!(parse_branch_list("").is_empty());
    }

    #[test]
    fn test_parse_single_local_branch() {
        let output = make_branch_line(
            "*",
            "main",
            "abc123",
            "1700000000",
            "Initial commit",
            "origin/main",
            "ahead 1",
            "refs/heads/main",
        );
        let branches = parse_branch_list(&output);
        assert_eq!(branches.len(), 1);
        let b = &branches[0];
        assert_eq!(b.name, "main");
        assert!(b.current);
        assert!(!b.remote);
        assert_eq!(b.last_commit_hash, "abc123");
        assert_eq!(b.last_commit_date, 1700000000);
        assert_eq!(b.last_commit_message, "Initial commit");
        assert_eq!(b.upstream.as_deref(), Some("origin/main"));
        assert_eq!(b.ahead, 1);
        assert_eq!(b.behind, 0);
    }

    #[test]
    fn test_parse_remote_branch() {
        let output = make_branch_line(
            " ",
            "origin/main",
            "abc123",
            "1700000000",
            "Latest",
            "",
            "",
            "refs/remotes/origin/main",
        );
        let branches = parse_branch_list(&output);
        assert_eq!(branches.len(), 1);
        assert!(branches[0].remote);
        assert!(!branches[0].current);
        assert!(branches[0].upstream.is_none());
    }

    #[test]
    fn test_parse_non_current_branch() {
        let output = make_branch_line(
            " ",
            "feature",
            "def456",
            "1700001000",
            "Add feature",
            "",
            "",
            "refs/heads/feature",
        );
        let branches = parse_branch_list(&output);
        assert!(!branches[0].current);
    }

    #[test]
    fn test_parse_multiple_branches() {
        let mut output = make_branch_line(
            "*",
            "main",
            "aaa",
            "100",
            "First",
            "origin/main",
            "",
            "refs/heads/main",
        );
        output += "\n";
        output += &make_branch_line(
            " ",
            "develop",
            "bbb",
            "200",
            "Second",
            "origin/develop",
            "behind 2",
            "refs/heads/develop",
        );
        output += "\n";
        output += &make_branch_line(
            " ",
            "origin/main",
            "aaa",
            "100",
            "First",
            "",
            "",
            "refs/remotes/origin/main",
        );

        let branches = parse_branch_list(&output);
        assert_eq!(branches.len(), 3);
        assert!(branches[0].current);
        assert_eq!(branches[1].behind, 2);
        assert!(branches[2].remote);
    }

    #[test]
    fn test_parse_branch_no_upstream() {
        let output = make_branch_line(
            " ",
            "local-only",
            "abc",
            "100",
            "Msg",
            "",
            "",
            "refs/heads/local-only",
        );
        let branches = parse_branch_list(&output);
        assert!(branches[0].upstream.is_none());
    }

    #[test]
    fn test_skips_malformed_lines() {
        let output = "just\0two\0fields\n";
        let branches = parse_branch_list(output);
        assert!(branches.is_empty());
    }

    #[test]
    fn test_skips_empty_lines() {
        let mut output = String::from("\n\n");
        output += &make_branch_line("*", "main", "abc", "100", "Msg", "", "", "refs/heads/main");
        output += "\n\n";
        let branches = parse_branch_list(&output);
        assert_eq!(branches.len(), 1);
    }
}
