//! Forge (GitHub/GitLab) detection and web-link construction (SAD/EPIC-20,
//! T-243/US-101: "abrir repository, branch e commit no navegador, para
//! consultar contexto remoto").
//!
//! ## Scope of `detect_forge` (documented decision)
//!
//! Recognized today:
//! - `github.com` (and `www.github.com`).
//! - `gitlab.com` (and `www.gitlab.com`).
//! - Self-hosted GitLab, via a cheap, deliberately narrow heuristic: a
//!   hostname whose *first* DNS label is exactly `gitlab` (e.g.
//!   `gitlab.example.com`, `gitlab.mycompany.io`) is treated as GitLab.
//!   This convention is common enough in practice to be worth the low
//!   false-positive risk, and detection failing closed (see below) means a
//!   false positive here only means an offered link goes to a host that
//!   turns out not to run GitLab — never a broken Git operation.
//!
//! Deliberately **not** attempted: GitHub Enterprise Server / other
//! self-hosted GitHub. Unlike GitLab, there is no comparable naming
//! convention for self-hosted GitHub instances (`git.example.com`,
//! `github.example.com`, `code.example.com` are all common), so guessing
//! from the hostname alone would be materially more prone to false
//! positives with no cheap way to narrow it down. A future version could
//! add an explicit user-configured mapping (hostname -> forge kind) for
//! this case; that is out of scope for T-243.
//!
//! Any remote whose host does not match one of the above is simply not
//! detected (`detect_forge` returns `None`) — per US-101 criterion 3, this
//! must never be treated as an error: local Git functionality never
//! depends on forge detection, and GitSail never invents a link for a
//! remote it cannot identify (see `github-gitlab-integration-rules` in the
//! project wiki).
//!
//! ## Security model of `build_web_url` (read before touching this file)
//!
//! [`build_web_url`] is the *only* function in GitSail that turns a forge
//! remote plus a caller-chosen destination into a URL that a presentation
//! layer may open in a browser. Its safety rests on two structural
//! invariants:
//!
//! 1. **The scheme and host are never influenced by caller-supplied path
//!    data.** The returned URL is always built as `https://<host>/...`,
//!    where `<host>` comes only from parsing the repository's own
//!    configured remote (never from a branch name, file path, or any other
//!    caller-controlled string) and the scheme is always the literal
//!    `https`, regardless of whether the remote itself used `ssh://`,
//!    `git@`, `http://`, or `git://`. There is no code path through which a
//!    [`ForgePath`] variant can change the scheme or host.
//! 2. **Every path component is a validated domain type, never a raw
//!    string the caller assembles.** [`ForgePath`] only accepts
//!    [`crate::ids::BranchName`] and [`crate::ids::CommitHash`] — a caller
//!    cannot pass an arbitrary path/query/fragment. `CommitHash` already
//!    guarantees a hex-only string, which is inherently safe as a URL path
//!    segment. `BranchName` only guarantees non-emptiness (Git itself
//!    allows ref names a browser would treat specially, and these remain
//!    reachable through plumbing even where porcelain like `git branch`
//!    would refuse them — see `gitsail_domain::sanitize`'s doc comment for
//!    the same caveat applied to rendering), so this module additionally:
//!    - Splits a branch name on `/` and pushes each resulting piece as its
//!      own URL path segment via [`url::Url::path_segments_mut`], which
//!      percent-encodes reserved characters (`?`, `#`, `%`, ...) within a
//!      segment. This also happens to do the right *functional* thing for
//!      branch names that legitimately contain `/` (e.g. `feature/foo`),
//!      since GitHub/GitLab both resolve such branches from literal
//!      multi-segment tree/commit URLs.
//!    - Additionally neutralizes any segment made up **only** of `.`
//!      characters (`.`, `..`, `...`, ...) by percent-encoding those dots
//!      by hand *before* handing the segment to `url`. This matters
//!      because RFC 3986 §5.2.4 dot-segment removal — which many HTTP
//!      clients, proxies and browsers still apply even to an
//!      already-absolute URI — operates on the literal ASCII syntax `.`/
//!      `..`, not on what a segment decodes to; a segment that is already
//!      percent-encoded (`url`'s own segment encoder then additionally
//!      encodes the literal `%` this module inserted, so the dots end up
//!      spelled `%252E`) is not special to that algorithm, so it can never
//!      be collapsed into a `../../` path traversal out of the
//!      `owner/repo/...` prefix this function always emits first.
//!
//! The mandatory malicious-input fixtures in this module's tests exercise
//! exactly these two invariants.

use url::Url;

use crate::ids::{BranchName, CommitHash};
use crate::remote::RemoteUrl;

/// A forge GitSail knows how to build web links for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForgeKind {
    GitHub,
    GitLab,
}

/// A validated destination inside a detected forge's web UI.
///
/// This is intentionally the *only* way [`build_web_url`] accepts "where in
/// the forge to link to": every variant either carries nothing or an
/// already-validated domain type. See this module's doc comment for why
/// that is a hard security requirement here, not just a style preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgePath {
    /// The repository's own root page.
    Repository,
    /// A branch's tree/files view.
    Branch(BranchName),
    /// A single commit's view.
    Commit(CommitHash),
}

/// A remote URL parsed down to what forge detection/link-building need:
/// the host, and the repository's path segments (owner, any GitLab
/// subgroups, repo — `.git` suffix already stripped from the last one).
struct RemoteLocation {
    host: String,
    segments: Vec<String>,
}

/// Parses the authority (`[user[:pass]@]host[:port]`) plus path out of a
/// remote URL that already had its scheme (or scp-like prefix) stripped.
fn parse_authority_and_path(rest: &str) -> Option<RemoteLocation> {
    let after_userinfo = match rest.rsplit_once('@') {
        // An '@' could also appear inside the path (rare, but a
        // credential-free repo path never contains one before the first
        // '/', so scanning up to the first '/' first would be safer);
        // guard by only stripping user-info found before the first '/'.
        Some((userinfo, host_and_path)) if !userinfo.contains('/') => host_and_path,
        _ => rest,
    };
    let (host_port, path) = after_userinfo.split_once('/')?;
    let host = host_port.split(':').next().unwrap_or(host_port);
    build_location(host, path)
}

fn build_location(host: &str, path: &str) -> Option<RemoteLocation> {
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    let trimmed_path = path.trim_matches('/');
    let mut segments: Vec<String> = trimmed_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    // Need at least an owner (or group/subgroup) and a repo name.
    if segments.len() < 2 {
        return None;
    }
    if let Some(last) = segments.last_mut() {
        if let Some(stripped) = last.strip_suffix(".git") {
            *last = stripped.to_string();
        }
    }
    if segments.iter().any(|s| s.is_empty()) {
        return None;
    }
    Some(RemoteLocation {
        host: host.to_string(),
        segments,
    })
}

/// Parses any of the remote URL shapes Git itself accepts for a forge
/// remote: `https://`/`http://` URLs, `ssh://` URLs, and the scp-like
/// `[user@]host:path` syntax (`git@github.com:org/repo.git`).
fn parse_remote(remote_url: &RemoteUrl) -> Option<RemoteLocation> {
    let raw = remote_url.as_str().trim();

    if let Some(rest) = raw
        .strip_prefix("ssh://")
        .or_else(|| raw.strip_prefix("git://"))
        .or_else(|| raw.strip_prefix("https://"))
        .or_else(|| raw.strip_prefix("http://"))
    {
        return parse_authority_and_path(rest);
    }

    // scp-like syntax has no "scheme://" at all.
    if !raw.contains("://") {
        let colon = raw.find(':')?;
        // A Windows-style drive path ("C:\...") is not a scp-like remote;
        // scp-like syntax always has a host (possibly with a user@) before
        // the colon, never a single letter.
        let (authority, rest) = raw.split_at(colon);
        if authority.len() <= 1 {
            return None;
        }
        let path = rest.get(1..)?;
        let host = authority.rsplit('@').next().unwrap_or(authority);
        return build_location(host, path);
    }

    None
}

/// Recognizes a known forge from its hostname alone. See this module's doc
/// comment for the exact, deliberately narrow scope.
fn forge_for_host(host: &str) -> Option<ForgeKind> {
    let host = host.to_ascii_lowercase();
    match host.as_str() {
        "github.com" | "www.github.com" => Some(ForgeKind::GitHub),
        "gitlab.com" | "www.gitlab.com" => Some(ForgeKind::GitLab),
        _ => {
            let mut labels = host.split('.');
            let first = labels.next().unwrap_or("");
            let has_more_labels = labels.next().is_some();
            if first == "gitlab" && has_more_labels {
                Some(ForgeKind::GitLab)
            } else {
                None
            }
        }
    }
}

/// Detects which forge (if any) `remote_url` points at, from its hostname.
///
/// Returns `None` for any remote GitSail cannot confidently identify —
/// this is the normal case for most remotes (a bare Git server, a
/// self-hosted Forgejo/Gitea instance, a local filesystem path, ...) and
/// callers must treat it as "no browser link available", never as an
/// error (US-101 criterion 3).
pub fn detect_forge(remote_url: &RemoteUrl) -> Option<ForgeKind> {
    let location = parse_remote(remote_url)?;
    forge_for_host(&location.host)
}

/// Splits a branch name into the path segments its web URL should use,
/// neutralizing dot-only segments so they can never be collapsed by a
/// downstream dot-segment-removal pass. See this module's top doc comment
/// for the full rationale.
fn safe_ref_segments(name: &str) -> Vec<String> {
    name.split('/')
        .map(|segment| {
            if !segment.is_empty() && segment.bytes().all(|b| b == b'.') {
                segment.replace('.', "%2E")
            } else {
                segment.to_string()
            }
        })
        .collect()
}

/// Builds the web (browser) URL for `path` inside `remote_url`'s
/// repository, once the caller already knows (via [`detect_forge`]) that
/// `remote_url` belongs to `forge`.
///
/// Returns `None` when `remote_url` cannot be parsed as a `forge` remote at
/// all (including when it turns out to belong to a *different* forge than
/// the one asserted) — this function never falls back to guessing or to a
/// partially-built URL.
pub fn build_web_url(forge: ForgeKind, remote_url: &RemoteUrl, path: ForgePath) -> Option<Url> {
    let location = parse_remote(remote_url)?;
    if forge_for_host(&location.host)? != forge {
        return None;
    }

    // Scheme and host are fixed literally here — nothing below this line
    // can ever influence either.
    let mut url = Url::parse(&format!("https://{}", location.host)).ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        for segment in &location.segments {
            segments.push(segment);
        }
        match path {
            ForgePath::Repository => {}
            ForgePath::Branch(name) => {
                match forge {
                    ForgeKind::GitHub => {
                        segments.push("tree");
                    }
                    ForgeKind::GitLab => {
                        segments.push("-").push("tree");
                    }
                };
                for part in safe_ref_segments(name.as_str()) {
                    segments.push(&part);
                }
            }
            ForgePath::Commit(hash) => {
                match forge {
                    ForgeKind::GitHub => {
                        segments.push("commit");
                    }
                    ForgeKind::GitLab => {
                        segments.push("-").push("commit");
                    }
                };
                segments.push(hash.as_str());
            }
        }
    }
    Some(url)
}

/// Resolves `remote_url`'s forge host and repository path segments (owner,
/// any GitLab subgroups, repo — `.git` suffix already stripped), once the
/// caller already knows (via [`detect_forge`]) that `remote_url` belongs to
/// `forge`.
///
/// This is the same validated parse [`build_web_url`] uses internally,
/// exposed directly for callers that need the raw host/path rather than a
/// browser URL — e.g. T-245/US-103's PR/MR listing, which needs to build a
/// forge *API* URL (`api.github.com/repos/<owner>/<repo>/...`, `<host>/api/
/// v4/projects/<path>/...`), not a web UI link. Kept in this module (rather
/// than duplicated in `gitsail-forge`) so there is exactly one place that
/// parses a remote URL's authority/path and validates it against a claimed
/// [`ForgeKind`] (see this module's top doc comment for why that parse is
/// itself the security-sensitive part).
///
/// Returns `None` under the exact same conditions as [`build_web_url`]:
/// `remote_url` cannot be parsed as a remote at all, or it resolves to a
/// *different* forge than `forge` asserts.
pub fn repository_location(forge: ForgeKind, remote_url: &RemoteUrl) -> Option<(String, Vec<String>)> {
    let location = parse_remote(remote_url)?;
    if forge_for_host(&location.host)? != forge {
        return None;
    }
    Some((location.host, location.segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> RemoteUrl {
        RemoteUrl::new(s)
    }

    // -- detect_forge: recognized shapes --------------------------------

    #[test]
    fn detects_github_https() {
        assert_eq!(
            detect_forge(&url("https://github.com/org/repo.git")),
            Some(ForgeKind::GitHub)
        );
        assert_eq!(
            detect_forge(&url("https://github.com/org/repo")),
            Some(ForgeKind::GitHub)
        );
    }

    #[test]
    fn detects_github_ssh_scp_like() {
        assert_eq!(
            detect_forge(&url("git@github.com:org/repo.git")),
            Some(ForgeKind::GitHub)
        );
    }

    #[test]
    fn detects_github_ssh_url_form() {
        assert_eq!(
            detect_forge(&url("ssh://git@github.com/org/repo.git")),
            Some(ForgeKind::GitHub)
        );
        assert_eq!(
            detect_forge(&url("ssh://git@github.com:22/org/repo.git")),
            Some(ForgeKind::GitHub)
        );
    }

    #[test]
    fn detects_gitlab_https_and_ssh() {
        assert_eq!(
            detect_forge(&url("https://gitlab.com/group/repo.git")),
            Some(ForgeKind::GitLab)
        );
        assert_eq!(
            detect_forge(&url("git@gitlab.com:group/repo.git")),
            Some(ForgeKind::GitLab)
        );
    }

    #[test]
    fn detects_gitlab_nested_subgroups() {
        assert_eq!(
            detect_forge(&url("https://gitlab.com/group/subgroup/repo.git")),
            Some(ForgeKind::GitLab)
        );
    }

    #[test]
    fn detects_self_hosted_gitlab_by_hostname_convention() {
        assert_eq!(
            detect_forge(&url("https://gitlab.example.com/group/repo.git")),
            Some(ForgeKind::GitLab)
        );
        assert_eq!(
            detect_forge(&url("git@gitlab.mycompany.io:group/repo.git")),
            Some(ForgeKind::GitLab)
        );
    }

    #[test]
    fn does_not_guess_self_hosted_github_or_generic_hosts() {
        assert_eq!(
            detect_forge(&url("https://git.example.com/org/repo.git")),
            None
        );
        assert_eq!(
            detect_forge(&url("https://github.example.com/org/repo.git")),
            None
        );
    }

    #[test]
    fn unknown_remote_is_not_detected_not_an_error() {
        assert_eq!(
            detect_forge(&url("https://example.com/some/path.git")),
            None
        );
        assert_eq!(detect_forge(&url("/local/bare/repo.git")), None);
        assert_eq!(detect_forge(&url("not a url at all")), None);
    }

    // -- build_web_url: normalization to the web form --------------------

    #[test]
    fn builds_repository_link_from_https_remote() {
        let remote = url("https://github.com/org/repo.git");
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Repository).unwrap();
        assert_eq!(built.as_str(), "https://github.com/org/repo");
    }

    #[test]
    fn builds_repository_link_from_ssh_remote() {
        let remote = url("git@github.com:org/repo.git");
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Repository).unwrap();
        assert_eq!(built.as_str(), "https://github.com/org/repo");
    }

    #[test]
    fn builds_gitlab_nested_subgroup_repository_link() {
        let remote = url("git@gitlab.com:group/subgroup/repo.git");
        let built = build_web_url(ForgeKind::GitLab, &remote, ForgePath::Repository).unwrap();
        assert_eq!(built.as_str(), "https://gitlab.com/group/subgroup/repo");
    }

    #[test]
    fn builds_github_branch_link() {
        let remote = url("https://github.com/org/repo.git");
        let branch = BranchName::new("main").unwrap();
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Branch(branch)).unwrap();
        assert_eq!(built.as_str(), "https://github.com/org/repo/tree/main");
    }

    #[test]
    fn builds_gitlab_branch_link_with_dash_segment() {
        let remote = url("https://gitlab.com/group/repo.git");
        let branch = BranchName::new("main").unwrap();
        let built = build_web_url(ForgeKind::GitLab, &remote, ForgePath::Branch(branch)).unwrap();
        assert_eq!(built.as_str(), "https://gitlab.com/group/repo/-/tree/main");
    }

    #[test]
    fn builds_branch_link_preserving_slash_containing_branch_name() {
        let remote = url("https://github.com/org/repo.git");
        let branch = BranchName::new("feature/nice-thing").unwrap();
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Branch(branch)).unwrap();
        assert_eq!(
            built.as_str(),
            "https://github.com/org/repo/tree/feature/nice-thing"
        );
    }

    #[test]
    fn builds_commit_links() {
        let remote = url("https://github.com/org/repo.git");
        let hash = CommitHash::new("deadbeefcafefeed").unwrap();
        let built =
            build_web_url(ForgeKind::GitHub, &remote, ForgePath::Commit(hash.clone())).unwrap();
        assert_eq!(
            built.as_str(),
            "https://github.com/org/repo/commit/deadbeefcafefeed"
        );

        let remote = url("https://gitlab.com/group/repo.git");
        let built = build_web_url(ForgeKind::GitLab, &remote, ForgePath::Commit(hash)).unwrap();
        assert_eq!(
            built.as_str(),
            "https://gitlab.com/group/repo/-/commit/deadbeefcafefeed"
        );
    }

    #[test]
    fn mismatched_forge_kind_yields_none() {
        let remote = url("https://gitlab.com/group/repo.git");
        assert!(build_web_url(ForgeKind::GitHub, &remote, ForgePath::Repository).is_none());
    }

    #[test]
    fn unrecognized_remote_yields_no_link_and_never_panics() {
        let remote = url("https://example.com/some/path.git");
        assert!(build_web_url(ForgeKind::GitHub, &remote, ForgePath::Repository).is_none());
        assert!(build_web_url(ForgeKind::GitLab, &remote, ForgePath::Repository).is_none());
    }

    // -- Mandatory malicious-input fixtures (T-243) ----------------------
    //
    // A branch name is git-plumbing-reachable free text (see this module's
    // and `gitsail_domain::sanitize`'s doc comments): each of these proves
    // that content cannot escape the `https://<forge-host>/...` prefix,
    // inject a query/fragment, or override the scheme.

    #[test]
    fn malicious_branch_name_cannot_inject_a_query_string() {
        let remote = url("https://github.com/org/repo.git");
        let branch = BranchName::new("main?redirect=https://evil.example").unwrap();
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Branch(branch)).unwrap();

        assert_eq!(built.scheme(), "https");
        assert_eq!(built.host_str(), Some("github.com"));
        assert_eq!(built.query(), None, "no query must be introduced");
        assert!(built.as_str().starts_with("https://github.com/org/repo/tree/"));
        // The literal branch text is still present (nothing is silently
        // dropped) but only as inert, percent-encoded path content — never
        // as an actual `?query=value` on the URL.
        assert!(!built.as_str().contains('?'), "no literal '?' must survive");
    }

    #[test]
    fn malicious_branch_name_cannot_inject_a_fragment() {
        let remote = url("https://github.com/org/repo.git");
        let branch = BranchName::new("main#/evil/redirect").unwrap();
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Branch(branch)).unwrap();

        assert_eq!(built.scheme(), "https");
        assert_eq!(built.host_str(), Some("github.com"));
        assert_eq!(built.fragment(), None, "no fragment must be introduced");
        assert!(built.as_str().starts_with("https://github.com/org/repo/tree/"));
    }

    #[test]
    fn malicious_branch_name_cannot_path_traverse_out_of_the_repo() {
        let remote = url("https://github.com/org/repo.git");
        let branch = BranchName::new("../../../evil").unwrap();
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Branch(branch)).unwrap();

        assert_eq!(built.scheme(), "https");
        assert_eq!(built.host_str(), Some("github.com"));
        // The generated URL must still literally begin with the repo's own
        // prefix: the ".." segments must show up as opaque, encoded text
        // *after* it, never having collapsed it away.
        assert!(
            built.as_str().starts_with("https://github.com/org/repo/tree/"),
            "got {built}"
        );
        assert!(!built.path().contains("/../"));
        // The dot segments are still there, just inert/encoded — not
        // collapsed away, and not literally interpretable as ".." by a
        // path-segment-aware normalizer.
        // `url`'s own path-segment percent-encoding additionally encodes
        // the `%` this module inserted by hand (`.` -> `%2E`), yielding
        // `%252E` — i.e. the segment now requires *two* rounds of percent
        // decoding to ever read back as `..`, which is stronger than
        // strictly necessary but still exactly the required property:
        // opaque to any single-pass dot-segment-removal algorithm.
        let segments: Vec<&str> = built.path_segments().unwrap().collect();
        assert_eq!(
            segments,
            vec!["org", "repo", "tree", "%252E%252E", "%252E%252E", "%252E%252E", "evil"]
        );

        // Even a strict RFC 3986 dot-segment-removal pass re-parsing this
        // exact string must not collapse anything either, because the
        // dots are already percent-encoded rather than literal: the full
        // 7-segment path survives unchanged, still anchored at org/repo.
        let reparsed = Url::parse(built.as_str()).unwrap();
        assert_eq!(reparsed.host_str(), Some("github.com"));
        let reparsed_segments: Vec<&str> = reparsed.path_segments().unwrap().collect();
        assert_eq!(reparsed_segments, segments);
    }

    #[test]
    fn malicious_branch_name_cannot_override_the_scheme() {
        let remote = url("https://github.com/org/repo.git");
        // Not a realistic Git ref (Git itself would reject embedded
        // whitespace/control chars at the porcelain layer), but the
        // *mechanism* under test is that no part of `ForgePath` is ever
        // interpolated into the scheme/host — only into path segments —
        // so this can never become a `javascript:`/`data:`/`file:` link
        // regardless of its content.
        let branch = BranchName::new("javascript:alert(1)//evil").unwrap();
        let built = build_web_url(ForgeKind::GitHub, &remote, ForgePath::Branch(branch)).unwrap();

        assert_eq!(built.scheme(), "https");
        assert_eq!(built.host_str(), Some("github.com"));
    }

    #[test]
    fn malicious_branch_name_with_backslash_traversal_is_neutralized() {
        let remote = url("https://gitlab.com/group/repo.git");
        let branch = BranchName::new("..\\..\\windows\\system32").unwrap();
        let built = build_web_url(ForgeKind::GitLab, &remote, ForgePath::Branch(branch)).unwrap();

        assert_eq!(built.scheme(), "https");
        assert_eq!(built.host_str(), Some("gitlab.com"));
        assert!(built
            .as_str()
            .starts_with("https://gitlab.com/group/repo/-/tree/"));
    }

    #[test]
    fn all_forge_kinds_have_distinct_variants() {
        assert_ne!(ForgeKind::GitHub, ForgeKind::GitLab);
    }

    // -- repository_location (T-245/US-103) -------------------------------

    #[test]
    fn repository_location_resolves_host_and_segments_for_github() {
        let remote = url("git@github.com:org/repo.git");
        let (host, segments) = repository_location(ForgeKind::GitHub, &remote).unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(segments, vec!["org".to_string(), "repo".to_string()]);
    }

    #[test]
    fn repository_location_resolves_nested_subgroups_for_gitlab() {
        let remote = url("https://gitlab.example.com/group/subgroup/repo.git");
        let (host, segments) = repository_location(ForgeKind::GitLab, &remote).unwrap();
        assert_eq!(host, "gitlab.example.com");
        assert_eq!(
            segments,
            vec!["group".to_string(), "subgroup".to_string(), "repo".to_string()]
        );
    }

    #[test]
    fn repository_location_is_none_for_mismatched_or_unrecognized_forge() {
        let remote = url("https://gitlab.com/group/repo.git");
        assert!(repository_location(ForgeKind::GitHub, &remote).is_none());

        let unrecognized = url("https://example.com/some/path.git");
        assert!(repository_location(ForgeKind::GitHub, &unrecognized).is_none());
        assert!(repository_location(ForgeKind::GitLab, &unrecognized).is_none());
    }
}
