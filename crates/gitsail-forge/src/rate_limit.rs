//! Shared "how long until retry" parsing for both forge adapters (T-245/
//! US-103 criterion 2: "rate-limited com tempo de espera se a API
//! informar").
//!
//! Both GitHub and GitLab report a wait time the same two ways: a plain
//! `Retry-After: <seconds>` header (GitHub's secondary rate limit, GitLab's
//! 429), or — GitHub-specific — an `X-RateLimit-Reset: <unix-epoch-seconds>`
//! header on its primary rate limit, with no `Retry-After` at all. This is
//! kept as one pure function (rather than duplicated per adapter) so both
//! forges' wait-time math is tested once, and so a test can pass a fixed
//! `now_unix` instead of racing the real clock (see this module's tests).

use crate::http::HttpResponse;

/// The wait time `response` reports, in seconds, or `None` when it reports
/// none at all (some rate-limit responses genuinely don't say — the caller
/// still reports `RateLimited { retry_after_seconds: None }` rather than
/// inventing a number).
///
/// `now_unix` is the caller's own read of the current time (seconds since
/// the Unix epoch) — passed in, rather than read here, purely so this
/// function stays a pure, directly testable calculation.
pub fn retry_after_seconds(response: &HttpResponse, now_unix: u64) -> Option<u64> {
    if let Some(seconds) = response.header("retry-after").and_then(|v| v.trim().parse::<u64>().ok()) {
        return Some(seconds);
    }
    // GitHub-only fallback: primary rate limit reports only a reset
    // timestamp, no `Retry-After`.
    let reset = response
        .header("x-ratelimit-reset")
        .and_then(|v| v.trim().parse::<u64>().ok())?;
    Some(reset.saturating_sub(now_unix))
}

/// Whether `response` looks like GitHub's rate-limit shape specifically
/// (`X-RateLimit-Remaining: 0`) — used to disambiguate a GitHub 403 between
/// "rate limited" and "insufficient permission" (they share the same HTTP
/// status code; see `github_pr_adapter`'s own doc comment).
pub fn github_rate_limit_exhausted(response: &HttpResponse) -> bool {
    response.header("x-ratelimit-remaining") == Some("0")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::json_response;

    #[test]
    fn prefers_the_retry_after_header_when_present() {
        let response = json_response(429, &[("Retry-After", "42"), ("X-RateLimit-Reset", "999999")], "{}");
        assert_eq!(retry_after_seconds(&response, 1_000_000), Some(42));
    }

    #[test]
    fn falls_back_to_the_rate_limit_reset_header() {
        let response = json_response(403, &[("X-RateLimit-Reset", "1000100")], "{}");
        assert_eq!(retry_after_seconds(&response, 1_000_000), Some(100));
    }

    #[test]
    fn a_reset_time_already_in_the_past_saturates_to_zero_not_a_panic() {
        let response = json_response(403, &[("X-RateLimit-Reset", "500")], "{}");
        assert_eq!(retry_after_seconds(&response, 1_000_000), Some(0));
    }

    #[test]
    fn no_recognized_header_yields_none_never_a_fabricated_wait_time() {
        let response = json_response(429, &[], "{}");
        assert_eq!(retry_after_seconds(&response, 1_000_000), None);
    }

    #[test]
    fn github_rate_limit_exhausted_detects_the_remaining_zero_header() {
        let limited = json_response(403, &[("X-RateLimit-Remaining", "0")], "{}");
        assert!(github_rate_limit_exhausted(&limited));

        let not_limited = json_response(403, &[("X-RateLimit-Remaining", "10")], "{}");
        assert!(!github_rate_limit_exhausted(&not_limited));

        let no_header = json_response(403, &[], "{}");
        assert!(!github_rate_limit_exhausted(&no_header));
    }
}
