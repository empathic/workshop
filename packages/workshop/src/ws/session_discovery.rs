//! Session Discovery
//!
//! Functions for finding and selecting Claude sessions for instances.

use chrono::{DateTime, Utc};
use toolpath_convo::{ConversationMeta, ConversationProvider};
use tracing::{debug, warn};

/// Find candidate sessions that could belong to this instance
pub fn find_candidate_sessions(
    provider: &dyn ConversationProvider,
    working_dir: &str,
    created_at: DateTime<Utc>,
) -> Vec<ConversationMeta> {
    match provider.list_metadata(working_dir) {
        Ok(metadata) => {
            debug!(
                "find_candidate_sessions: list_metadata returned {} entries for {} (filtering >= {})",
                metadata.len(),
                working_dir,
                created_at
            );
            metadata
                .into_iter()
                .filter(|m| m.started_at.map(|s| s >= created_at).unwrap_or(false))
                .collect()
        }
        Err(e) => {
            warn!(
                "find_candidate_sessions: list_metadata failed for {}: {}",
                working_dir, e
            );
            vec![]
        }
    }
}

/// Pick the best candidate from multiple options.
///
/// Selects the candidate whose `started_at` is closest to `search_after`
/// by absolute time distance. This is almost always the session created
/// for this specific instance.
#[cfg(test)]
fn pick_best_candidate(
    candidates: &[ConversationMeta],
    search_after: DateTime<Utc>,
) -> Option<&ConversationMeta> {
    candidates
        .iter()
        .filter_map(|c| c.started_at.map(|s| (c, s)))
        .min_by_key(|(_, started)| {
            let diff = *started - search_after;
            diff.num_milliseconds().unsigned_abs()
        })
        .map(|(c, _)| c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use toolpath_convo::{ConversationView, ConvoError};

    /// Mock provider that returns a fixed list of metadata.
    struct MockProvider {
        metadata: Vec<ConversationMeta>,
    }

    impl ConversationProvider for MockProvider {
        fn list_conversations(&self, _project: &str) -> toolpath_convo::Result<Vec<String>> {
            Ok(self.metadata.iter().map(|m| m.id.clone()).collect())
        }

        fn load_conversation(
            &self,
            _project: &str,
            _conversation_id: &str,
        ) -> toolpath_convo::Result<ConversationView> {
            Err(ConvoError::Provider("not implemented".into()))
        }

        fn load_metadata(
            &self,
            _project: &str,
            _conversation_id: &str,
        ) -> toolpath_convo::Result<ConversationMeta> {
            Err(ConvoError::Provider("not implemented".into()))
        }

        fn list_metadata(&self, _project: &str) -> toolpath_convo::Result<Vec<ConversationMeta>> {
            Ok(self.metadata.clone())
        }
    }

    /// Mock provider that always errors.
    struct ErrorProvider;

    impl ConversationProvider for ErrorProvider {
        fn list_conversations(&self, _: &str) -> toolpath_convo::Result<Vec<String>> {
            Err(ConvoError::Provider("boom".into()))
        }
        fn load_conversation(&self, _: &str, _: &str) -> toolpath_convo::Result<ConversationView> {
            Err(ConvoError::Provider("boom".into()))
        }
        fn load_metadata(&self, _: &str, _: &str) -> toolpath_convo::Result<ConversationMeta> {
            Err(ConvoError::Provider("boom".into()))
        }
        fn list_metadata(&self, _: &str) -> toolpath_convo::Result<Vec<ConversationMeta>> {
            Err(ConvoError::Provider("boom".into()))
        }
    }

    fn meta(id: &str, started_at: Option<DateTime<Utc>>) -> ConversationMeta {
        ConversationMeta {
            id: id.to_string(),
            started_at,
            last_activity: None,
            message_count: 1,
            file_path: None,
            predecessor: None,
            successor: None,
        }
    }

    #[test]
    fn filters_by_created_at() {
        let t1 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 5).unwrap();
        let t3 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 10).unwrap();

        let provider = MockProvider {
            metadata: vec![meta("old-session", Some(t1)), meta("new-session", Some(t3))],
        };

        // created_at = t2, so only t3 (>= t2) should match
        let results = find_candidate_sessions(&provider, "/test", t2);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "new-session");
    }

    #[test]
    fn exact_timestamp_match_included() {
        let t = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();

        let provider = MockProvider {
            metadata: vec![meta("exact", Some(t))],
        };

        let results = find_candidate_sessions(&provider, "/test", t);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "exact");
    }

    #[test]
    fn none_started_at_excluded() {
        let t = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();

        let provider = MockProvider {
            metadata: vec![meta("no-timestamp", None)],
        };

        let results = find_candidate_sessions(&provider, "/test", t);
        assert!(results.is_empty());
    }

    #[test]
    fn provider_error_returns_empty() {
        let t = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let results = find_candidate_sessions(&ErrorProvider, "/test", t);
        assert!(results.is_empty());
    }

    #[test]
    fn empty_metadata_returns_empty() {
        let t = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let provider = MockProvider { metadata: vec![] };
        let results = find_candidate_sessions(&provider, "/test", t);
        assert!(results.is_empty());
    }

    #[test]
    fn returns_convo_meta_id_field() {
        // Regression: old code used .session_id, new uses .id
        let t = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let provider = MockProvider {
            metadata: vec![meta("abc-123-def", Some(t))],
        };
        let results = find_candidate_sessions(&provider, "/test", t);
        assert_eq!(results[0].id, "abc-123-def");
    }

    // --- pick_best_candidate tests ---

    #[test]
    fn pick_best_closest_to_search_after() {
        let search = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 1).unwrap(); // 1s after
        let t2 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 10).unwrap(); // 10s after

        let candidates = vec![meta("far", Some(t2)), meta("close", Some(t1))];
        let best = pick_best_candidate(&candidates, search).unwrap();
        assert_eq!(best.id, "close");
    }

    #[test]
    fn pick_best_single_candidate() {
        let search = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let t = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 5).unwrap();

        let candidates = vec![meta("only", Some(t))];
        let best = pick_best_candidate(&candidates, search).unwrap();
        assert_eq!(best.id, "only");
    }

    #[test]
    fn pick_best_empty_returns_none() {
        let search = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        assert!(pick_best_candidate(&[], search).is_none());
    }

    #[test]
    fn pick_best_skips_none_started_at() {
        let search = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let t = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 2).unwrap();

        let candidates = vec![meta("no-time", None), meta("has-time", Some(t))];
        let best = pick_best_candidate(&candidates, search).unwrap();
        assert_eq!(best.id, "has-time");
    }

    #[test]
    fn pick_best_all_none_started_at_returns_none() {
        let search = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let candidates = vec![meta("a", None), meta("b", None)];
        assert!(pick_best_candidate(&candidates, search).is_none());
    }

    // --- Multi-instance race scenario tests ---
    //
    // These simulate the core bug: two instances in the same working directory,
    // where Instance A could steal Instance B's session.

    /// Simulates the discovery + claimed-session filter that the watcher does.
    /// Returns unclaimed candidates for a given instance.
    fn discover_unclaimed(
        provider: &dyn ConversationProvider,
        working_dir: &str,
        search_after: DateTime<Utc>,
        claimed: &std::collections::HashSet<String>,
    ) -> Vec<ConversationMeta> {
        find_candidate_sessions(provider, working_dir, search_after)
            .into_iter()
            .filter(|c| !claimed.contains(&c.id))
            .collect()
    }

    #[test]
    fn two_instances_claimed_filter_prevents_stealing() {
        // Instance A created at T1, Instance B created at T2.
        // Session S_B created at T3 (belongs to B).
        // A discovers first — but B has already claimed S_B.
        let t1 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap(); // A created
        let t3 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 5).unwrap(); // S_B created

        let provider = MockProvider {
            metadata: vec![meta("session-b", Some(t3))],
        };

        // B claims its session first
        let mut claimed = std::collections::HashSet::new();
        claimed.insert("session-b".to_string());

        // A tries to discover — session-b is claimed, so A gets nothing
        let a_candidates = discover_unclaimed(&provider, "/test", t1, &claimed);
        assert!(
            a_candidates.is_empty(),
            "A must not see B's claimed session"
        );
    }

    #[test]
    fn two_instances_each_claims_own_session() {
        // Both instances have sessions. Each should claim its own.
        let t1 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap(); // A created
        let t2 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 1).unwrap(); // B created
        let t3 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 2).unwrap(); // S_A created
        let t4 = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 10).unwrap(); // S_B created

        let provider = MockProvider {
            metadata: vec![meta("session-a", Some(t3)), meta("session-b", Some(t4))],
        };

        let no_claims = std::collections::HashSet::new();

        // A discovers: sees both sessions (both >= T1). Picks closest to A's input.
        let a_input_at = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 1).unwrap();
        let a_candidates = discover_unclaimed(&provider, "/test", t1, &no_claims);
        let a_pick = pick_best_candidate(&a_candidates, a_input_at).unwrap();
        assert_eq!(
            a_pick.id, "session-a",
            "A should pick session closest to its input"
        );

        // A claims session-a
        let mut claimed = std::collections::HashSet::new();
        claimed.insert("session-a".to_string());

        // B discovers: session-a is claimed, only session-b remains
        let b_candidates = discover_unclaimed(&provider, "/test", t2, &claimed);
        assert_eq!(b_candidates.len(), 1);
        assert_eq!(b_candidates[0].id, "session-b");
    }

    #[test]
    fn without_first_input_gate_instance_a_would_steal() {
        // This test documents the bug: without the first_input_at gate,
        // Instance A (created earlier, no input) would find Instance B's
        // session because search_after=created_at(A) < session.started_at.
        let t_a_created = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
        let t_b_created = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 1).unwrap();
        let t_session_b = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 5).unwrap();

        let provider = MockProvider {
            metadata: vec![meta("session-b", Some(t_session_b))],
        };
        let no_claims = std::collections::HashSet::new();

        // BUG: A has no input, but if it uses created_at as search_after,
        // it finds B's session (t_session_b >= t_a_created).
        let a_candidates = discover_unclaimed(&provider, "/test", t_a_created, &no_claims);
        assert_eq!(
            a_candidates.len(),
            1,
            "Without the gate, A WOULD see B's session — this is the bug"
        );

        // FIX: The watcher skips discovery entirely when first_input_at is None.
        // This test can't exercise the async watcher gate, but documents why
        // the gate is necessary: the filter alone is insufficient.

        // B discovers correctly using its own created_at
        let b_candidates = discover_unclaimed(&provider, "/test", t_b_created, &no_claims);
        assert_eq!(b_candidates.len(), 1);
        assert_eq!(b_candidates[0].id, "session-b");
    }
}
