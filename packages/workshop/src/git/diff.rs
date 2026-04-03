use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::executor::run_git;
use super::types::*;
use crate::AppState;

// =============================================================================
// Structural diff (syndiff + tree-sitter)
// =============================================================================

/// Map file extension to tree-sitter language.
fn tree_sitter_language_for_path(path: &str) -> Option<tree_sitter::Language> {
    use tree_sitter_language::LanguageFn;

    let ext = path.rsplit('.').next()?.to_lowercase();
    let lang_fn: LanguageFn = match ext.as_str() {
        "rs" => tree_sitter_rust::LANGUAGE,
        "js" | "jsx" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE,
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX,
        "py" | "pyi" => tree_sitter_python::LANGUAGE,
        "go" => tree_sitter_go::LANGUAGE,
        "json" => tree_sitter_json::LANGUAGE,
        "toml" => tree_sitter_toml_ng::LANGUAGE,
        "css" => tree_sitter_css::LANGUAGE,
        "html" | "htm" | "svelte" => tree_sitter_html::LANGUAGE,
        "sh" | "bash" | "zsh" => tree_sitter_bash::LANGUAGE,
        _ => return None,
    };
    Some(lang_fn.into())
}

/// Get old and new source text for a file path in a git repo.
async fn get_file_sources(
    working_dir: &str,
    path: &str,
    commit: Option<&str>,
    base_head: Option<(&str, &str)>,
) -> Option<(String, String)> {
    // Branch-to-branch comparison: use base and head refs directly
    if let Some((base, head)) = base_head {
        let old_ref = format!("{}:{}", base, path);
        let new_ref = format!("{}:{}", head, path);
        let old = run_git(working_dir, &["show", &old_ref])
            .await
            .unwrap_or_default();
        let new = run_git(working_dir, &["show", &new_ref])
            .await
            .unwrap_or_default();
        return Some((old, new));
    }
    match commit {
        Some(c) => {
            let old_ref = format!("{}~1:{}", c, path);
            let new_ref = format!("{}:{}", c, path);
            let old = run_git(working_dir, &["show", &old_ref]).await.ok()?;
            let new = run_git(working_dir, &["show", &new_ref]).await.ok()?;
            Some((old, new))
        }
        None => {
            // Working tree: old = HEAD version, new = filesystem
            let head_ref = format!("HEAD:{}", path);
            let old = run_git(working_dir, &["show", &head_ref]).await.ok()?;
            let abs_path = std::path::Path::new(working_dir).join(path);
            let new = tokio::fs::read_to_string(&abs_path).await.ok()?;
            Some((old, new))
        }
    }
}

/// Build a vec of byte offsets for each line start in `source`.
fn build_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Clip syndiff byte ranges to a single line's span and produce InlineHighlight offsets
/// relative to the line's content start.
fn compute_line_highlights(
    line_content_start: usize,
    line_content_end: usize,
    ranges: &[std::ops::Range<usize>],
) -> Option<Vec<InlineHighlight>> {
    let mut highlights = Vec::new();
    for r in ranges {
        if r.end <= line_content_start || r.start >= line_content_end {
            continue;
        }
        let start = r.start.max(line_content_start) - line_content_start;
        let end = r.end.min(line_content_end) - line_content_start;
        if start < end {
            highlights.push(InlineHighlight { start, end });
        }
    }
    if highlights.is_empty() {
        None
    } else {
        Some(highlights)
    }
}

/// Core: build a GitDiffFile from old/new source + syndiff byte ranges.
fn byte_ranges_to_diff_file(
    path: &str,
    old_src: &str,
    new_src: &str,
    old_ranges: &[std::ops::Range<usize>],
    new_ranges: &[std::ops::Range<usize>],
) -> GitDiffFile {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(old_src, new_src);
    let old_line_starts = build_line_starts(old_src);
    let new_line_starts = build_line_starts(new_src);

    let mut hunks = Vec::new();
    let mut total_additions: i64 = 0;
    let mut total_deletions: i64 = 0;

    for group in diff.grouped_ops(3) {
        let mut hunk_lines = Vec::new();

        let first = &group[0];
        let last = &group[group.len() - 1];
        let old_start = first.old_range().start + 1;
        let old_count = last.old_range().end - first.old_range().start;
        let new_start = first.new_range().start + 1;
        let new_count = last.new_range().end - first.new_range().start;
        let header = format!(
            "@@ -{},{} +{},{} @@",
            old_start, old_count, new_start, new_count
        );

        for op in &group {
            for change in diff.iter_changes(op) {
                let value = change.value();
                let content = value.strip_suffix('\n').unwrap_or(value).to_string();

                match change.tag() {
                    ChangeTag::Equal => {
                        hunk_lines.push(GitDiffLine {
                            line_type: "ctx".to_string(),
                            content,
                            old_num: Some(change.old_index().unwrap() as i64 + 1),
                            new_num: Some(change.new_index().unwrap() as i64 + 1),
                            highlights: None,
                        });
                    }
                    ChangeTag::Delete => {
                        let line_idx = change.old_index().unwrap();
                        let line_byte_start = old_line_starts[line_idx];
                        let line_byte_end = line_byte_start + content.len();
                        let hl =
                            compute_line_highlights(line_byte_start, line_byte_end, old_ranges);

                        hunk_lines.push(GitDiffLine {
                            line_type: "del".to_string(),
                            content,
                            old_num: Some(line_idx as i64 + 1),
                            new_num: None,
                            highlights: hl,
                        });
                        total_deletions += 1;
                    }
                    ChangeTag::Insert => {
                        let line_idx = change.new_index().unwrap();
                        let line_byte_start = new_line_starts[line_idx];
                        let line_byte_end = line_byte_start + content.len();
                        let hl =
                            compute_line_highlights(line_byte_start, line_byte_end, new_ranges);

                        hunk_lines.push(GitDiffLine {
                            line_type: "add".to_string(),
                            content,
                            old_num: None,
                            new_num: Some(line_idx as i64 + 1),
                            highlights: hl,
                        });
                        total_additions += 1;
                    }
                }
            }
        }

        hunks.push(GitDiffHunk {
            header,
            lines: hunk_lines,
        });
    }

    GitDiffFile {
        path: path.to_string(),
        old_path: None,
        status: "modified".to_string(),
        additions: total_additions,
        deletions: total_deletions,
        hunks,
    }
}

/// Orchestrator: attempt structural diff for a single file.
async fn structural_diff_file(
    working_dir: &str,
    path: &str,
    commit: Option<&str>,
    base_head: Option<(&str, &str)>,
) -> Option<GitDiffFile> {
    let ts_lang = tree_sitter_language_for_path(path)?;

    let (old_src, new_src) = get_file_sources(working_dir, path, commit, base_head).await?;

    if old_src.is_empty() || new_src.is_empty() {
        return None;
    }

    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_lang).ok()?;

    let old_tree = parser.parse(&old_src, None)?;
    let new_tree = parser.parse(&new_src, None)?;

    let old_syn = syndiff::build_tree(old_tree.walk(), &old_src);
    let new_syn = syndiff::build_tree(new_tree.walk(), &new_src);

    let opts = syndiff::SyntaxDiffOptions {
        graph_limit: 200_000,
    };
    let (old_ranges, new_ranges) = syndiff::diff_trees(&old_syn, &new_syn, None, None, Some(opts))?;

    if old_ranges.is_empty() && new_ranges.is_empty() {
        return None;
    }

    Some(byte_ranges_to_diff_file(
        path,
        &old_src,
        &new_src,
        &old_ranges,
        &new_ranges,
    ))
}

/// GET /api/instances/{id}/git/diff
pub async fn get_git_diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<GitDiffQuery>,
) -> Response {
    let instance = match state.instance_manager.get(&id).await {
        Some(inst) => inst,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let wd = &instance.working_dir;

    // Structural engine: try syndiff for single-file diffs, fall back to patience
    let requested_structural = query.engine.as_deref() == Some("structural");
    if requested_structural && let Some(ref path) = query.path {
        let base_head = match (&query.base, &query.head) {
            (Some(b), Some(h)) => Some((b.as_str(), h.as_str())),
            _ => None,
        };
        if let Some(file) = structural_diff_file(wd, path, query.commit.as_deref(), base_head).await
        {
            let stats = GitDiffStats {
                additions: file.additions,
                deletions: file.deletions,
                files_changed: 1,
            };
            return Json(GitDiffResponse {
                files: vec![file],
                stats,
                engine: Some("structural".to_string()),
            })
            .into_response();
        }
    }

    let use_patience = query.engine.as_deref() != Some("standard");

    // Branch-to-branch comparison
    if let (Some(base), Some(head)) = (&query.base, &query.head) {
        let is_threedot = query.diff_mode.as_deref() != Some("twodot");
        let range = if is_threedot {
            format!("{}...{}", base, head)
        } else {
            format!("{}..{}", base, head)
        };

        if query.stat_only {
            let args = vec!["diff", "--numstat", range.as_str()];
            let raw = match run_git(wd, &args).await {
                Ok(r) => r,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e })),
                    )
                        .into_response();
                }
            };
            let (files, stats) = parse_numstat(&raw);
            return Json(GitDiffResponse {
                files,
                stats,
                engine: None,
            })
            .into_response();
        }

        let mut args = vec!["diff"];
        if use_patience {
            args.push("--patience");
        }
        args.push(range.as_str());
        if let Some(ref path) = query.path {
            args.push("--");
            args.push(path.as_str());
        }
        let raw_diff = match run_git(wd, &args).await {
            Ok(d) => d,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
                    .into_response();
            }
        };

        let (mut files, stats) = parse_unified_diff(&raw_diff);
        if use_patience {
            for file in &mut files {
                for hunk in &mut file.hunks {
                    add_inline_highlights(&mut hunk.lines);
                }
            }
        }
        let engine = if requested_structural {
            Some("patience".to_string())
        } else {
            None
        };
        return Json(GitDiffResponse {
            files,
            stats,
            engine,
        })
        .into_response();
    }

    // Build the diff command
    let raw_diff = if let Some(ref commit) = query.commit {
        let range = format!("{}~1..{}", commit, commit);
        let mut args = vec!["diff"];
        if use_patience {
            args.push("--patience");
        }
        args.push(range.as_str());
        if let Some(ref path) = query.path {
            args.push("--");
            args.push(path.as_str());
        }
        run_git(wd, &args).await
    } else {
        // Working tree diff (unstaged + staged combined)
        let mut combined = String::new();
        let mut staged_args = vec!["diff", "--cached"];
        if use_patience {
            staged_args.insert(1, "--patience");
        }
        if let Some(ref path) = query.path {
            staged_args.push("--");
            staged_args.push(path.as_str());
        }
        if let Ok(staged) = run_git(wd, &staged_args).await {
            combined.push_str(&staged);
        }
        let mut unstaged_args = vec!["diff"];
        if use_patience {
            unstaged_args.push("--patience");
        }
        if let Some(ref path) = query.path {
            unstaged_args.push("--");
            unstaged_args.push(path.as_str());
        }
        if let Ok(unstaged) = run_git(wd, &unstaged_args).await {
            combined.push_str(&unstaged);
        }
        Ok(combined)
    };

    let diff_text = match raw_diff {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let (mut files, stats) = parse_unified_diff(&diff_text);

    if query.engine.as_deref() != Some("standard") {
        for file in &mut files {
            for hunk in &mut file.hunks {
                add_inline_highlights(&mut hunk.lines);
            }
        }
    }

    let engine = if requested_structural {
        Some("patience".to_string())
    } else {
        None
    };
    Json(GitDiffResponse {
        files,
        stats,
        engine,
    })
    .into_response()
}

/// Parse unified diff output into structured data.
pub fn parse_unified_diff(diff_text: &str) -> (Vec<GitDiffFile>, GitDiffStats) {
    let mut files = Vec::new();
    let mut total_additions: i64 = 0;
    let mut total_deletions: i64 = 0;

    let file_diffs: Vec<&str> = diff_text.split("\ndiff --git ").collect();

    for (i, chunk) in file_diffs.iter().enumerate() {
        let chunk = if i == 0 {
            chunk.strip_prefix("diff --git ").unwrap_or(chunk)
        } else {
            chunk
        };

        if chunk.trim().is_empty() {
            continue;
        }

        let lines: Vec<&str> = chunk.lines().collect();
        if lines.is_empty() {
            continue;
        }

        let header = lines[0];
        let (old_path, new_path) = parse_diff_header_paths(header);

        let mut status = "modified".to_string();
        for line in &lines[1..] {
            if line.starts_with("new file") {
                status = "added".to_string();
            } else if line.starts_with("deleted file") {
                status = "deleted".to_string();
            } else if line.starts_with("rename from") || line.starts_with("similarity index") {
                status = "renamed".to_string();
            } else if line.starts_with("@@") {
                break;
            }
        }

        let mut hunks = Vec::new();
        let mut current_hunk: Option<(String, Vec<GitDiffLine>)> = None;
        let mut file_additions: i64 = 0;
        let mut file_deletions: i64 = 0;
        let mut old_num: i64 = 0;
        let mut new_num: i64 = 0;

        for line in &lines[1..] {
            if line.starts_with("@@") {
                if let Some((header, hunk_lines)) = current_hunk.take() {
                    hunks.push(GitDiffHunk {
                        header,
                        lines: hunk_lines,
                    });
                }

                let hunk_header = line.to_string();
                if let Some((os, ns)) = parse_hunk_header(line) {
                    old_num = os;
                    new_num = ns;
                }
                current_hunk = Some((hunk_header, Vec::new()));
            } else if let Some((_, ref mut hunk_lines)) = current_hunk {
                if let Some(rest) = line.strip_prefix('+') {
                    hunk_lines.push(GitDiffLine {
                        line_type: "add".to_string(),
                        content: rest.to_string(),
                        old_num: None,
                        new_num: Some(new_num),
                        highlights: None,
                    });
                    new_num += 1;
                    file_additions += 1;
                } else if let Some(rest) = line.strip_prefix('-') {
                    hunk_lines.push(GitDiffLine {
                        line_type: "del".to_string(),
                        content: rest.to_string(),
                        old_num: Some(old_num),
                        new_num: None,
                        highlights: None,
                    });
                    old_num += 1;
                    file_deletions += 1;
                } else if let Some(rest) = line.strip_prefix(' ') {
                    hunk_lines.push(GitDiffLine {
                        line_type: "ctx".to_string(),
                        content: rest.to_string(),
                        old_num: Some(old_num),
                        new_num: Some(new_num),
                        highlights: None,
                    });
                    old_num += 1;
                    new_num += 1;
                } else if line.starts_with('\\') {
                    // "\ No newline at end of file" — skip
                }
            }
        }

        if let Some((header, hunk_lines)) = current_hunk.take() {
            hunks.push(GitDiffHunk {
                header,
                lines: hunk_lines,
            });
        }

        total_additions += file_additions;
        total_deletions += file_deletions;

        let display_old = if status == "renamed" || status == "deleted" {
            Some(old_path.clone())
        } else {
            None
        };

        files.push(GitDiffFile {
            path: new_path,
            old_path: display_old,
            status,
            additions: file_additions,
            deletions: file_deletions,
            hunks,
        });
    }

    let stats = GitDiffStats {
        additions: total_additions,
        deletions: total_deletions,
        files_changed: files.len() as i64,
    };

    (files, stats)
}

/// Parse `git diff --numstat` output into structured file stats (no hunks).
pub fn parse_numstat(raw: &str) -> (Vec<GitDiffFile>, GitDiffStats) {
    let mut files = Vec::new();
    let mut total_additions: i64 = 0;
    let mut total_deletions: i64 = 0;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let additions = parts[0].parse::<i64>().unwrap_or(0);
        let deletions = parts[1].parse::<i64>().unwrap_or(0);
        let path = parts[2].to_string();

        let status = if parts[0] == "-" && parts[1] == "-" {
            "binary".to_string()
        } else if additions > 0 && deletions == 0 {
            "added".to_string()
        } else if deletions > 0 && additions == 0 {
            "deleted".to_string()
        } else {
            "modified".to_string()
        };

        total_additions += additions;
        total_deletions += deletions;

        files.push(GitDiffFile {
            path,
            old_path: None,
            status,
            additions,
            deletions,
            hunks: Vec::new(),
        });
    }

    let stats = GitDiffStats {
        additions: total_additions,
        deletions: total_deletions,
        files_changed: files.len() as i64,
    };

    (files, stats)
}

/// Add word-level inline highlights to paired del/add lines in a hunk.
pub fn add_inline_highlights(lines: &mut [GitDiffLine]) {
    use similar::{ChangeTag, TextDiff};

    let len = lines.len();
    let mut i = 0;
    while i < len {
        let del_start = i;
        while i < len && lines[i].line_type == "del" {
            i += 1;
        }
        let del_end = i;

        let add_start = i;
        while i < len && lines[i].line_type == "add" {
            i += 1;
        }
        let add_end = i;

        let del_count = del_end - del_start;
        let add_count = add_end - add_start;

        if del_count == 0 || add_count == 0 || del_count > 8 || add_count > 8 {
            if del_count == 0 && add_count == 0 {
                i += 1;
            }
            continue;
        }

        let pairs = del_count.min(add_count);
        for p in 0..pairs {
            let del_idx = del_start + p;
            let add_idx = add_start + p;
            let old_text = &lines[del_idx].content;
            let new_text = &lines[add_idx].content;

            let diff = TextDiff::from_words(old_text, new_text);

            let mut del_highlights = Vec::new();
            let mut add_highlights = Vec::new();
            let mut old_pos: usize = 0;
            let mut new_pos: usize = 0;

            for change in diff.iter_all_changes() {
                let val = change.value();
                let byte_len = val.len();
                match change.tag() {
                    ChangeTag::Equal => {
                        old_pos += byte_len;
                        new_pos += byte_len;
                    }
                    ChangeTag::Delete => {
                        del_highlights.push(InlineHighlight {
                            start: old_pos,
                            end: old_pos + byte_len,
                        });
                        old_pos += byte_len;
                    }
                    ChangeTag::Insert => {
                        add_highlights.push(InlineHighlight {
                            start: new_pos,
                            end: new_pos + byte_len,
                        });
                        new_pos += byte_len;
                    }
                }
            }

            if !del_highlights.is_empty() {
                lines[del_idx].highlights = Some(del_highlights);
            }
            if !add_highlights.is_empty() {
                lines[add_idx].highlights = Some(add_highlights);
            }
        }
    }
}

/// Parse "a/path b/path" from diff header, stripping a/ and b/ prefixes.
fn parse_diff_header_paths(header: &str) -> (String, String) {
    if let Some(idx) = header.find(" b/") {
        let old = header[..idx].strip_prefix("a/").unwrap_or(&header[..idx]);
        let new = header[idx + 1..]
            .strip_prefix("b/")
            .unwrap_or(&header[idx + 1..]);
        (old.to_string(), new.to_string())
    } else {
        let parts: Vec<&str> = header.splitn(2, ' ').collect();
        let old = parts[0].strip_prefix("a/").unwrap_or(parts[0]);
        let new_p = parts.get(1).unwrap_or(&parts[0]);
        let new_p = new_p.strip_prefix("b/").unwrap_or(new_p);
        (old.to_string(), new_p.to_string())
    }
}

/// Parse @@ -start[,count] +start[,count] @@ returning (old_start, new_start).
fn parse_hunk_header(header: &str) -> Option<(i64, i64)> {
    let header = header.strip_prefix("@@")?;
    let end = header.find("@@")?;
    let range = header[..end].trim();
    let parts: Vec<&str> = range.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let old_start = parts[0]
        .trim_start_matches('-')
        .split(',')
        .next()?
        .parse::<i64>()
        .ok()?;
    let new_start = parts[1]
        .trim_start_matches('+')
        .split(',')
        .next()?
        .parse::<i64>()
        .ok()?;
    Some((old_start, new_start))
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    // =========================================================================
    // build_line_starts
    // =========================================================================

    #[test]
    fn build_line_starts_empty_string() {
        assert_eq!(build_line_starts(""), vec![0]);
    }

    #[test]
    fn build_line_starts_single_line_no_newline() {
        assert_eq!(build_line_starts("hello"), vec![0]);
    }

    #[test]
    fn build_line_starts_single_line_with_newline() {
        assert_eq!(build_line_starts("hello\n"), vec![0, 6]);
    }

    #[test]
    fn build_line_starts_multi_line() {
        // "ab\ncd\nef"
        // offsets: a=0, b=1, \n=2, c=3, d=4, \n=5, e=6, f=7
        assert_eq!(build_line_starts("ab\ncd\nef"), vec![0, 3, 6]);
    }

    #[test]
    fn build_line_starts_trailing_newline() {
        assert_eq!(build_line_starts("ab\ncd\n"), vec![0, 3, 6]);
    }

    #[test]
    fn build_line_starts_crlf() {
        // \r\n — only \n triggers a new line start
        // "a\r\nb" → a=0, \r=1, \n=2, b=3
        let starts = build_line_starts("a\r\nb");
        assert_eq!(starts, vec![0, 3]);
    }

    #[test]
    fn build_line_starts_unicode() {
        // "é\nà" — é is 2 bytes (0xC3 0xA9), \n is byte 2, à starts at byte 3
        let starts = build_line_starts("é\nà");
        assert_eq!(starts, vec![0, 3]);
    }

    #[test]
    fn build_line_starts_consecutive_newlines() {
        assert_eq!(build_line_starts("\n\n\n"), vec![0, 1, 2, 3]);
    }

    // =========================================================================
    // compute_line_highlights
    // =========================================================================

    #[test]
    fn compute_line_highlights_no_overlap() {
        // Range entirely before the line
        let result = compute_line_highlights(10, 20, &[0..5]);
        assert!(result.is_none());
    }

    #[test]
    fn compute_line_highlights_range_after_line() {
        let result = compute_line_highlights(10, 20, &[25..30]);
        assert!(result.is_none());
    }

    #[test]
    fn compute_line_highlights_full_overlap() {
        // Range covers entire line
        let result = compute_line_highlights(10, 20, &[10..20]);
        let hl = result.unwrap();
        assert_eq!(hl.len(), 1);
        assert_eq!(hl[0].start, 0);
        assert_eq!(hl[0].end, 10);
    }

    #[test]
    fn compute_line_highlights_partial_overlap_start() {
        // Range starts before line, ends within
        let result = compute_line_highlights(10, 20, &[5..15]);
        let hl = result.unwrap();
        assert_eq!(hl.len(), 1);
        assert_eq!(hl[0].start, 0);
        assert_eq!(hl[0].end, 5);
    }

    #[test]
    fn compute_line_highlights_partial_overlap_end() {
        // Range starts within line, ends after
        let result = compute_line_highlights(10, 20, &[15..25]);
        let hl = result.unwrap();
        assert_eq!(hl.len(), 1);
        assert_eq!(hl[0].start, 5);
        assert_eq!(hl[0].end, 10);
    }

    #[test]
    fn compute_line_highlights_spanning_boundary() {
        // Range that spans entire line and beyond both sides
        let result = compute_line_highlights(10, 20, &[5..25]);
        let hl = result.unwrap();
        assert_eq!(hl.len(), 1);
        assert_eq!(hl[0].start, 0);
        assert_eq!(hl[0].end, 10);
    }

    #[test]
    fn compute_line_highlights_multiple_ranges() {
        let result = compute_line_highlights(10, 30, &[12..15, 20..25]);
        let hl = result.unwrap();
        assert_eq!(hl.len(), 2);
        assert_eq!(hl[0].start, 2);
        assert_eq!(hl[0].end, 5);
        assert_eq!(hl[1].start, 10);
        assert_eq!(hl[1].end, 15);
    }

    #[test]
    fn compute_line_highlights_empty_ranges_vec() {
        let result = compute_line_highlights(10, 20, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn compute_line_highlights_range_touches_boundary_exactly() {
        // Range ends exactly at line start — should NOT match
        let result = compute_line_highlights(10, 20, &[5..10]);
        assert!(result.is_none());
        // Range starts exactly at line end — should NOT match
        let result = compute_line_highlights(10, 20, &[20..25]);
        assert!(result.is_none());
    }

    // =========================================================================
    // parse_diff_header_paths
    // =========================================================================

    #[test]
    fn parse_diff_header_standard() {
        let (old, new) = parse_diff_header_paths("a/src/main.rs b/src/main.rs");
        assert_eq!(old, "src/main.rs");
        assert_eq!(new, "src/main.rs");
    }

    #[test]
    fn parse_diff_header_nested_dirs() {
        let (old, new) = parse_diff_header_paths("a/src/git/diff.rs b/src/git/diff.rs");
        assert_eq!(old, "src/git/diff.rs");
        assert_eq!(new, "src/git/diff.rs");
    }

    #[test]
    fn parse_diff_header_renamed() {
        let (old, new) = parse_diff_header_paths("a/old_name.rs b/new_name.rs");
        assert_eq!(old, "old_name.rs");
        assert_eq!(new, "new_name.rs");
    }

    #[test]
    fn parse_diff_header_single_segment() {
        let (old, new) = parse_diff_header_paths("a/Cargo.toml b/Cargo.toml");
        assert_eq!(old, "Cargo.toml");
        assert_eq!(new, "Cargo.toml");
    }

    #[test]
    fn parse_diff_header_no_prefix() {
        // When no a/ b/ prefix
        let (old, new) = parse_diff_header_paths("foo.rs bar.rs");
        assert_eq!(old, "foo.rs");
        assert_eq!(new, "bar.rs");
    }

    // =========================================================================
    // parse_hunk_header
    // =========================================================================

    #[test]
    fn parse_hunk_header_standard() {
        let result = parse_hunk_header("@@ -1,3 +1,4 @@");
        assert_eq!(result, Some((1, 1)));
    }

    #[test]
    fn parse_hunk_header_large_numbers() {
        let result = parse_hunk_header("@@ -100,50 +200,60 @@");
        assert_eq!(result, Some((100, 200)));
    }

    #[test]
    fn parse_hunk_header_no_count() {
        // Implied count of 1 — just "-N +N"
        let result = parse_hunk_header("@@ -5 +7 @@");
        assert_eq!(result, Some((5, 7)));
    }

    #[test]
    fn parse_hunk_header_with_context() {
        // Some diffs have function context after the second @@
        let result = parse_hunk_header("@@ -10,6 +10,8 @@ fn main() {");
        assert_eq!(result, Some((10, 10)));
    }

    #[test]
    fn parse_hunk_header_malformed_no_at() {
        let result = parse_hunk_header("not a hunk header");
        assert!(result.is_none());
    }

    #[test]
    fn parse_hunk_header_malformed_single_part() {
        let result = parse_hunk_header("@@ -1 @@");
        assert!(result.is_none());
    }

    #[test]
    fn parse_hunk_header_zero_start() {
        let result = parse_hunk_header("@@ -0,0 +1,5 @@");
        assert_eq!(result, Some((0, 1)));
    }

    // =========================================================================
    // tree_sitter_language_for_path
    // =========================================================================

    #[test]
    fn tree_sitter_known_extensions() {
        // All supported extensions should return Some
        let known = [
            "foo.rs",
            "foo.js",
            "foo.jsx",
            "foo.mjs",
            "foo.cjs",
            "foo.ts",
            "foo.tsx",
            "foo.py",
            "foo.pyi",
            "foo.go",
            "foo.json",
            "foo.toml",
            "foo.css",
            "foo.html",
            "foo.htm",
            "foo.svelte",
            "foo.sh",
            "foo.bash",
            "foo.zsh",
        ];
        for path in &known {
            assert!(
                tree_sitter_language_for_path(path).is_some(),
                "Expected Some for {}",
                path
            );
        }
    }

    #[test]
    fn tree_sitter_unknown_extension() {
        assert!(tree_sitter_language_for_path("foo.xyz").is_none());
        assert!(tree_sitter_language_for_path("foo.md").is_none());
        assert!(tree_sitter_language_for_path("foo.yml").is_none());
    }

    #[test]
    fn tree_sitter_no_extension() {
        assert!(tree_sitter_language_for_path("Makefile").is_none());
    }

    #[test]
    fn tree_sitter_dotfile() {
        // ".bashrc" → extension is "bashrc", not in the list
        assert!(tree_sitter_language_for_path(".bashrc").is_none());
    }

    #[test]
    fn tree_sitter_case_insensitive() {
        // Extension matching is lowercased
        assert!(tree_sitter_language_for_path("FOO.RS").is_some());
        assert!(tree_sitter_language_for_path("bar.Py").is_some());
    }

    #[test]
    fn tree_sitter_nested_path() {
        assert!(tree_sitter_language_for_path("src/git/diff.rs").is_some());
        assert!(tree_sitter_language_for_path("a/b/c/d.ts").is_some());
    }

    // =========================================================================
    // byte_ranges_to_diff_file
    // =========================================================================

    #[test]
    fn byte_ranges_to_diff_file_identical() {
        let src = "hello\nworld\n";
        let result = byte_ranges_to_diff_file("test.rs", src, src, &[], &[]);
        assert_eq!(result.additions, 0);
        assert_eq!(result.deletions, 0);
        assert!(result.hunks.is_empty());
    }

    #[test]
    fn byte_ranges_to_diff_file_pure_addition() {
        let old = "line1\n";
        let new = "line1\nline2\n";
        let result = byte_ranges_to_diff_file("test.rs", old, new, &[], &[]);
        assert_eq!(result.additions, 1);
        assert_eq!(result.deletions, 0);
        assert_eq!(result.path, "test.rs");
        assert_eq!(result.status, "modified");
    }

    #[test]
    fn byte_ranges_to_diff_file_pure_deletion() {
        let old = "line1\nline2\n";
        let new = "line1\n";
        let result = byte_ranges_to_diff_file("test.rs", old, new, &[], &[]);
        assert_eq!(result.additions, 0);
        assert_eq!(result.deletions, 1);
    }

    #[test]
    fn byte_ranges_to_diff_file_modification() {
        let old = "hello\n";
        let new = "world\n";
        let result = byte_ranges_to_diff_file("test.rs", old, new, &[], &[]);
        assert_eq!(result.additions, 1);
        assert_eq!(result.deletions, 1);
        assert_eq!(result.hunks.len(), 1);
    }

    #[test]
    fn byte_ranges_to_diff_file_with_highlights() {
        let old = "hello world\n";
        let new = "hello earth\n";
        // Highlight the changed word in old (bytes 6..11 = "world")
        let old_ranges = vec![6..11usize];
        // Highlight the changed word in new (bytes 6..11 = "earth")
        let new_ranges = vec![6..11usize];
        let result = byte_ranges_to_diff_file("test.rs", old, new, &old_ranges, &new_ranges);

        // The deletion line should have highlights
        let hunk = &result.hunks[0];
        let del_line = hunk.lines.iter().find(|l| l.line_type == "del").unwrap();
        assert!(del_line.highlights.is_some());
        let add_line = hunk.lines.iter().find(|l| l.line_type == "add").unwrap();
        assert!(add_line.highlights.is_some());
    }

    #[test]
    fn byte_ranges_to_diff_file_empty_files() {
        let result = byte_ranges_to_diff_file("test.rs", "", "", &[], &[]);
        assert_eq!(result.additions, 0);
        assert_eq!(result.deletions, 0);
        assert!(result.hunks.is_empty());
    }

    #[test]
    fn byte_ranges_to_diff_file_multi_hunk() {
        // Create content with enough context lines between changes to produce separate hunks
        let mut old_lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut new_lines = old_lines.clone();
        // Change line 0 and line 19 — with default context of 3, these should be in separate hunks
        old_lines[0] = "old first".to_string();
        new_lines[0] = "new first".to_string();
        old_lines[19] = "old last".to_string();
        new_lines[19] = "new last".to_string();

        let old_src = old_lines.join("\n") + "\n";
        let new_src = new_lines.join("\n") + "\n";
        let result = byte_ranges_to_diff_file("test.rs", &old_src, &new_src, &[], &[]);
        assert!(
            result.hunks.len() >= 2,
            "Expected at least 2 hunks, got {}",
            result.hunks.len()
        );
    }

    #[test]
    fn byte_ranges_to_diff_file_unicode_content() {
        let old = "café\n";
        let new = "naïf\n";
        let result = byte_ranges_to_diff_file("test.rs", old, new, &[], &[]);
        assert_eq!(result.additions, 1);
        assert_eq!(result.deletions, 1);
        let hunk = &result.hunks[0];
        let del = hunk.lines.iter().find(|l| l.line_type == "del").unwrap();
        assert_eq!(del.content, "café");
    }

    // =========================================================================
    // parse_unified_diff (public, also worth testing)
    // =========================================================================

    #[test]
    fn parse_unified_diff_empty() {
        let (files, stats) = parse_unified_diff("");
        assert!(files.is_empty());
        assert_eq!(stats.files_changed, 0);
    }

    #[test]
    fn parse_unified_diff_single_file_add() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
new file mode 100644
--- /dev/null
+++ b/foo.rs
@@ -0,0 +1,2 @@
+line 1
+line 2
";
        let (files, stats) = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "added");
        assert_eq!(files[0].path, "foo.rs");
        assert_eq!(stats.additions, 2);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn parse_unified_diff_modification() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,3 @@
 line 1
-old line
+new line
 line 3
";
        let (files, stats) = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "modified");
        assert_eq!(stats.additions, 1);
        assert_eq!(stats.deletions, 1);

        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.lines.len(), 4);
        assert_eq!(hunk.lines[0].line_type, "ctx");
        assert_eq!(hunk.lines[1].line_type, "del");
        assert_eq!(hunk.lines[2].line_type, "add");
        assert_eq!(hunk.lines[3].line_type, "ctx");
    }

    #[test]
    fn parse_unified_diff_multiple_files() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1 +1 @@
-old
+new

diff --git a/bar.rs b/bar.rs
--- a/bar.rs
+++ b/bar.rs
@@ -1 +1,2 @@
 existing
+added
";
        let (files, stats) = parse_unified_diff(diff);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "foo.rs");
        assert_eq!(files[1].path, "bar.rs");
        assert_eq!(stats.files_changed, 2);
        assert_eq!(stats.additions, 2);
        assert_eq!(stats.deletions, 1);
    }

    #[test]
    fn parse_unified_diff_deleted_file() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
deleted file mode 100644
--- a/foo.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-line 1
-line 2
";
        let (files, _stats) = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "deleted");
        assert_eq!(files[0].old_path, Some("foo.rs".to_string()));
    }

    // =========================================================================
    // parse_numstat
    // =========================================================================

    #[test]
    fn parse_numstat_basic() {
        let raw = "3\t1\tsrc/main.rs\n5\t0\tsrc/new.rs\n0\t10\tsrc/old.rs\n";
        let (files, stats) = parse_numstat(raw);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].status, "modified");
        assert_eq!(files[1].status, "added");
        assert_eq!(files[2].status, "deleted");
        assert_eq!(stats.additions, 8);
        assert_eq!(stats.deletions, 11);
    }

    #[test]
    fn parse_numstat_binary() {
        let raw = "-\t-\timage.png\n";
        let (files, _) = parse_numstat(raw);
        assert_eq!(files[0].status, "binary");
    }

    #[test]
    fn parse_numstat_empty() {
        let (files, stats) = parse_numstat("");
        assert!(files.is_empty());
        assert_eq!(stats.files_changed, 0);
    }

    // =========================================================================
    // add_inline_highlights
    // =========================================================================

    #[test]
    fn add_inline_highlights_paired_lines() {
        let mut lines = vec![
            GitDiffLine {
                line_type: "del".to_string(),
                content: "hello world".to_string(),
                old_num: Some(1),
                new_num: None,
                highlights: None,
            },
            GitDiffLine {
                line_type: "add".to_string(),
                content: "hello earth".to_string(),
                old_num: None,
                new_num: Some(1),
                highlights: None,
            },
        ];
        add_inline_highlights(&mut lines);
        assert!(lines[0].highlights.is_some());
        assert!(lines[1].highlights.is_some());
    }

    #[test]
    fn add_inline_highlights_no_pairs() {
        // Only additions, no deletions to pair with
        let mut lines = vec![GitDiffLine {
            line_type: "add".to_string(),
            content: "new line".to_string(),
            old_num: None,
            new_num: Some(1),
            highlights: None,
        }];
        add_inline_highlights(&mut lines);
        assert!(lines[0].highlights.is_none());
    }

    #[test]
    fn add_inline_highlights_too_many_skipped() {
        // More than 8 consecutive del lines — should skip highlighting
        let mut lines: Vec<GitDiffLine> = (0..9)
            .map(|i| GitDiffLine {
                line_type: "del".to_string(),
                content: format!("line {}", i),
                old_num: Some(i + 1),
                new_num: None,
                highlights: None,
            })
            .chain((0..9).map(|i| GitDiffLine {
                line_type: "add".to_string(),
                content: format!("new line {}", i),
                old_num: None,
                new_num: Some(i + 1),
                highlights: None,
            }))
            .collect();
        add_inline_highlights(&mut lines);
        // All should still have None highlights (skipped due to >8)
        for line in &lines {
            assert!(line.highlights.is_none());
        }
    }
}
