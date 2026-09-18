//! A minimal, adapter-agnostic HTTP transport (T-245/US-103).
//!
//! [`GitHubPullRequestAdapter`](crate::github_pr_adapter::GitHubPullRequestAdapter)
//! and
//! [`GitLabMergeRequestAdapter`](crate::gitlab_mr_adapter::GitLabMergeRequestAdapter)
//! depend on [`HttpClient`] — a tiny trait, not `ureq` directly — for
//! exactly one reason: it lets each adapter's real job (building the right
//! URL/headers for its forge, and mapping that forge's specific response
//! shape/status codes/rate-limit headers to
//! [`gitsail_application::PullRequestPage`]/
//! [`gitsail_application::PullRequestQueryError`]) be unit-tested against an
//! in-process [`FakeHttpClient`] double, with **no real network call and no
//! mock HTTP server**, satisfying T-245's DoD ("use um HTTP mock/double —
//! não faça chamadas de rede reais em teste") with the smallest possible
//! amount of test-only machinery. [`UreqHttpClient`] is the one production
//! implementation, and the only place in this crate `ureq` itself is
//! mentioned.

use std::collections::HashMap;
use std::time::Duration;

/// A transport-level failure: nothing about the forge's own response — no
/// status code was ever received at all (DNS failure, connection refused,
/// timeout, ...). This is exactly the "offline" state US-103 criterion 2
/// requires to be distinct from every other failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpTransportError {
    /// A redacted, log-safe description (never a raw credential — callers
    /// must not need to redact this again themselves, but `gitsail-forge`'s
    /// own adapters still pass it through `redact_secrets` as defense in
    /// depth, matching this crate's `keyring_store` module).
    pub message: String,
}

/// An HTTP response that was actually received — including a 4xx/5xx one:
/// unlike some HTTP clients, [`HttpClient::get`] never turns a non-2xx
/// status into an error. An adapter needs the exact status code and
/// headers of a 401/403/429 to build the right
/// [`gitsail_application::PullRequestQueryError`] variant, so collapsing
/// those into a generic "request failed" would throw away the information
/// this whole feature exists to preserve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// Header names are stored lowercased (HTTP header names are
    /// case-insensitive; this makes every adapter's own lookup a plain,
    /// case-sensitive `get` rather than each reimplementing
    /// case-insensitive matching).
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl HttpResponse {
    /// Case-insensitive header lookup (see the field's own doc comment).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// One GET request, headers in, full response (status/headers/body) or a
/// transport failure out. Deliberately GET-only and header-only-in: PR/MR
/// listing (T-245's whole scope) never needs a request body, a different
/// method, or any request feature richer than this.
pub trait HttpClient: Send + Sync {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpTransportError>;
}

/// The production [`HttpClient`], backed by `ureq`.
pub struct UreqHttpClient {
    agent: ureq::Agent,
}

impl Default for UreqHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqHttpClient {
    /// A 10s timeout: generous enough for a slow forge API, short enough
    /// that a hung TCP connection surfaces as the "offline" state (US-103
    /// criterion 2) instead of an indefinitely "loading" UI.
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(10))
                .build(),
        }
    }
}

impl HttpClient for UreqHttpClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpTransportError> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        match request.call() {
            Ok(response) => Ok(to_http_response(response)),
            // `ureq` reports any non-2xx status as `Err(Status(..))` by
            // default — this is exactly the "response we must still
            // inspect" case (see this module's `HttpResponse` doc
            // comment), not a transport failure.
            Err(ureq::Error::Status(_, response)) => Ok(to_http_response(response)),
            Err(ureq::Error::Transport(transport)) => Err(HttpTransportError {
                message: gitsail_domain::redact::redact_secrets(&transport.to_string()),
            }),
        }
    }
}

fn to_http_response(response: ureq::Response) -> HttpResponse {
    let status = response.status();
    let headers: HashMap<String, String> = response
        .headers_names()
        .into_iter()
        .filter_map(|name| {
            let value = response.header(&name)?.to_string();
            Some((name.to_ascii_lowercase(), value))
        })
        .collect();
    let body = response.into_string().unwrap_or_default();
    HttpResponse {
        status,
        headers,
        body,
    }
}

/// One recorded call a [`FakeHttpClient`] received: the URL, and the
/// headers passed, in order.
pub type RecordedHttpCall = (String, Vec<(String, String)>);

/// A scripted [`HttpClient`] double for tests (see this module's top doc
/// comment for why this — not a mock HTTP server — is this crate's whole
/// answer to "no real network calls in tests").
#[derive(Default)]
pub struct FakeHttpClient {
    /// Responses returned in call order; a call past the end panics with a
    /// clear message rather than silently reusing the last one, so a test
    /// asserting "exactly one GET happened" cannot pass by accident.
    responses: std::sync::Mutex<Vec<Result<HttpResponse, HttpTransportError>>>,
    /// Every call this double received, in order — so a test can assert an
    /// adapter built the right request (e.g. the right
    /// `Authorization`/`PRIVATE-TOKEN` header, or none at all when no
    /// token was supplied).
    calls: std::sync::Mutex<Vec<RecordedHttpCall>>,
}

impl FakeHttpClient {
    /// Queues `response` to be returned by the next [`HttpClient::get`]
    /// call, in the order queued.
    pub fn queue(&self, response: Result<HttpResponse, HttpTransportError>) {
        self.responses.lock().unwrap().push(response);
    }

    pub fn calls(&self) -> Vec<RecordedHttpCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl HttpClient for FakeHttpClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpTransportError> {
        self.calls.lock().unwrap().push((
            url.to_string(),
            headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ));
        // Queued in call order, so pop from the front (a `Vec` used as a
        // FIFO queue here, not a stack).
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            panic!("FakeHttpClient: no more queued responses for GET {url}");
        }
        responses.remove(0)
    }
}

/// Builds a plain 200 JSON response with the given headers, for tests.
#[cfg(test)]
pub(crate) fn json_response(status: u16, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
            .collect(),
        body: body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_is_case_insensitive() {
        let response = json_response(200, &[("X-Next-Page", "2")], "{}");
        assert_eq!(response.header("x-next-page"), Some("2"));
        assert_eq!(response.header("X-NEXT-PAGE"), Some("2"));
        assert_eq!(response.header("missing"), None);
    }

    #[test]
    fn fake_http_client_returns_queued_responses_and_records_calls() {
        let client = FakeHttpClient::default();
        client.queue(Ok(json_response(200, &[], "{}")));

        let response = client
            .get("https://example.test/x", &[("Accept", "application/json")])
            .unwrap();
        assert_eq!(response.status, 200);

        let calls = client.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "https://example.test/x");
        assert_eq!(
            calls[0].1,
            vec![("Accept".to_string(), "application/json".to_string())]
        );
    }

    #[test]
    #[should_panic(expected = "no more queued responses")]
    fn fake_http_client_panics_on_an_unexpected_extra_call() {
        let client = FakeHttpClient::default();
        let _ = client.get("https://example.test/x", &[]);
    }
}
