//! [`PullRequestQueryPort`] adapter for GitHub's REST API (T-245/US-103).
//!
//! Maps GitHub's `GET /repos/{owner}/{repo}/pulls` response shape to
//! [`PullRequestPage`]/[`PullRequestSummary`], and its status codes/headers
//! to [`PullRequestQueryError`]. This is the only module in the workspace
//! that knows GitHub's specific field names or status-code conventions —
//! `gitsail-application`'s `pull_requests` module stays forge-agnostic.
//!
//! ## Status code mapping (documented decisions)
//! - `401` -> [`PullRequestQueryError::AuthenticationRequired`] (no token,
//!   or an invalid/expired one).
//! - `403` with `X-RateLimit-Remaining: 0` ->
//!   [`PullRequestQueryError::RateLimited`] — GitHub's *primary* rate limit
//!   is reported as a 403, not a 429, so a bare "403 means insufficient
//!   permission" mapping would misreport a plain rate limit as a permission
//!   problem. Any other `403` -> [`PullRequestQueryError::PermissionDenied`]
//!   (a token was presented but lacks the `Pull requests: Read-only` scope,
//!   or GitHub's *secondary* rate limit — which also often includes a
//!   `Retry-After` header on a plain 403; this adapter checks that header
//!   too, before falling through to `PermissionDenied`).
//! - `404` -> [`PullRequestQueryError::PermissionDenied`]. GitHub returns
//!   `404`, not `401`/`403`, for a private repository the caller's
//!   credentials (or lack of any) cannot see — this hides whether the repo
//!   exists at all, but from GitSail's point of view it is exactly the
//!   same "the caller does not currently have access" outcome as a `403`,
//!   so it is deliberately folded into the same UI state rather than
//!   inventing a fourth "maybe it doesn't exist" state this feature has no
//!   good way to act on differently anyway.
//! - `429` -> [`PullRequestQueryError::RateLimited`].
//! - Anything else (5xx, an unexpected 2xx shape, ...) ->
//!   [`PullRequestQueryError::Other`].

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    ForgeRepositoryRef, ForgeToken, PullRequestPage, PullRequestQueryError, PullRequestQueryPort,
    PullRequestState, PullRequestSummary,
};
use gitsail_domain::redact::redact_secrets;
use gitsail_domain::{ErrorCode, GitSailError};
use serde::Deserialize;

use crate::http::{HttpClient, HttpResponse};
use crate::rate_limit::{github_rate_limit_exhausted, retry_after_seconds};

const PER_PAGE: u32 = 30;

/// GitHub's own PR shape, trimmed to exactly the fields US-103 criterion 1
/// needs (title/state/author/branches/url) — this is intentionally not a
/// complete mirror of GitHub's API (no diffs, comments, CI status, labels,
/// reviewers, ... — see `gitsail_application::pull_requests`'s module doc
/// for the full scope-cut list).
#[derive(Debug, Deserialize)]
struct GitHubPull {
    title: String,
    state: String,
    merged_at: Option<String>,
    user: Option<GitHubUser>,
    head: GitHubBranchRef,
    base: GitHubBranchRef,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GitHubBranchRef {
    #[serde(rename = "ref")]
    branch_ref: String,
}

impl From<GitHubPull> for PullRequestSummary {
    fn from(pull: GitHubPull) -> Self {
        let state = if pull.merged_at.is_some() {
            PullRequestState::Merged
        } else if pull.state == "open" {
            PullRequestState::Open
        } else {
            PullRequestState::Closed
        };
        PullRequestSummary {
            title: pull.title,
            state,
            author: pull.user.map(|u| u.login),
            source_branch: Some(pull.head.branch_ref),
            target_branch: Some(pull.base.branch_ref),
            url: pull.html_url,
        }
    }
}

/// Queries GitHub's REST API for one repository's pull requests.
pub struct GitHubPullRequestAdapter {
    http: Arc<dyn HttpClient>,
}

impl GitHubPullRequestAdapter {
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// GitHub's own `Link: <...>; rel="next", <...>; rel="last"` pagination
/// header — `has_next_page` is true exactly when a `rel="next"` entry is
/// present (GitHub never sends this header at all on the last page).
fn has_next_page(response: &HttpResponse) -> bool {
    response
        .header("link")
        .map(|link| link.contains("rel=\"next\""))
        .unwrap_or(false)
}

impl PullRequestQueryPort for GitHubPullRequestAdapter {
    fn list_pull_requests(
        &self,
        repository: &ForgeRepositoryRef,
        page: u32,
        token: Option<&ForgeToken>,
    ) -> Result<PullRequestPage, PullRequestQueryError> {
        let [owner, repo] = repository.path_segments.as_slice() else {
            return Err(PullRequestQueryError::Other(GitSailError::new(
                ErrorCode::Internal,
                "unexpected GitHub remote path shape (expected exactly owner/repo)",
            )));
        };
        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/pulls?state=all&per_page={PER_PAGE}&page={page}"
        );

        // GitHub requires a `User-Agent`; `Accept` pins the stable REST
        // media type explicitly rather than relying on whatever GitHub
        // currently defaults to.
        let mut headers = vec![
            ("Accept", "application/vnd.github+json"),
            ("User-Agent", "gitsail"),
        ];
        let bearer;
        if let Some(token) = token {
            bearer = format!("Bearer {}", token.expose_secret());
            headers.push(("Authorization", &bearer));
        }

        let response = self
            .http
            .get(&url, &headers)
            .map_err(|err| PullRequestQueryError::NetworkFailure(err.message))?;

        match response.status {
            200 => parse_page(&response),
            401 => Err(PullRequestQueryError::AuthenticationRequired),
            403 if github_rate_limit_exhausted(&response) => {
                Err(PullRequestQueryError::RateLimited {
                    retry_after_seconds: retry_after_seconds(&response, now_unix()),
                })
            }
            403 => match retry_after_seconds(&response, now_unix()) {
                // GitHub's secondary rate limit is also a bare 403, often
                // with only `Retry-After` (no `X-RateLimit-*` headers) —
                // see this module's own doc comment.
                Some(seconds) => Err(PullRequestQueryError::RateLimited {
                    retry_after_seconds: Some(seconds),
                }),
                None => Err(PullRequestQueryError::PermissionDenied),
            },
            404 => Err(PullRequestQueryError::PermissionDenied),
            429 => Err(PullRequestQueryError::RateLimited {
                retry_after_seconds: retry_after_seconds(&response, now_unix()),
            }),
            status => Err(PullRequestQueryError::Other(GitSailError::new(
                ErrorCode::NetworkFailure,
                redact_secrets(&format!("GitHub API request failed with status {status}")),
            ))),
        }
    }
}

fn parse_page(response: &HttpResponse) -> Result<PullRequestPage, PullRequestQueryError> {
    let pulls: Vec<GitHubPull> = serde_json::from_str(&response.body).map_err(|err| {
        PullRequestQueryError::Other(
            GitSailError::new(
                ErrorCode::ParseFailure,
                "could not parse GitHub's pull request response",
            )
            .with_source(err),
        )
    })?;
    Ok(PullRequestPage {
        items: pulls.into_iter().map(PullRequestSummary::from).collect(),
        has_next_page: has_next_page(response),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::ForgeKind;

    use crate::http::{json_response, FakeHttpClient};

    fn repository() -> ForgeRepositoryRef {
        ForgeRepositoryRef {
            kind: ForgeKind::GitHub,
            host: "github.com".to_string(),
            path_segments: vec!["org".to_string(), "repo".to_string()],
        }
    }

    fn sample_body() -> &'static str {
        r#"[
            {
                "title": "Fix the thing",
                "state": "open",
                "merged_at": null,
                "user": {"login": "octocat"},
                "head": {"ref": "feature/fix"},
                "base": {"ref": "main"},
                "html_url": "https://github.com/org/repo/pull/1"
            },
            {
                "title": "Old work",
                "state": "closed",
                "merged_at": "2024-01-01T00:00:00Z",
                "user": {"login": "hubot"},
                "head": {"ref": "old-branch"},
                "base": {"ref": "main"},
                "html_url": "https://github.com/org/repo/pull/2"
            },
            {
                "title": "Rejected work",
                "state": "closed",
                "merged_at": null,
                "user": null,
                "head": {"ref": "rejected"},
                "base": {"ref": "main"},
                "html_url": "https://github.com/org/repo/pull/3"
            }
        ]"#
    }

    #[test]
    fn maps_a_successful_page_including_merged_and_closed_and_no_author() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], sample_body())));
        let adapter = GitHubPullRequestAdapter::new(http.clone());

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();

        assert_eq!(page.items.len(), 3);
        assert_eq!(page.items[0].state, PullRequestState::Open);
        assert_eq!(page.items[0].author.as_deref(), Some("octocat"));
        assert_eq!(page.items[0].source_branch.as_deref(), Some("feature/fix"));
        assert_eq!(page.items[0].target_branch.as_deref(), Some("main"));
        assert_eq!(page.items[0].url, "https://github.com/org/repo/pull/1");

        assert_eq!(
            page.items[1].state,
            PullRequestState::Merged,
            "merged_at set wins over state=closed"
        );
        assert_eq!(page.items[2].state, PullRequestState::Closed);
        assert_eq!(
            page.items[2].author, None,
            "a null user must never become an empty string"
        );
        assert!(!page.has_next_page);
    }

    #[test]
    fn a_link_header_with_rel_next_reports_has_next_page() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(
            200,
            &[(
                "Link",
                "<https://api.github.com/repos/org/repo/pulls?page=2>; rel=\"next\"",
            )],
            "[]",
        )));
        let adapter = GitHubPullRequestAdapter::new(http);

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();
        assert!(page.has_next_page);
        assert!(page.items.is_empty());
    }

    #[test]
    fn a_truly_empty_page_has_no_next_page_and_no_items() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], "[]")));
        let adapter = GitHubPullRequestAdapter::new(http);

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();
        assert!(page.items.is_empty());
        assert!(!page.has_next_page);
    }

    #[test]
    fn a_token_is_sent_as_a_bearer_authorization_header() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], "[]")));
        let adapter = GitHubPullRequestAdapter::new(http.clone());

        adapter
            .list_pull_requests(
                &repository(),
                1,
                Some(&ForgeToken::new("sentinel-fake-token")),
            )
            .unwrap();

        let calls = http.calls();
        let (_, headers) = &calls[0];
        assert!(headers.contains(&(
            "Authorization".to_string(),
            "Bearer sentinel-fake-token".to_string()
        )));
    }

    #[test]
    fn no_token_sends_no_authorization_header_still_attempting_public_listing() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], "[]")));
        let adapter = GitHubPullRequestAdapter::new(http.clone());

        adapter.list_pull_requests(&repository(), 1, None).unwrap();

        let calls = http.calls();
        assert!(!calls[0].1.iter().any(|(name, _)| name == "Authorization"));
    }

    #[test]
    fn a_401_maps_to_authentication_required() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(401, &[], "{}")));
        let adapter = GitHubPullRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::AuthenticationRequired
        ));
    }

    #[test]
    fn a_plain_403_without_rate_limit_headers_maps_to_permission_denied() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(403, &[], "{}")));
        let adapter = GitHubPullRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::PermissionDenied
        ));
    }

    #[test]
    fn a_403_with_rate_limit_remaining_zero_maps_to_rate_limited_with_wait_time() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(
            403,
            &[("X-RateLimit-Remaining", "0"), ("Retry-After", "30")],
            "{}",
        )));
        let adapter = GitHubPullRequestAdapter::new(http);

        match adapter
            .list_pull_requests(&repository(), 1, None)
            .unwrap_err()
        {
            PullRequestQueryError::RateLimited {
                retry_after_seconds,
            } => {
                assert_eq!(retry_after_seconds, Some(30));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn a_403_with_only_a_retry_after_header_is_the_secondary_rate_limit_not_permission_denied() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(403, &[("Retry-After", "5")], "{}")));
        let adapter = GitHubPullRequestAdapter::new(http);

        match adapter
            .list_pull_requests(&repository(), 1, None)
            .unwrap_err()
        {
            PullRequestQueryError::RateLimited {
                retry_after_seconds,
            } => {
                assert_eq!(retry_after_seconds, Some(5));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn a_404_maps_to_permission_denied_documented_private_repo_hiding() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(404, &[], "{}")));
        let adapter = GitHubPullRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::PermissionDenied
        ));
    }

    #[test]
    fn a_429_maps_to_rate_limited() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(429, &[("Retry-After", "60")], "{}")));
        let adapter = GitHubPullRequestAdapter::new(http);

        match adapter
            .list_pull_requests(&repository(), 1, None)
            .unwrap_err()
        {
            PullRequestQueryError::RateLimited {
                retry_after_seconds,
            } => {
                assert_eq!(retry_after_seconds, Some(60));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn a_transport_failure_maps_to_network_failure_offline() {
        use crate::http::HttpTransportError;
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Err(HttpTransportError {
            message: "connection refused".to_string(),
        }));
        let adapter = GitHubPullRequestAdapter::new(http);

        match adapter
            .list_pull_requests(&repository(), 1, None)
            .unwrap_err()
        {
            PullRequestQueryError::NetworkFailure(message) => {
                assert_eq!(message, "connection refused")
            }
            other => panic!("expected NetworkFailure, got {other:?}"),
        }
    }

    #[test]
    fn a_server_error_status_maps_to_other_not_a_fabricated_empty_page() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(500, &[], "internal error")));
        let adapter = GitHubPullRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::Other(_)
        ));
    }

    /// US-103 criterion 3: a title/author containing active HTML/Markdown
    /// must round-trip as inert, literal string data — this adapter (and
    /// `gitsail-application`) never interprets it, only a presentation
    /// layer's own escaping (never implemented here) decides how it is
    /// eventually displayed.
    #[test]
    fn malicious_title_and_author_content_survives_as_opaque_untouched_text() {
        let malicious_title = "<script>alert(1)</script> [click me](javascript:alert(1))";
        let malicious_author = "<img src=x onerror=alert(1)>";
        let body = serde_json::json!([{
            "title": malicious_title,
            "state": "open",
            "merged_at": null,
            "user": {"login": malicious_author},
            "head": {"ref": "main"},
            "base": {"ref": "main"},
            "html_url": "https://github.com/org/repo/pull/9",
        }])
        .to_string();

        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], &body)));
        let adapter = GitHubPullRequestAdapter::new(http);

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();
        assert_eq!(
            page.items[0].title, malicious_title,
            "must be preserved verbatim, not interpreted"
        );
        assert_eq!(page.items[0].author.as_deref(), Some(malicious_author));
    }

    #[test]
    fn an_unexpected_remote_path_shape_is_rejected_rather_than_guessing() {
        let repository = ForgeRepositoryRef {
            kind: ForgeKind::GitHub,
            host: "github.com".to_string(),
            path_segments: vec!["only-one-segment".to_string()],
        };
        let http = Arc::new(FakeHttpClient::default());
        let adapter = GitHubPullRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository, 1, None)
                .unwrap_err(),
            PullRequestQueryError::Other(_)
        ));
    }
}
