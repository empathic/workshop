use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::executor::run_git;
use super::types::*;
use crate::AppState;

/// GET /api/instances/{id}/git/log
pub async fn get_git_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<GitLogQuery>,
) -> Response {
    let instance = match state.instance_manager.get(&id).await {
        Some(inst) => inst,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let limit = query.limit.min(200);
    let fetch_count = (limit + 1).to_string();
    let offset_str = query.offset.to_string();

    // Use NUL as field separator and record separator \x1e between body and refs
    let format = "%H%x00%h%x00%an%x00%ae%x00%at%x00%s%x00%b%x1e%D%x1f";
    let format_arg = format!("--format={}", format);
    let count_arg = format!("--max-count={}", fetch_count);
    let skip_arg = format!("--skip={}", offset_str);
    let mut args = vec![
        "log",
        format_arg.as_str(),
        count_arg.as_str(),
        skip_arg.as_str(),
    ];

    if let Some(ref branch) = query.branch {
        args.push(branch.as_str());
    }

    let output = match run_git(&instance.working_dir, &args).await {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    Json(parse_git_log_output(&output, limit)).into_response()
}

/// Parse the custom-formatted git log output into structured commits.
///
/// Format per commit: `%H\0%h\0%an\0%ae\0%at\0%s\0%b\x1e%D\x1f`
/// Records are separated by `\x1f`, body/refs by `\x1e`, fields by `\0`.
pub fn parse_git_log_output(output: &str, limit: i64) -> GitLogResponse {
    let mut commits = Vec::new();
    // Each record ends with \x1f (unit separator)
    for record in output.split('\x1f') {
        let record = record.trim();
        if record.is_empty() {
            continue;
        }

        // Split on \x1e to separate body+fields from refs
        let parts: Vec<&str> = record.splitn(2, '\x1e').collect();
        let fields_part = parts[0];
        let refs_str = parts.get(1).unwrap_or(&"");

        let fields: Vec<&str> = fields_part.splitn(7, '\0').collect();
        if fields.len() < 6 {
            continue;
        }

        let refs: Vec<String> = refs_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        commits.push(GitCommit {
            hash: fields[0].to_string(),
            short_hash: fields[1].to_string(),
            author_name: fields[2].to_string(),
            author_email: fields[3].to_string(),
            date: fields[4].parse::<i64>().unwrap_or(0),
            message: fields[5].to_string(),
            body: fields.get(6).unwrap_or(&"").trim().to_string(),
            refs,
        });
    }

    let has_more = commits.len() as i64 > limit;
    commits.truncate(limit as usize);

    GitLogResponse { commits, has_more }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn make_record(
        hash: &str,
        short: &str,
        name: &str,
        email: &str,
        ts: &str,
        msg: &str,
        body: &str,
        refs: &str,
    ) -> String {
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\x1e{}\x1f",
            hash, short, name, email, ts, msg, body, refs
        )
    }

    #[test]
    fn test_parse_empty_output() {
        let result = parse_git_log_output("", 50);
        assert!(result.commits.is_empty());
        assert!(!result.has_more);
    }

    #[test]
    fn test_parse_single_commit() {
        let output = make_record(
            "abc123def456",
            "abc123",
            "Alice",
            "alice@example.com",
            "1700000000",
            "Fix the thing",
            "Detailed body here",
            "HEAD -> main",
        );
        let result = parse_git_log_output(&output, 50);
        assert_eq!(result.commits.len(), 1);
        let c = &result.commits[0];
        assert_eq!(c.hash, "abc123def456");
        assert_eq!(c.short_hash, "abc123");
        assert_eq!(c.author_name, "Alice");
        assert_eq!(c.author_email, "alice@example.com");
        assert_eq!(c.date, 1700000000);
        assert_eq!(c.message, "Fix the thing");
        assert_eq!(c.body, "Detailed body here");
        assert_eq!(c.refs, vec!["HEAD -> main"]);
        assert!(!result.has_more);
    }

    #[test]
    fn test_parse_multiple_commits() {
        let mut output = make_record("aaa", "aa", "Alice", "a@x", "100", "First", "", "");
        output += &make_record("bbb", "bb", "Bob", "b@x", "200", "Second", "", "");
        output += &make_record("ccc", "cc", "Carol", "c@x", "300", "Third", "", "");

        let result = parse_git_log_output(&output, 50);
        assert_eq!(result.commits.len(), 3);
        assert_eq!(result.commits[0].message, "First");
        assert_eq!(result.commits[2].message, "Third");
        assert!(!result.has_more);
    }

    #[test]
    fn test_has_more_when_exceeds_limit() {
        let mut output = String::new();
        for i in 0..4 {
            output += &make_record(
                &format!("hash{i}"),
                &format!("h{i}"),
                "A",
                "a@x",
                "100",
                &format!("msg{i}"),
                "",
                "",
            );
        }

        let result = parse_git_log_output(&output, 3);
        assert_eq!(result.commits.len(), 3);
        assert!(result.has_more);
    }

    #[test]
    fn test_no_has_more_at_exact_limit() {
        let mut output = String::new();
        for i in 0..3 {
            output += &make_record(
                &format!("hash{i}"),
                &format!("h{i}"),
                "A",
                "a@x",
                "100",
                &format!("msg{i}"),
                "",
                "",
            );
        }

        let result = parse_git_log_output(&output, 3);
        assert_eq!(result.commits.len(), 3);
        assert!(!result.has_more);
    }

    #[test]
    fn test_parse_multiple_refs() {
        let output = make_record(
            "aaa",
            "aa",
            "Alice",
            "a@x",
            "100",
            "Merge",
            "",
            "HEAD -> main, origin/main, tag: v1.0",
        );
        let result = parse_git_log_output(&output, 50);
        assert_eq!(
            result.commits[0].refs,
            vec!["HEAD -> main", "origin/main", "tag: v1.0"]
        );
    }

    #[test]
    fn test_parse_no_refs() {
        let output = make_record("aaa", "aa", "Alice", "a@x", "100", "Commit", "", "");
        let result = parse_git_log_output(&output, 50);
        assert!(result.commits[0].refs.is_empty());
    }

    #[test]
    fn test_parse_invalid_date() {
        let output = make_record(
            "aaa",
            "aa",
            "Alice",
            "a@x",
            "not_a_number",
            "Commit",
            "",
            "",
        );
        let result = parse_git_log_output(&output, 50);
        assert_eq!(result.commits[0].date, 0);
    }

    #[test]
    fn test_parse_empty_body() {
        let output = make_record("aaa", "aa", "Alice", "a@x", "100", "Commit", "", "");
        let result = parse_git_log_output(&output, 50);
        assert_eq!(result.commits[0].body, "");
    }

    #[test]
    fn test_parse_multiline_body() {
        let output = make_record(
            "aaa",
            "aa",
            "Alice",
            "a@x",
            "100",
            "Commit",
            "Line 1\nLine 2\nLine 3",
            "",
        );
        let result = parse_git_log_output(&output, 50);
        assert!(result.commits[0].body.contains("Line 1"));
        assert!(result.commits[0].body.contains("Line 3"));
    }

    #[test]
    fn test_skips_malformed_records() {
        // A record with too few fields (only 3 NUL-separated parts)
        let bad = "aaa\0bb\0cc\x1erefs\x1f";
        let good = make_record("ddd", "dd", "Dave", "d@x", "100", "Good", "", "");
        let output = format!("{bad}{good}");
        let result = parse_git_log_output(&output, 50);
        assert_eq!(result.commits.len(), 1);
        assert_eq!(result.commits[0].hash, "ddd");
    }
}
