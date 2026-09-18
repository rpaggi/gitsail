//! [`UpdateCheckPort`] adapter for GitHub's Releases API (T-260/US-127).
//!
//! Mirrors `github_pr_adapter.rs`'s own shape exactly (T-245's precedent):
//! this is the one module in the workspace that knows GitHub's specific
//! `/releases/latest` response fields; `gitsail_application::update_check`
//! stays forge/HTTP-agnostic.
//!
//! ## Status code mapping
//! - `200` -> parse the body as a release.
//! - `404` -> [`UpdateCheckError::NoReleasesPublished`] — GitHub's own
//!   documented meaning for this endpoint when a repository has no
//!   releases yet (distinct from a malformed/unexpected response).
//! - Anything else (5xx, an unexpected 2xx shape, ...) ->
//!   [`UpdateCheckError::Malformed`], never a panic.

use std::sync::{Arc, Mutex};

use gitsail_application::{ReleaseInfo, UpdateCheckError, UpdateCheckPort};
use gitsail_domain::redact::redact_secrets;
use gitsail_domain::{ErrorCode, GitSailError};
use serde::Deserialize;

use crate::http::HttpClient;

/// The repository ADR-021 records as GitSail's current identity
/// (`docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`). Not configurable in
/// this first version — GitSail only ever checks its own upstream project's
/// releases.
const OWNER_REPO: &str = "rpaggi/gitsail";

/// The checksums asset every release `release.yml` has produced since
/// T-257 is named exactly this (`docs/architecture/release-process.md`).
const CHECKSUMS_ASSET_NAME: &str = "SHA256SUMS.txt";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

impl From<GitHubRelease> for ReleaseInfo {
    fn from(release: GitHubRelease) -> Self {
        let checksums_url = release
            .assets
            .into_iter()
            .find(|asset| asset.name == CHECKSUMS_ASSET_NAME)
            .map(|asset| asset.browser_download_url);
        ReleaseInfo {
            tag: release.tag_name,
            html_url: release.html_url,
            checksums_url,
            notes: release.body,
        }
    }
}

/// Queries GitHub's REST API for `OWNER_REPO`'s latest published release.
pub struct GitHubReleaseUpdateAdapter {
    http: Arc<dyn HttpClient>,
}

impl GitHubReleaseUpdateAdapter {
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http }
    }
}

impl UpdateCheckPort for GitHubReleaseUpdateAdapter {
    fn latest_release(&self) -> Result<ReleaseInfo, UpdateCheckError> {
        let url = format!("https://api.github.com/repos/{OWNER_REPO}/releases/latest");
        // GitHub requires a `User-Agent`; `Accept` pins the stable REST
        // media type, matching `github_pr_adapter`'s own headers exactly.
        // No token is ever sent: this is a public, read-only, unauthenticated
        // GET against a public repository's releases — nothing here needs
        // (or should risk sending) a forge credential.
        let headers = [
            ("Accept", "application/vnd.github+json"),
            ("User-Agent", "gitsail"),
        ];

        let response = self
            .http
            .get(&url, &headers)
            .map_err(|err| UpdateCheckError::NetworkFailure(err.message))?;

        match response.status {
            200 => parse_release(&response.body),
            404 => Err(UpdateCheckError::NoReleasesPublished),
            status => Err(UpdateCheckError::Malformed(GitSailError::new(
                ErrorCode::NetworkFailure,
                redact_secrets(&format!(
                    "GitHub releases API request failed with status {status}"
                )),
            ))),
        }
    }
}

fn parse_release(body: &str) -> Result<ReleaseInfo, UpdateCheckError> {
    let release: GitHubRelease = serde_json::from_str(body).map_err(|err| {
        UpdateCheckError::Malformed(
            GitSailError::new(
                ErrorCode::ParseFailure,
                "could not parse GitHub's latest-release response",
            )
            .with_source(err),
        )
    })?;
    Ok(ReleaseInfo::from(release))
}

/// A scripted whole-[`UpdateCheckPort`] double, mirroring
/// `FakePullRequestQueryPort`'s own precedent: for a consumer (e.g.
/// Desktop's own command tests) that just needs a canned outcome without
/// exercising this module's own HTTP-shape mapping (already covered by
/// this module's tests against [`crate::http::FakeHttpClient`]).
#[derive(Default)]
pub struct FakeUpdateCheckPort {
    result: Mutex<Option<Result<ReleaseInfo, UpdateCheckError>>>,
}

impl FakeUpdateCheckPort {
    pub fn new(result: Result<ReleaseInfo, UpdateCheckError>) -> Self {
        Self {
            result: Mutex::new(Some(result)),
        }
    }
}

impl UpdateCheckPort for FakeUpdateCheckPort {
    fn latest_release(&self) -> Result<ReleaseInfo, UpdateCheckError> {
        self.result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Err(UpdateCheckError::NoReleasesPublished))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{json_response, FakeHttpClient};

    fn sample_body() -> &'static str {
        r#"{
            "tag_name": "v0.5.0",
            "html_url": "https://github.com/rpaggi/gitsail/releases/tag/v0.5.0",
            "body": "What's new: fixed things",
            "assets": [
                {"name": "gitsail-v0.5.0-linux-x86_64.tar.gz", "browser_download_url": "https://github.com/rpaggi/gitsail/releases/download/v0.5.0/gitsail-v0.5.0-linux-x86_64.tar.gz"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/rpaggi/gitsail/releases/download/v0.5.0/SHA256SUMS.txt"}
            ]
        }"#
    }

    #[test]
    fn maps_a_successful_response_including_the_checksums_asset() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], sample_body())));
        let adapter = GitHubReleaseUpdateAdapter::new(http);

        let release = adapter.latest_release().unwrap();

        assert_eq!(release.tag, "v0.5.0");
        assert_eq!(
            release.html_url,
            "https://github.com/rpaggi/gitsail/releases/tag/v0.5.0"
        );
        assert_eq!(
            release.checksums_url.as_deref(),
            Some("https://github.com/rpaggi/gitsail/releases/download/v0.5.0/SHA256SUMS.txt")
        );
        assert_eq!(release.notes.as_deref(), Some("What's new: fixed things"));
    }

    #[test]
    fn a_release_with_no_checksums_asset_leaves_it_none_rather_than_guessing() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(
            200,
            &[],
            r#"{"tag_name": "v0.5.0", "html_url": "https://github.com/rpaggi/gitsail/releases/tag/v0.5.0", "assets": []}"#,
        )));
        let adapter = GitHubReleaseUpdateAdapter::new(http);

        let release = adapter.latest_release().unwrap();
        assert_eq!(release.checksums_url, None);
        assert_eq!(release.notes, None);
    }

    #[test]
    fn a_404_maps_to_no_releases_published_not_a_failure() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(404, &[], "{}")));
        let adapter = GitHubReleaseUpdateAdapter::new(http);

        assert!(matches!(
            adapter.latest_release().unwrap_err(),
            UpdateCheckError::NoReleasesPublished
        ));
    }

    #[test]
    fn a_server_error_status_maps_to_malformed_not_a_fabricated_release() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(500, &[], "internal error")));
        let adapter = GitHubReleaseUpdateAdapter::new(http);

        assert!(matches!(
            adapter.latest_release().unwrap_err(),
            UpdateCheckError::Malformed(_)
        ));
    }

    #[test]
    fn an_unparseable_body_maps_to_malformed_rather_than_panicking() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], "{ not valid json")));
        let adapter = GitHubReleaseUpdateAdapter::new(http);

        assert!(matches!(
            adapter.latest_release().unwrap_err(),
            UpdateCheckError::Malformed(_)
        ));
    }

    #[test]
    fn a_transport_failure_maps_to_network_failure_offline() {
        use crate::http::HttpTransportError;
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Err(HttpTransportError {
            message: "connection refused".to_string(),
        }));
        let adapter = GitHubReleaseUpdateAdapter::new(http);

        match adapter.latest_release().unwrap_err() {
            UpdateCheckError::NetworkFailure(message) => {
                assert_eq!(message, "connection refused")
            }
            other => panic!("expected NetworkFailure, got {other:?}"),
        }
    }

    #[test]
    fn no_authorization_header_is_ever_sent_this_is_a_public_unauthenticated_check() {
        let http = Arc::new(FakeHttpClient::default());
        http.queue(Ok(json_response(200, &[], sample_body())));
        let adapter = GitHubReleaseUpdateAdapter::new(http.clone());

        adapter.latest_release().unwrap();

        let calls = http.calls();
        assert!(!calls[0].1.iter().any(|(name, _)| name == "Authorization"));
    }
}
