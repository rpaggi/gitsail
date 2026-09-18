//! [`PullRequestQueryPort`] adapter for GitLab's REST API (v4) (T-245/
//! US-103).
//!
//! Maps GitLab's `GET /projects/:id/merge_requests` response shape to
//! [`PullRequestPage`]/[`PullRequestSummary`], and its status codes/headers
//! to [`PullRequestQueryError`]. `:id` is the URL-encoded full namespace
//! path (`group%2Fsubgroup%2Frepo`) — GitLab's own documented way to
//! address a project without first resolving it to a numeric id, which
//! matters here since this adapter never has (and never needs) that
//! numeric id, only what [`gitsail_domain::forge::repository_location`]
//! already resolved from the remote URL.
//!
//! Unlike GitHub, a GitLab remote's host is not fixed (self-hosted GitLab
//! is explicitly supported — see `gitsail_domain::forge`'s own doc
//! comment), so every request URL here is built from
//! [`ForgeRepositoryRef::host`], never a hardcoded `gitlab.com`.
//!
//! Token header: GitLab's REST API expects a personal/project access token
//! in a `PRIVATE-TOKEN` header (not `Authorization: Bearer`, which GitLab
//! reserves for OAuth2 tokens) — T-244/US-102's module docs already commit
//! to a PAT with the `read_api` scope for v1.0, so `PRIVATE-TOKEN` is the
//! correct header for the only kind of token this workspace ever stores.
//!
//! ## Status code mapping (documented decisions)
//! - `401` -> [`PullRequestQueryError::AuthenticationRequired`].
//! - `403` -> [`PullRequestQueryError::PermissionDenied`] (GitLab does not
//!   overload 403 for rate limiting the way GitHub does; GitLab's rate
//!   limit is always a plain `429`).
//! - `404` -> [`PullRequestQueryError::PermissionDenied`], the same
//!   documented "hidden means denied" decision `github_pr_adapter`'s own
//!   doc comment explains for GitHub — GitLab equally returns `404` (not
//!   `401`/`403`) for a private project the caller cannot see.
//! - `429` -> [`PullRequestQueryError::RateLimited`], using the `Retry-After`
//!   header GitLab sends on it when it sends one.
//! - Anything else -> [`PullRequestQueryError::Other`].

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
use crate::rate_limit::retry_after_seconds;

const PER_PAGE: u32 = 30;

/// GitLab's own MR shape, trimmed to exactly what US-103 criterion 1 needs
/// (see `github_pr_adapter`'s matching struct for the same scope note).
#[derive(Debug, Deserialize)]
struct GitLabMergeRequest {
    title: String,
    state: String,
    author: Option<GitLabAuthor>,
    source_branch: String,
    target_branch: String,
    web_url: String,
}

#[derive(Debug, Deserialize)]
struct GitLabAuthor {
    username: String,
}

impl From<GitLabMergeRequest> for PullRequestSummary {
    fn from(mr: GitLabMergeRequest) -> Self {
        let state = match mr.state.as_str() {
            "opened" => PullRequestState::Open,
            "merged" => PullRequestState::Merged,
            // "closed" and the rare transitional "locked" (mid-merge) both
            // map to Closed — see `gitsail_application::pull_requests`'s
            // `PullRequestState` doc comment for why a fourth UI state for
            // "locked" was judged not worth it.
            _ => PullRequestState::Closed,
        };
        PullRequestSummary {
            title: mr.title,
            state,
            author: mr.author.map(|a| a.username),
            source_branch: Some(mr.source_branch),
            target_branch: Some(mr.target_branch),
            url: mr.web_url,
        }
    }
}

/// Queries a GitLab instance's REST API (v4) for one project's merge
/// requests.
pub struct GitLabMergeRequestAdapter {
    http: Arc<dyn HttpClient>,
}

impl GitLabMergeRequestAdapter {
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

/// GitLab's own `X-Next-Page` pagination header: present but empty on the
/// last page, present with a page number on every other page. Absent
/// entirely is treated the same as empty (defensively — GitLab documents
/// it as always present on a paginated endpoint, but "no next page" is the
/// safe default if that ever changes).
fn has_next_page(response: &HttpResponse) -> bool {
    response
        .header("x-next-page")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

impl PullRequestQueryPort for GitLabMergeRequestAdapter {
    fn list_pull_requests(
        &self,
        repository: &ForgeRepositoryRef,
        page: u32,
        token: Option<&ForgeToken>,
    ) -> Result<PullRequestPage, PullRequestQueryError> {
        let project_path = repository.path_segments.join("/");
        let encoded_path = percent_encode_path(&project_path);
        let host = &repository.host;
        let url = format!(
            "https://{host}/api/v4/projects/{encoded_path}/merge_requests?state=all&per_page={PER_PAGE}&page={page}"
        );

        let mut headers = vec![("Accept", "application/json")];
        if let Some(token) = token {
            headers.push(("PRIVATE-TOKEN", token.expose_secret()));
        }

        let response = self
            .http
            .get(&url, &headers)
            .map_err(|err| PullRequestQueryError::NetworkFailure(err.message))?;

        match response.status {
            200 => parse_page(&response),
            401 => Err(PullRequestQueryError::AuthenticationRequired),
            403 => Err(PullRequestQueryError::PermissionDenied),
            404 => Err(PullRequestQueryError::PermissionDenied),
            429 => Err(PullRequestQueryError::RateLimited {
                retry_after_seconds: retry_after_seconds(&response, now_unix()),
            }),
            status => Err(PullRequestQueryError::Other(GitSailError::new(
                ErrorCode::NetworkFailure,
                redact_secrets(&format!("GitLab API request failed with status {status}")),
            ))),
        }
    }
}

/// Percent-encodes every `/` in `path` as `%2F`, GitLab's documented way of
/// addressing a project by its full namespace path instead of its numeric
/// id. Every other character in a Git owner/group/repo path segment
/// (already validated as non-empty by
/// [`gitsail_domain::forge::repository_location`]) is left as-is: GitLab
/// project paths are restricted to a safe character set GitLab itself
/// enforces at creation time, and this adapter only ever receives segments
/// that came from an already-configured remote URL, not free-form user
/// input.
fn percent_encode_path(path: &str) -> String {
    path.replace('/', "%2F")
}

fn parse_page(response: &HttpResponse) -> Result<PullRequestPage, PullRequestQueryError> {
    let merge_requests: Vec<GitLabMergeRequest> =
        serde_json::from_str(&response.body).map_err(|err| {
            PullRequestQueryError::Other(
                GitSailError::new(
                    ErrorCode::ParseFailure,
                    "could not parse GitLab's merge request response",
                )
                .with_source(err),
            )
        })?;
    Ok(PullRequestPage {
        items: merge_requests
            .into_iter()
            .map(PullRequestSummary::from)
            .collect(),
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
            kind: ForgeKind::GitLab,
            host: "gitlab.com".to_string(),
            path_segments: vec![
                "group".to_string(),
                "subgroup".to_string(),
                "repo".to_string(),
            ],
        }
    }

    fn sample_body() -> &'static str {
        r#"[
            {
                "title": "Fix the thing",
                "state": "opened",
                "author": {"username": "alice"},
                "source_branch": "feature/fix",
                "target_branch": "main",
                "web_url": "https://gitlab.com/group/subgroup/repo/-/merge_requests/1"
            },
            {
                "title": "Old work",
                "state": "merged",
                "author": {"username": "bob"},
                "source_branch": "old-branch",
                "target_branch": "main",
                "web_url": "https://gitlab.com/group/subgroup/repo/-/merge_requests/2"
            },
            {
                "title": "Rejected work",
                "state": "closed",
                "author": null,
                "source_branch": "rejected",
                "target_branch": "main",
                "web_url": "https://gitlab.com/group/subgroup/repo/-/merge_requests/3"
            }
        ]"#
    }

    #[test]
    fn maps_a_successful_page_including_merged_and_closed_and_no_author() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], sample_body())));
        let adapter = GitLabMergeRequestAdapter::new(http);

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();

        assert_eq!(page.items.len(), 3);
        assert_eq!(page.items[0].state, PullRequestState::Open);
        assert_eq!(page.items[0].author.as_deref(), Some("alice"));
        assert_eq!(page.items[0].source_branch.as_deref(), Some("feature/fix"));
        assert_eq!(page.items[1].state, PullRequestState::Merged);
        assert_eq!(page.items[2].state, PullRequestState::Closed);
        assert_eq!(page.items[2].author, None);
        assert!(!page.has_next_page);
    }

    #[test]
    fn the_project_path_is_percent_encoded_for_nested_subgroups() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], "[]")));
        let adapter = GitLabMergeRequestAdapter::new(http.clone());

        adapter.list_pull_requests(&repository(), 1, None).unwrap();

        let calls = http.calls();
        assert!(calls[0]
            .0
            .contains("/projects/group%2Fsubgroup%2Frepo/merge_requests"));
    }

    #[test]
    fn a_self_hosted_host_is_used_verbatim_not_hardcoded_to_gitlab_com() {
        let repository = ForgeRepositoryRef {
            kind: ForgeKind::GitLab,
            host: "gitlab.example.com".to_string(),
            path_segments: vec!["team".to_string(), "repo".to_string()],
        };
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], "[]")));
        let adapter = GitLabMergeRequestAdapter::new(http.clone());

        adapter.list_pull_requests(&repository, 1, None).unwrap();

        let calls = http.calls();
        assert!(calls[0].0.starts_with("https://gitlab.example.com/api/v4/"));
    }

    #[test]
    fn an_x_next_page_header_with_a_value_reports_has_next_page() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[("X-Next-Page", "2")], "[]")));
        let adapter = GitLabMergeRequestAdapter::new(http);

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();
        assert!(page.has_next_page);
    }

    #[test]
    fn an_empty_x_next_page_header_means_no_next_page() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[("X-Next-Page", "")], "[]")));
        let adapter = GitLabMergeRequestAdapter::new(http);

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();
        assert!(!page.has_next_page);
    }

    #[test]
    fn a_token_is_sent_as_a_private_token_header_not_authorization() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], "[]")));
        let adapter = GitLabMergeRequestAdapter::new(http.clone());

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
            "PRIVATE-TOKEN".to_string(),
            "sentinel-fake-token".to_string()
        )));
        assert!(!headers.iter().any(|(name, _)| name == "Authorization"));
    }

    #[test]
    fn a_401_maps_to_authentication_required() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(401, &[], "{}")));
        let adapter = GitLabMergeRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::AuthenticationRequired
        ));
    }

    #[test]
    fn a_403_maps_to_permission_denied() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(403, &[], "{}")));
        let adapter = GitLabMergeRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::PermissionDenied
        ));
    }

    #[test]
    fn a_404_maps_to_permission_denied_documented_private_project_hiding() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(404, &[], "{}")));
        let adapter = GitLabMergeRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::PermissionDenied
        ));
    }

    #[test]
    fn a_429_maps_to_rate_limited_with_the_retry_after_wait_time() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(429, &[("Retry-After", "15")], "{}")));
        let adapter = GitLabMergeRequestAdapter::new(http);

        match adapter
            .list_pull_requests(&repository(), 1, None)
            .unwrap_err()
        {
            PullRequestQueryError::RateLimited {
                retry_after_seconds,
            } => {
                assert_eq!(retry_after_seconds, Some(15));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn a_transport_failure_maps_to_network_failure_offline() {
        use crate::http::HttpTransportError;
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Err(HttpTransportError {
            message: "timed out".to_string(),
        }));
        let adapter = GitLabMergeRequestAdapter::new(http);

        match adapter
            .list_pull_requests(&repository(), 1, None)
            .unwrap_err()
        {
            PullRequestQueryError::NetworkFailure(message) => assert_eq!(message, "timed out"),
            other => panic!("expected NetworkFailure, got {other:?}"),
        }
    }

    #[test]
    fn a_server_error_status_maps_to_other() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(500, &[], "boom")));
        let adapter = GitLabMergeRequestAdapter::new(http);

        assert!(matches!(
            adapter
                .list_pull_requests(&repository(), 1, None)
                .unwrap_err(),
            PullRequestQueryError::Other(_)
        ));
    }

    /// US-103 criterion 3 (see `github_pr_adapter`'s matching test for the
    /// full rationale).
    #[test]
    fn malicious_title_and_author_content_survives_as_opaque_untouched_text() {
        let malicious_title = "<b>bold</b><script>alert(1)</script>";
        let malicious_author = "*italic* [link](http://evil.example)";
        let body = serde_json::json!([{
            "title": malicious_title,
            "state": "opened",
            "author": {"username": malicious_author},
            "source_branch": "main",
            "target_branch": "main",
            "web_url": "https://gitlab.com/group/repo/-/merge_requests/9",
        }])
        .to_string();

        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], &body)));
        let adapter = GitLabMergeRequestAdapter::new(http);

        let page = adapter.list_pull_requests(&repository(), 1, None).unwrap();
        assert_eq!(page.items[0].title, malicious_title);
        assert_eq!(page.items[0].author.as_deref(), Some(malicious_author));
    }
}
