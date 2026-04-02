use axum::{
    Json,
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::git::executor::run_git;

// --- Types ---

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct BrowseResponse {
    pub path: String,
    pub entries: Vec<BrowseEntry>,
    pub git: Option<GitRepoInfo>,
}

#[derive(Serialize)]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "hasChildren")]
    pub has_children: bool,
}

#[derive(Serialize)]
pub struct GitRepoInfo {
    #[serde(rename = "repoRoot")]
    pub repo_root: String,
    #[serde(rename = "currentBranch")]
    pub current_branch: String,
    pub worktrees: Vec<WorktreeInfo>,
    #[serde(rename = "localBranches")]
    pub local_branches: Vec<String>,
}

#[derive(Serialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    #[serde(rename = "isMain")]
    pub is_main: bool,
}

#[derive(Deserialize)]
pub struct CreateDirectoryRequest {
    pub path: String,
}

#[derive(Serialize)]
pub struct CreateDirectoryResponse {
    pub path: String,
}

#[derive(Deserialize)]
pub struct CreateWorktreeRequest {
    pub repo_path: String,
    pub branch: String,
    pub target_path: String,
    /// If true, create a new branch (git worktree add -b <branch>)
    #[serde(default)]
    pub new_branch: bool,
}

#[derive(Serialize)]
pub struct CreateWorktreeResponse {
    pub path: String,
}

// --- Helpers ---

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn parse_worktree_list(output: &str) -> Vec<WorktreeInfo> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;
    let mut is_first = true;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous entry if any
            if let Some(path) = current_path.take() {
                worktrees.push(WorktreeInfo {
                    path,
                    branch: current_branch.take().unwrap_or_default(),
                    is_main: worktrees.is_empty() && is_first,
                });
                is_first = false;
            }
            current_path = Some(line.strip_prefix("worktree ").unwrap().to_string());
        } else if line.starts_with("branch ") {
            // e.g. "branch refs/heads/main" → "main"
            let refname = line.strip_prefix("branch ").unwrap();
            current_branch = Some(
                refname
                    .strip_prefix("refs/heads/")
                    .unwrap_or(refname)
                    .to_string(),
            );
        } else if line.trim().is_empty() {
            // Block separator — flush current entry
            if let Some(path) = current_path.take() {
                worktrees.push(WorktreeInfo {
                    path,
                    branch: current_branch.take().unwrap_or_default(),
                    is_main: worktrees.is_empty() && is_first,
                });
                is_first = false;
            }
        }
    }

    // Flush last entry (output may not end with blank line)
    if let Some(path) = current_path.take() {
        worktrees.push(WorktreeInfo {
            path,
            branch: current_branch.take().unwrap_or_default(),
            is_main: worktrees.is_empty() && is_first,
        });
    }

    worktrees
}

async fn get_git_info(repo_root: &Path) -> Option<GitRepoInfo> {
    let root_str = repo_root.to_str()?;

    // Run all three git commands concurrently
    let (branch_result, worktree_result, branches_result) = tokio::join!(
        run_git(root_str, &["rev-parse", "--abbrev-ref", "HEAD"]),
        run_git(root_str, &["worktree", "list", "--porcelain"]),
        run_git(root_str, &["branch", "--format=%(refname:short)"]),
    );

    let current_branch = branch_result.ok()?.trim().to_string();

    let worktrees = worktree_result
        .map(|out| parse_worktree_list(&out))
        .unwrap_or_default();

    let local_branches = branches_result
        .map(|out| {
            out.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Some(GitRepoInfo {
        repo_root: root_str.to_string(),
        current_branch,
        worktrees,
        local_branches,
    })
}

// --- Detailed Git Info Types ---

#[derive(Deserialize)]
pub struct GitInfoQuery {
    pub path: String,
}

#[derive(Serialize)]
pub struct GitDetailedInfo {
    #[serde(rename = "repoRoot")]
    pub repo_root: String,
    #[serde(rename = "currentBranch")]
    pub current_branch: String,
    #[serde(rename = "headSha")]
    pub head_sha: String,
    #[serde(rename = "lastCommitSubject")]
    pub last_commit_subject: String,
    #[serde(rename = "lastCommitDate")]
    pub last_commit_date: String,
    pub remotes: Vec<RemoteInfo>,
    pub upstream: Option<UpstreamInfo>,
    pub changes: ChangesSummary,
    #[serde(rename = "stashCount")]
    pub stash_count: u32,
    #[serde(rename = "recentBranches")]
    pub recent_branches: Vec<String>,
}

#[derive(Serialize)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

#[derive(Serialize)]
pub struct UpstreamInfo {
    pub name: String,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Serialize)]
pub struct ChangesSummary {
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
}

// --- Handlers ---

pub async fn git_detailed_info(Query(query): Query<GitInfoQuery>) -> Response {
    let path = PathBuf::from(&query.path);

    if !path.exists() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Path does not exist: {}", query.path),
        )
            .into_response();
    }

    let repo_root = match find_git_root(&path) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Not a git repository: {}", query.path),
            )
                .into_response();
        }
    };

    let root_str = repo_root.to_string_lossy().to_string();

    // Run all git commands concurrently
    let (log_result, remote_result, upstream_result, status_result, stash_result, branches_result) = tokio::join!(
        run_git(&root_str, &["log", "-1", "--format=%H%n%s%n%ai"]),
        run_git(&root_str, &["remote", "-v"]),
        run_git(
            &root_str,
            &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]
        ),
        run_git(
            &root_str,
            &["status", "--porcelain=v2", "--untracked-files=normal"]
        ),
        run_git(&root_str, &["stash", "list"]),
        run_git(
            &root_str,
            &[
                "branch",
                "--sort=-committerdate",
                "--format=%(refname:short)"
            ]
        ),
    );

    // Parse HEAD info
    let (head_sha, last_commit_subject, last_commit_date) = log_result
        .map(|out| {
            let lines: Vec<&str> = out.lines().collect();
            (
                lines.first().unwrap_or(&"").to_string(),
                lines.get(1).unwrap_or(&"").to_string(),
                lines.get(2).unwrap_or(&"").to_string(),
            )
        })
        .unwrap_or_default();

    // Parse current branch
    let current_branch = run_git(&root_str, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Parse remotes — dedupe by name (remote -v shows fetch + push lines)
    let remotes = remote_result
        .map(|out| {
            let mut seen = std::collections::HashSet::new();
            out.lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let name = parts[0].to_string();
                        if seen.insert(name.clone()) {
                            Some(RemoteInfo {
                                name,
                                url: parts[1].to_string(),
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Parse upstream ahead/behind
    let upstream = upstream_result.ok().and_then(|out| {
        let parts: Vec<&str> = out.split_whitespace().collect();
        if parts.len() == 2 {
            Some((
                parts[0].parse::<u32>().unwrap_or(0),
                parts[1].parse::<u32>().unwrap_or(0),
            ))
        } else {
            None
        }
    });

    // Get upstream name (only if we have ahead/behind)
    let upstream = if let Some((ahead, behind)) = upstream {
        let name = run_git(
            &root_str,
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
        )
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| String::new());
        Some(UpstreamInfo {
            name,
            ahead,
            behind,
        })
    } else {
        None
    };

    // Parse status — count staged, modified, untracked
    let changes = status_result
        .map(|out| {
            let mut staged = 0u32;
            let mut modified = 0u32;
            let mut untracked = 0u32;
            for line in out.lines() {
                if line.starts_with("? ") {
                    untracked += 1;
                } else if line.starts_with("1 ") || line.starts_with("2 ") {
                    // porcelain v2: "1 XY ..." or "2 XY ..."
                    // XY: X = staged status, Y = working tree status
                    let chars: Vec<char> = line.chars().collect();
                    if chars.len() >= 4 {
                        let x = chars[2];
                        let y = chars[3];
                        if x != '.' {
                            staged += 1;
                        }
                        if y != '.' {
                            modified += 1;
                        }
                    }
                }
            }
            ChangesSummary {
                staged,
                modified,
                untracked,
            }
        })
        .unwrap_or(ChangesSummary {
            staged: 0,
            modified: 0,
            untracked: 0,
        });

    // Parse stash count
    let stash_count = stash_result
        .map(|out| out.lines().count() as u32)
        .unwrap_or(0);

    // Parse recent branches (take 10)
    let recent_branches = branches_result
        .map(|out| {
            out.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .take(10)
                .collect()
        })
        .unwrap_or_default();

    Json(GitDetailedInfo {
        repo_root: root_str,
        current_branch,
        head_sha,
        last_commit_subject,
        last_commit_date,
        remotes,
        upstream,
        changes,
        stash_count,
        recent_branches,
    })
    .into_response()
}

pub async fn browse_directory(Query(query): Query<BrowseQuery>) -> Response {
    let path = match &query.path {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            // Default to server's working directory
            match std::env::current_dir() {
                Ok(cwd) => cwd,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Cannot determine working directory",
                    )
                        .into_response();
                }
            }
        }
    };

    // Canonicalize the path
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Path does not exist: {}", path.display()),
            )
                .into_response();
        }
    };

    // Read directory entries (directories only, skip hidden)
    let mut entries = Vec::new();
    match std::fs::read_dir(&canonical) {
        Ok(read_dir) => {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden directories
                if name.starts_with('.') {
                    continue;
                }
                // Only include directories
                let file_type = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if file_type.is_dir() || (file_type.is_symlink() && entry.path().is_dir()) {
                    let child_path = entry.path();
                    let has_children = std::fs::read_dir(&child_path)
                        .map(|rd| {
                            rd.flatten().any(|c| {
                                let cn = c.file_name();
                                let cn = cn.to_string_lossy();
                                !cn.starts_with('.')
                                    && c.file_type().is_ok_and(|ft| {
                                        ft.is_dir() || (ft.is_symlink() && c.path().is_dir())
                                    })
                            })
                        })
                        .unwrap_or(false);
                    entries.push(BrowseEntry {
                        path: child_path.to_string_lossy().to_string(),
                        name,
                        has_children,
                    });
                }
            }
        }
        Err(e) => {
            return (
                StatusCode::FORBIDDEN,
                format!("Cannot read directory: {}", e),
            )
                .into_response();
        }
    }

    // Sort alphabetically (case-insensitive)
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Detect git repo
    let git = find_git_root(&canonical);
    let git_info = match git {
        Some(ref root) => get_git_info(root).await,
        None => None,
    };

    Json(BrowseResponse {
        path: canonical.to_string_lossy().to_string(),
        entries,
        git: git_info,
    })
    .into_response()
}

pub async fn create_worktree(Json(req): Json<CreateWorktreeRequest>) -> Response {
    let repo_path = Path::new(&req.repo_path);

    // Validate repo exists and is a git repo
    if !repo_path.exists() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Repository path does not exist: {}", req.repo_path),
        )
            .into_response();
    }
    if !repo_path.join(".git").exists() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Not a git repository: {}", req.repo_path),
        )
            .into_response();
    }

    // Validate target path doesn't already exist
    let target = Path::new(&req.target_path);
    if target.exists() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Target path already exists: {}", req.target_path),
        )
            .into_response();
    }

    // Create the worktree
    let args: Vec<&str> = if req.new_branch {
        // git worktree add -b <new-branch> <path>
        vec!["worktree", "add", "-b", &req.branch, &req.target_path]
    } else {
        // git worktree add <path> <existing-branch>
        vec!["worktree", "add", &req.target_path, &req.branch]
    };
    match run_git(&req.repo_path, &args).await {
        Ok(_) => Json(CreateWorktreeResponse {
            path: req.target_path,
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            format!("Failed to create worktree: {}", e),
        )
            .into_response(),
    }
}

pub async fn create_directory(Json(req): Json<CreateDirectoryRequest>) -> Response {
    let path = Path::new(&req.path);

    if path.exists() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Path already exists: {}", req.path),
        )
            .into_response();
    }

    // Parent must exist
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        return (
            StatusCode::BAD_REQUEST,
            format!("Parent directory does not exist: {}", parent.display()),
        )
            .into_response();
    }

    match std::fs::create_dir(&req.path) {
        Ok(()) => {
            // Canonicalize to return the resolved path
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            Json(CreateDirectoryResponse {
                path: canonical.to_string_lossy().to_string(),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            format!("Failed to create directory: {}", e),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_worktree_list() {
        let output = "worktree /home/user/code/myrepo\nHEAD abc123def\nbranch refs/heads/main\n\nworktree /home/user/code/myrepo-feature\nHEAD def456abc\nbranch refs/heads/feature-x\n\n";
        let worktrees = parse_worktree_list(output);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].path, "/home/user/code/myrepo");
        assert_eq!(worktrees[0].branch, "main");
        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[1].path, "/home/user/code/myrepo-feature");
        assert_eq!(worktrees[1].branch, "feature-x");
        assert!(!worktrees[1].is_main);
    }

    #[test]
    fn test_parse_worktree_list_no_trailing_newline() {
        let output = "worktree /path/to/repo\nHEAD abc123\nbranch refs/heads/main";
        let worktrees = parse_worktree_list(output);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].branch, "main");
        assert!(worktrees[0].is_main);
    }

    #[test]
    fn test_parse_worktree_list_detached_head() {
        let output = "worktree /path/to/repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /path/to/detached\nHEAD def456\ndetached\n\n";
        let worktrees = parse_worktree_list(output);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[1].branch, ""); // detached has no branch
    }

    #[test]
    fn test_find_git_root_not_a_repo() {
        // /tmp is unlikely to be a git repo
        let result = find_git_root(Path::new("/tmp"));
        assert!(result.is_none());
    }
}
