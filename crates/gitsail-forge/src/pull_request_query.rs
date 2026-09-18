//! Composes the two forge-specific [`PullRequestQueryPort`] adapters into
//! the single port a consumer (e.g. Desktop's `AppState`) holds (T-245/
//! US-103) — the same "one credential-agnostic port at the boundary, forge
//! detection happens beneath it" shape `ForgeCredentialPort`'s single
//! `keyring` service name already establishes for T-244.
//!
//! Also exports [`FakePullRequestQueryPort`], a whole-port test double for
//! consumers that only need "PR listing returns this canned outcome" (e.g.
//! Desktop Tauri command tests) without exercising either adapter's own
//! HTTP-shape mapping — that mapping is already covered by
//! `github_pr_adapter`'s and `gitlab_mr_adapter`'s own unit tests against
//! [`crate::http::FakeHttpClient`].

use std::sync::{Arc, Mutex};

use gitsail_application::{
    ForgeRepositoryRef, ForgeToken, PullRequestPage, PullRequestQueryError, PullRequestQueryPort,
};
use gitsail_domain::ForgeKind;

use crate::github_pr_adapter::GitHubPullRequestAdapter;
use crate::gitlab_mr_adapter::GitLabMergeRequestAdapter;
use crate::http::{HttpClient, UreqHttpClient};

/// Dispatches a [`PullRequestQueryPort`] call to whichever concrete adapter
/// matches [`ForgeRepositoryRef::kind`]. This is the type Desktop's
/// `AppState` actually holds — callers never need to know GitHub and
/// GitLab are two different adapters underneath.
pub struct CompositePullRequestQueryPort {
    github: GitHubPullRequestAdapter,
    gitlab: GitLabMergeRequestAdapter,
}

impl CompositePullRequestQueryPort {
    /// The production composite, backed by [`UreqHttpClient`] for both
    /// forges (a single shared `ureq::Agent` — the same connection pool
    /// serves both, and neither forge needs a different one).
    pub fn production() -> Self {
        let http: Arc<dyn HttpClient> = Arc::new(UreqHttpClient::new());
        Self::with_adapters(
            GitHubPullRequestAdapter::new(http.clone()),
            GitLabMergeRequestAdapter::new(http),
        )
    }

    /// Builds a composite over already-constructed adapters — the seam
    /// this module's own tests use to inject `FakeHttpClient`-backed
    /// adapters (never a real `UreqHttpClient`) so dispatch can be
    /// verified with no real network call, matching this whole feature's
    /// DoD requirement.
    pub fn with_adapters(
        github: GitHubPullRequestAdapter,
        gitlab: GitLabMergeRequestAdapter,
    ) -> Self {
        Self { github, gitlab }
    }
}

impl Default for CompositePullRequestQueryPort {
    fn default() -> Self {
        Self::production()
    }
}

impl PullRequestQueryPort for CompositePullRequestQueryPort {
    fn list_pull_requests(
        &self,
        repository: &ForgeRepositoryRef,
        page: u32,
        token: Option<&ForgeToken>,
    ) -> Result<PullRequestPage, PullRequestQueryError> {
        match repository.kind {
            ForgeKind::GitHub => self.github.list_pull_requests(repository, page, token),
            ForgeKind::GitLab => self.gitlab.list_pull_requests(repository, page, token),
        }
    }
}

/// A scripted whole-[`PullRequestQueryPort`] double (see this module's top
/// doc comment for when to reach for this instead of
/// [`crate::http::FakeHttpClient`]).
#[derive(Default)]
pub struct FakePullRequestQueryPort {
    result: Mutex<Option<Result<PullRequestPage, PullRequestQueryError>>>,
}

impl FakePullRequestQueryPort {
    pub fn new(result: Result<PullRequestPage, PullRequestQueryError>) -> Self {
        Self {
            result: Mutex::new(Some(result)),
        }
    }
}

impl PullRequestQueryPort for FakePullRequestQueryPort {
    fn list_pull_requests(
        &self,
        _repository: &ForgeRepositoryRef,
        _page: u32,
        _token: Option<&ForgeToken>,
    ) -> Result<PullRequestPage, PullRequestQueryError> {
        self.result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Ok(PullRequestPage::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_application::PullRequestSummary;

    fn sample_ref(kind: ForgeKind) -> ForgeRepositoryRef {
        ForgeRepositoryRef {
            kind,
            host: match kind {
                ForgeKind::GitHub => "github.com".to_string(),
                ForgeKind::GitLab => "gitlab.com".to_string(),
            },
            path_segments: vec!["org".to_string(), "repo".to_string()],
        }
    }

    #[test]
    fn dispatches_github_and_gitlab_to_their_own_adapter_without_crossing_forges() {
        use crate::http::{json_response, FakeHttpClient};

        // Two independent fakes (never a real `UreqHttpClient`/network
        // call) queued with distinguishable bodies, so a dispatch bug that
        // sent a GitHub-shaped request to the GitLab adapter (or vice
        // versa) would surface as a parse failure instead of silently
        // passing.
        let github_http = Arc::new(FakeHttpClient::default());
        github_http.queue(Ok(json_response(200, &[], "[]")));
        let gitlab_http = Arc::new(FakeHttpClient::default());
        gitlab_http.queue(Ok(json_response(200, &[("X-Next-Page", "2")], "[]")));

        let composite = CompositePullRequestQueryPort::with_adapters(
            GitHubPullRequestAdapter::new(github_http.clone()),
            GitLabMergeRequestAdapter::new(gitlab_http.clone()),
        );

        let github_page = composite
            .list_pull_requests(&sample_ref(ForgeKind::GitHub), 1, None)
            .unwrap();
        let gitlab_page = composite
            .list_pull_requests(&sample_ref(ForgeKind::GitLab), 1, None)
            .unwrap();

        assert!(
            !github_page.has_next_page,
            "must have been served by the GitHub fake, not GitLab's"
        );
        assert!(
            gitlab_page.has_next_page,
            "must have been served by the GitLab fake, not GitHub's"
        );
        assert_eq!(github_http.calls().len(), 1);
        assert_eq!(gitlab_http.calls().len(), 1);
    }

    #[test]
    fn fake_port_returns_the_configured_result_exactly_once() {
        let page = PullRequestPage {
            items: vec![PullRequestSummary {
                title: "t".to_string(),
                state: gitsail_application::PullRequestState::Open,
                author: None,
                source_branch: None,
                target_branch: None,
                url: "https://example.test/pr/1".to_string(),
            }],
            has_next_page: false,
        };
        let fake = FakePullRequestQueryPort::new(Ok(page.clone()));

        let result = fake
            .list_pull_requests(&sample_ref(ForgeKind::GitHub), 1, None)
            .unwrap();
        assert_eq!(result, page);
    }

    #[test]
    fn fake_port_defaults_to_an_empty_page_when_unconfigured() {
        let fake = FakePullRequestQueryPort::default();
        let result = fake
            .list_pull_requests(&sample_ref(ForgeKind::GitHub), 1, None)
            .unwrap();
        assert_eq!(result, PullRequestPage::default());
    }
}
