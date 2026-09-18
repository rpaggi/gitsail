//! Pull/Merge Request listing, in explicitly limited scope (SAD/EPIC-20,
//! T-245/US-103: "quero listar e abrir PRs/MRs relacionados ao repositório,
//! para acompanhar revisão sem um cliente completo de projetos").
//!
//! ## Scope of this task (documented decision — DoD requirement)
//!
//! This is **not** a PR/MR client. It implements exactly:
//! - Listing title, state (open/closed/merged), author, and source/target
//!   branch for the current repository's detected forge remote
//!   (US-103 criterion 1), one page at a time.
//! - A distinct, explicit outcome for every failure mode US-103 criterion 2
//!   calls out — no token/insufficient permission, rate limiting (with a
//!   wait time when the forge API reports one), offline/network failure —
//!   so none of these can ever be misread as "this repository simply has no
//!   PRs/MRs" (see [`ListPullRequestsOutcome`]).
//!
//! It deliberately does **not** implement: file diffs, comments, CI/check
//! status, reviewers, labels, merging/creating/commenting on a PR/MR, or
//! any write access at all (T-244/US-102's module docs already fix the
//! token scope GitSail requests to read-only). A future story that wants
//! any of this must add its own explicitly-scoped capability.
//!
//! ## Where this sits in Ports & Adapters
//!
//! [`PullRequestQueryPort`] is the boundary: this module (and
//! [`ListPullRequests`]) knows nothing about HTTP, JSON, or which forge
//! uses which API shape — that lives in `gitsail-forge`'s adapters
//! (`GitHubPullRequestAdapter`/`GitLabMergeRequestAdapter`), which map each
//! forge's own response shape to [`PullRequestSummary`]/[`PullRequestPage`]
//! and each forge's own failure shape (headers, status codes) to
//! [`PullRequestQueryError`]. This module only composes: pick the forge
//! remote (reusing [`crate::forge_links::pick_forge_remote`]'s exact
//! policy, so "which remote" is never decided twice), look up whatever
//! token is currently stored for it (never required — T-245's suggested
//! scope explicitly allows falling back to public/unauthenticated listing;
//! see [`ListPullRequests::execute`]), and translate the port's result into
//! one of the explicit states the UI must distinguish.
//!
//! ## Untrusted content (US-103 criterion 3)
//! `PullRequestSummary::title`/`author`/`source_branch`/`target_branch` are
//! repository/forge-authored free text — the same trust level as a commit
//! subject or author name. Nothing in this crate (or `gitsail-domain`)
//! renders them; a presentation layer must treat them exactly like
//! `gitsail-tui`'s `sanitize` module and the VS Code extension's
//! `hoverSanitizer` already treat commit content: escaped, never
//! interpreted as active markup, and never used to construct a URL that is
//! opened without a fresh, independent validation (a PR's own `url` field
//! is opened only through an explicit user action and only after the
//! presentation layer re-validates its host against the detected forge —
//! see `apps/desktop/src-tauri/src/commands.rs`'s `open_pull_request_link`).

use std::sync::Arc;

use gitsail_domain::{repository_location, ForgeKind, GitSailError, Remote};

use crate::forge_credentials::{ForgeAccountId, ForgeCredentialPort, ForgeToken};
use crate::forge_links::pick_forge_remote;

/// One PR's (GitHub) or MR's (GitLab) state, normalized to the three
/// states both forges' web UIs actually distinguish (US-103 criterion 1).
///
/// GitLab's API additionally has a transitional `"locked"` state (a merge
/// request mid-merge); adapters map it to [`Closed`](Self::Closed) — it is
/// not open for further review and is not yet confirmed merged, and adding
/// a fourth UI state for a narrow transitional window was judged not worth
/// the extra state every presentation layer would have to handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// One PR/MR, agnostic to which forge it came from (US-103 criterion 1).
///
/// Every `String` field here is untrusted, forge/repository-authored
/// content — see this module's top doc comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestSummary {
    pub title: String,
    pub state: PullRequestState,
    /// The author's display/login name, or `None` when the forge reports
    /// no author at all (e.g. a deleted account) — distinct from an empty
    /// string, so a presentation layer can render its own "(unknown)"
    /// label rather than an empty one.
    pub author: Option<String>,
    /// The source ("head"/"compare") branch name, when the forge's
    /// response includes one intact (GitHub/GitLab both omit or null this
    /// when the source branch has since been deleted).
    pub source_branch: Option<String>,
    /// The target ("base") branch name.
    pub target_branch: Option<String>,
    /// The PR/MR's own web page — opened only via an explicit user action,
    /// after being re-validated against the detected forge host (US-103
    /// criterion 3; see this module's top doc comment).
    pub url: String,
}

/// One page of [`PullRequestSummary`] results.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PullRequestPage {
    pub items: Vec<PullRequestSummary>,
    /// Whether the forge API reports at least one more page after this
    /// one. An empty `items` with `has_next_page: false` is the *only*
    /// shape that means "this repository truly has no PRs/MRs" (US-103
    /// criterion 2) — every other empty-looking result is one of
    /// [`ListPullRequestsOutcome`]'s other variants instead.
    pub has_next_page: bool,
}

/// Identifies which repository, on which forge/host, a
/// [`PullRequestQueryPort`] call is for — the pieces
/// [`gitsail_domain::forge::repository_location`] resolves from a remote,
/// carried alongside the [`ForgeKind`] a dispatching adapter needs to pick
/// the right API shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeRepositoryRef {
    pub kind: ForgeKind,
    pub host: String,
    /// Repository path segments: `[owner, repo]` for GitHub, or
    /// `[group, subgroup.., repo]` for GitLab (subgroups are real on
    /// GitLab, never on GitHub — see `gitsail_domain::forge`'s own doc
    /// comment on this asymmetry).
    pub path_segments: Vec<String>,
}

/// Every distinct failure a [`PullRequestQueryPort`] adapter can report,
/// deliberately richer than [`GitSailError`]'s single `code` for the one
/// case that needs structured data no existing `ErrorCode` carries: a
/// rate-limit wait time (US-103 criterion 2). Kept local to this module
/// rather than added to `gitsail_domain::ErrorCode` because `ErrorCode` is
/// a stable, workspace-wide taxonomy matched exhaustively in several
/// places (SAD §19) — a forge-API-specific "how long until retry" concept
/// does not belong alongside `RepositoryLocked`/`GitNotInstalled`/etc, and
/// forcing it through `GitSailError::remediation`'s free-form text would
/// make the wait time unparseable by a presentation layer instead of a
/// plain, typed `Option<u64>`.
#[derive(Debug)]
pub enum PullRequestQueryError {
    /// 401 (or the forge's own equivalent): no credential, or the stored
    /// one is invalid/expired.
    AuthenticationRequired,
    /// 403 (or, for a private repository, the 404-that-really-means-403 an
    /// unauthenticated/under-scoped request gets from GitHub/GitLab — see
    /// each adapter's own doc comment for that mapping): a credential was
    /// presented but does not carry enough access.
    PermissionDenied,
    /// 429 (or GitHub's secondary-rate-limit 403), with the wait time the
    /// API itself reported, when it reported one.
    RateLimited { retry_after_seconds: Option<u64> },
    /// Timeout, DNS failure, connection refused, or any other
    /// transport-level failure — "offline" from the UI's point of view.
    /// Always a redacted, log-safe message (never a raw token/credential).
    NetworkFailure(String),
    /// Anything else unexpected: a malformed response body, an
    /// unrecognized status code, ... — surfaced as an ordinary
    /// [`GitSailError`] rather than invented structure this module cannot
    /// give any better meaning to.
    Other(GitSailError),
}

/// Queries one page of PRs/MRs for a repository on a specific forge
/// (US-103's own port, implemented by an adapter outside this crate — see
/// `gitsail-forge`'s `GitHubPullRequestAdapter`/`GitLabMergeRequestAdapter`,
/// and its `CompositePullRequestQueryPort` that dispatches between them by
/// [`ForgeRepositoryRef::kind`]).
///
/// `page` is 1-based, matching both GitHub's and GitLab's own `page` query
/// parameter (US-103 criterion 2's pagination — this port never fetches
/// every page itself; a presentation layer asks for one page at a time so
/// "loading" is always a single bounded network call).
pub trait PullRequestQueryPort: Send + Sync {
    fn list_pull_requests(
        &self,
        repository: &ForgeRepositoryRef,
        page: u32,
        token: Option<&ForgeToken>,
    ) -> Result<PullRequestPage, PullRequestQueryError>;
}

/// Every state a caller (a Tauri command, a TUI view, ...) must be able to
/// render distinctly (US-103 criterion 2) — deliberately not a single
/// `Result<PullRequestPage, GitSailError>`, because collapsing "no PRs" and
/// "could not check" into the same empty-list shape is exactly the bug
/// this criterion exists to prevent.
#[derive(Debug)]
pub enum ListPullRequestsOutcome {
    /// No configured remote resolves to a known GitHub/GitLab forge (same
    /// "no link available, never an error" case [`crate::forge_links`]
    /// already establishes for T-243).
    NoForgeDetected,
    /// A real page of results — `items` empty *and* `has_next_page: false`
    /// is the only shape meaning "truly no PRs/MRs" (US-103 criterion 2).
    Page(PullRequestPage),
    AuthenticationRequired,
    PermissionDenied,
    RateLimited {
        retry_after_seconds: Option<u64>,
    },
    Offline {
        message: String,
    },
    Error(GitSailError),
}

/// Lists one page of PRs/MRs for whichever remote
/// [`pick_forge_remote`] selects, using whatever token (if any)
/// [`ForgeCredentialPort`] currently has stored for that forge/host.
pub struct ListPullRequests {
    query_port: Arc<dyn PullRequestQueryPort>,
    credentials: Arc<dyn ForgeCredentialPort>,
}

impl ListPullRequests {
    pub fn new(
        query_port: Arc<dyn PullRequestQueryPort>,
        credentials: Arc<dyn ForgeCredentialPort>,
    ) -> Self {
        Self {
            query_port,
            credentials,
        }
    }

    /// `page` is 1-based (see [`PullRequestQueryPort`]'s own doc comment).
    pub fn execute(&self, remotes: &[Remote], page: u32) -> ListPullRequestsOutcome {
        let Some((remote, kind)) = pick_forge_remote(remotes) else {
            return ListPullRequestsOutcome::NoForgeDetected;
        };
        let Some((host, path_segments)) = repository_location(kind, &remote.fetch_url) else {
            return ListPullRequestsOutcome::NoForgeDetected;
        };
        let repository = ForgeRepositoryRef {
            kind,
            host: host.clone(),
            path_segments,
        };

        // No stored token is never a hard stop (suggested scope
        // decision: still try to list *public* PRs/MRs unauthenticated —
        // see this module's top doc comment). A credential-store failure
        // is folded into "no token" the same way
        // `GetForgeConnectionStatus` already folds it into `NotConnected`:
        // a keyring hiccup must never look like a reason to refuse a
        // request that might well succeed unauthenticated anyway.
        let account = ForgeAccountId::new(kind, host);
        let token = self.credentials.token(&account).ok().flatten();

        match self
            .query_port
            .list_pull_requests(&repository, page, token.as_ref())
        {
            Ok(result_page) => ListPullRequestsOutcome::Page(result_page),
            Err(PullRequestQueryError::AuthenticationRequired) => {
                ListPullRequestsOutcome::AuthenticationRequired
            }
            Err(PullRequestQueryError::PermissionDenied) => {
                ListPullRequestsOutcome::PermissionDenied
            }
            Err(PullRequestQueryError::RateLimited {
                retry_after_seconds,
            }) => ListPullRequestsOutcome::RateLimited {
                retry_after_seconds,
            },
            Err(PullRequestQueryError::NetworkFailure(message)) => {
                ListPullRequestsOutcome::Offline { message }
            }
            Err(PullRequestQueryError::Other(err)) => ListPullRequestsOutcome::Error(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::RemoteUrl;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn remote(name: &str, url: &str) -> Remote {
        Remote {
            name: name.to_string(),
            fetch_url: RemoteUrl::new(url),
            push_url: RemoteUrl::new(url),
        }
    }

    fn github_remotes() -> Vec<Remote> {
        vec![remote("origin", "https://github.com/org/repo.git")]
    }

    #[derive(Default)]
    struct FakeCredentials {
        tokens: Mutex<HashMap<ForgeAccountId, ForgeToken>>,
    }
    impl ForgeCredentialPort for FakeCredentials {
        fn connect(&self, account: &ForgeAccountId, token: ForgeToken) -> Result<(), GitSailError> {
            self.tokens.lock().unwrap().insert(account.clone(), token);
            Ok(())
        }
        fn disconnect(&self, account: &ForgeAccountId) -> Result<(), GitSailError> {
            self.tokens.lock().unwrap().remove(account);
            Ok(())
        }
        fn status(
            &self,
            account: &ForgeAccountId,
        ) -> Result<crate::ForgeConnectionStatus, GitSailError> {
            Ok(if self.tokens.lock().unwrap().contains_key(account) {
                crate::ForgeConnectionStatus::Connected
            } else {
                crate::ForgeConnectionStatus::NotConnected
            })
        }
        fn token(&self, account: &ForgeAccountId) -> Result<Option<ForgeToken>, GitSailError> {
            Ok(self.tokens.lock().unwrap().get(account).cloned())
        }
    }

    /// A scripted [`PullRequestQueryPort`] double: records the last call it
    /// received (so tests can assert *what* was requested, e.g. that a
    /// stored token was actually passed through) and returns whatever
    /// result was configured.
    struct ScriptedPort {
        result: Mutex<Option<Result<PullRequestPage, PullRequestQueryErrorKind>>>,
        last_call: Mutex<Option<(ForgeRepositoryRef, u32, Option<String>)>>,
    }
    enum PullRequestQueryErrorKind {
        AuthenticationRequired,
        PermissionDenied,
        RateLimited(Option<u64>),
        NetworkFailure(String),
        Other,
    }
    impl ScriptedPort {
        fn new(result: Result<PullRequestPage, PullRequestQueryErrorKind>) -> Self {
            Self {
                result: Mutex::new(Some(result)),
                last_call: Mutex::new(None),
            }
        }
    }
    impl PullRequestQueryPort for ScriptedPort {
        fn list_pull_requests(
            &self,
            repository: &ForgeRepositoryRef,
            page: u32,
            token: Option<&ForgeToken>,
        ) -> Result<PullRequestPage, PullRequestQueryError> {
            *self.last_call.lock().unwrap() = Some((
                repository.clone(),
                page,
                token.map(|t| t.expose_secret().to_string()),
            ));
            match self
                .result
                .lock()
                .unwrap()
                .take()
                .expect("called more than once")
            {
                Ok(page) => Ok(page),
                Err(PullRequestQueryErrorKind::AuthenticationRequired) => {
                    Err(PullRequestQueryError::AuthenticationRequired)
                }
                Err(PullRequestQueryErrorKind::PermissionDenied) => {
                    Err(PullRequestQueryError::PermissionDenied)
                }
                Err(PullRequestQueryErrorKind::RateLimited(secs)) => {
                    Err(PullRequestQueryError::RateLimited {
                        retry_after_seconds: secs,
                    })
                }
                Err(PullRequestQueryErrorKind::NetworkFailure(msg)) => {
                    Err(PullRequestQueryError::NetworkFailure(msg))
                }
                Err(PullRequestQueryErrorKind::Other) => Err(PullRequestQueryError::Other(
                    GitSailError::new(gitsail_domain::ErrorCode::Internal, "boom"),
                )),
            }
        }
    }

    fn sample_pr() -> PullRequestSummary {
        PullRequestSummary {
            title: "Fix the thing".to_string(),
            state: PullRequestState::Open,
            author: Some("octocat".to_string()),
            source_branch: Some("feature/fix".to_string()),
            target_branch: Some("main".to_string()),
            url: "https://github.com/org/repo/pull/1".to_string(),
        }
    }

    #[test]
    fn no_forge_detected_short_circuits_without_calling_the_port() {
        struct PanicPort;
        impl PullRequestQueryPort for PanicPort {
            fn list_pull_requests(
                &self,
                _: &ForgeRepositoryRef,
                _: u32,
                _: Option<&ForgeToken>,
            ) -> Result<PullRequestPage, PullRequestQueryError> {
                panic!("must not be called when no forge remote is detected");
            }
        }
        let remotes = vec![remote(
            "origin",
            "https://internal.example.com/team/repo.git",
        )];
        let use_case =
            ListPullRequests::new(Arc::new(PanicPort), Arc::new(FakeCredentials::default()));

        assert!(matches!(
            use_case.execute(&remotes, 1),
            ListPullRequestsOutcome::NoForgeDetected
        ));
    }

    #[test]
    fn successful_page_is_passed_through_unchanged() {
        let page = PullRequestPage {
            items: vec![sample_pr()],
            has_next_page: true,
        };
        let port = Arc::new(ScriptedPort::new(Ok(page.clone())));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));

        match use_case.execute(&github_remotes(), 1) {
            ListPullRequestsOutcome::Page(result) => assert_eq!(result, page),
            other => panic!("expected Page, got {other:?}"),
        }
    }

    #[test]
    fn truly_empty_page_is_distinguishable_from_every_error_state() {
        let empty = PullRequestPage {
            items: vec![],
            has_next_page: false,
        };
        let port = Arc::new(ScriptedPort::new(Ok(empty.clone())));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));

        match use_case.execute(&github_remotes(), 1) {
            ListPullRequestsOutcome::Page(result) => {
                assert!(result.items.is_empty());
                assert!(!result.has_next_page);
            }
            other => panic!("expected an empty Page (not an error state), got {other:?}"),
        }
    }

    #[test]
    fn stored_token_is_passed_through_to_the_port() {
        let credentials = Arc::new(FakeCredentials::default());
        let account = ForgeAccountId::new(ForgeKind::GitHub, "github.com");
        credentials
            .connect(&account, ForgeToken::new("sentinel-fake-token"))
            .unwrap();

        let empty = PullRequestPage::default();
        let port = Arc::new(ScriptedPort::new(Ok(empty)));
        let use_case = ListPullRequests::new(port.clone(), credentials);

        use_case.execute(&github_remotes(), 1);

        let (repository, page, token) = port.last_call.lock().unwrap().clone().unwrap();
        assert_eq!(repository.kind, ForgeKind::GitHub);
        assert_eq!(repository.host, "github.com");
        assert_eq!(
            repository.path_segments,
            vec!["org".to_string(), "repo".to_string()]
        );
        assert_eq!(page, 1);
        assert_eq!(token.as_deref(), Some("sentinel-fake-token"));
    }

    #[test]
    fn no_stored_token_still_calls_the_port_unauthenticated() {
        let empty = PullRequestPage::default();
        let port = Arc::new(ScriptedPort::new(Ok(empty)));
        let use_case = ListPullRequests::new(port.clone(), Arc::new(FakeCredentials::default()));

        use_case.execute(&github_remotes(), 1);

        let (_, _, token) = port.last_call.lock().unwrap().clone().unwrap();
        assert_eq!(
            token, None,
            "must still attempt an unauthenticated (public) listing"
        );
    }

    #[test]
    fn a_credential_store_failure_falls_back_to_unauthenticated_rather_than_erroring() {
        struct AlwaysFailingCredentials;
        impl ForgeCredentialPort for AlwaysFailingCredentials {
            fn connect(&self, _: &ForgeAccountId, _: ForgeToken) -> Result<(), GitSailError> {
                unimplemented!()
            }
            fn disconnect(&self, _: &ForgeAccountId) -> Result<(), GitSailError> {
                unimplemented!()
            }
            fn status(
                &self,
                _: &ForgeAccountId,
            ) -> Result<crate::ForgeConnectionStatus, GitSailError> {
                unimplemented!()
            }
            fn token(&self, _: &ForgeAccountId) -> Result<Option<ForgeToken>, GitSailError> {
                Err(GitSailError::new(
                    gitsail_domain::ErrorCode::Internal,
                    "keyring boom",
                ))
            }
        }
        let empty = PullRequestPage::default();
        let port = Arc::new(ScriptedPort::new(Ok(empty)));
        let use_case = ListPullRequests::new(port, Arc::new(AlwaysFailingCredentials));

        assert!(matches!(
            use_case.execute(&github_remotes(), 1),
            ListPullRequestsOutcome::Page(_)
        ));
    }

    #[test]
    fn authentication_required_is_distinct_from_permission_denied() {
        let port = Arc::new(ScriptedPort::new(Err(
            PullRequestQueryErrorKind::AuthenticationRequired,
        )));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));
        assert!(matches!(
            use_case.execute(&github_remotes(), 1),
            ListPullRequestsOutcome::AuthenticationRequired
        ));

        let port = Arc::new(ScriptedPort::new(Err(
            PullRequestQueryErrorKind::PermissionDenied,
        )));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));
        assert!(matches!(
            use_case.execute(&github_remotes(), 1),
            ListPullRequestsOutcome::PermissionDenied
        ));
    }

    #[test]
    fn rate_limited_carries_the_reported_wait_time_through() {
        let port = Arc::new(ScriptedPort::new(Err(
            PullRequestQueryErrorKind::RateLimited(Some(42)),
        )));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));

        match use_case.execute(&github_remotes(), 1) {
            ListPullRequestsOutcome::RateLimited {
                retry_after_seconds,
            } => {
                assert_eq!(retry_after_seconds, Some(42));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn rate_limited_without_a_reported_wait_time_is_still_distinct_from_offline() {
        let port = Arc::new(ScriptedPort::new(Err(
            PullRequestQueryErrorKind::RateLimited(None),
        )));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));

        assert!(matches!(
            use_case.execute(&github_remotes(), 1),
            ListPullRequestsOutcome::RateLimited {
                retry_after_seconds: None
            }
        ));
    }

    #[test]
    fn network_failure_maps_to_the_offline_state() {
        let port = Arc::new(ScriptedPort::new(Err(
            PullRequestQueryErrorKind::NetworkFailure("connection refused".to_string()),
        )));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));

        match use_case.execute(&github_remotes(), 1) {
            ListPullRequestsOutcome::Offline { message } => {
                assert_eq!(message, "connection refused")
            }
            other => panic!("expected Offline, got {other:?}"),
        }
    }

    #[test]
    fn an_unexpected_failure_surfaces_as_a_plain_error_not_fabricated_structure() {
        let port = Arc::new(ScriptedPort::new(Err(PullRequestQueryErrorKind::Other)));
        let use_case = ListPullRequests::new(port, Arc::new(FakeCredentials::default()));

        match use_case.execute(&github_remotes(), 1) {
            ListPullRequestsOutcome::Error(err) => {
                assert_eq!(err.code(), gitsail_domain::ErrorCode::Internal);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
