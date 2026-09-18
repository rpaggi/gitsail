//! Computes a forge web link for the currently open repository's remotes
//! (T-243/US-101), so presentation layers (TUI/CLI/Desktop) share one
//! "which remote do we use, which forge is it" policy instead of each
//! reimplementing it.
//!
//! This is pure composition over [`gitsail_domain::forge`] and an
//! already-fetched `Vec<Remote>` (from
//! [`crate::ports::RepositoryReadPort::list_remotes`]) — no port, no I/O —
//! which is why, unlike every other use case in this crate,
//! [`GetForgeLink`] holds nothing and its `execute` takes no `&self`.

use gitsail_domain::{build_web_url, detect_forge, ForgeKind, ForgePath, Remote};

/// Picks which of `remotes` a forge link should be built from.
///
/// Policy: prefer a remote literally named `origin` when its URL resolves
/// to a known forge; otherwise use the first remote (in the given order)
/// that does. Returns `None` when no remote is recognized at all — this is
/// the normal "no browser link available" case (US-101 criterion 3), never
/// an error.
pub fn pick_forge_remote(remotes: &[Remote]) -> Option<(&Remote, ForgeKind)> {
    if let Some(origin) = remotes.iter().find(|r| r.name == "origin") {
        if let Some(kind) = detect_forge(&origin.fetch_url) {
            return Some((origin, kind));
        }
    }
    remotes
        .iter()
        .find_map(|r| detect_forge(&r.fetch_url).map(|kind| (r, kind)))
}

/// Builds the browser-openable web link for `path` in whichever remote
/// [`pick_forge_remote`] selects out of `remotes`.
pub struct GetForgeLink;

impl GetForgeLink {
    /// Returns `None` when no remote is recognized, matching
    /// [`pick_forge_remote`] — a caller (e.g. a TUI action or a Tauri
    /// command) uses this to decide whether an "open in browser" action is
    /// even offered, never to fail an otherwise-successful Git read.
    pub fn execute(remotes: &[Remote], path: ForgePath) -> Option<String> {
        let (remote, kind) = pick_forge_remote(remotes)?;
        build_web_url(kind, &remote.fetch_url, path).map(|url| url.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{BranchName, RemoteUrl};

    fn remote(name: &str, url: &str) -> Remote {
        Remote {
            name: name.to_string(),
            fetch_url: RemoteUrl::new(url),
            push_url: RemoteUrl::new(url),
        }
    }

    #[test]
    fn prefers_origin_when_it_is_a_recognized_forge() {
        let remotes = vec![
            remote("upstream", "https://gitlab.com/upstream/repo.git"),
            remote("origin", "https://github.com/me/repo.git"),
        ];
        let (picked, kind) = pick_forge_remote(&remotes).unwrap();
        assert_eq!(picked.name, "origin");
        assert_eq!(kind, ForgeKind::GitHub);
    }

    #[test]
    fn falls_back_to_first_recognized_remote_when_origin_is_unrecognized() {
        let remotes = vec![
            remote("origin", "https://internal.example.com/team/repo.git"),
            remote("upstream", "https://gitlab.com/upstream/repo.git"),
        ];
        let (picked, kind) = pick_forge_remote(&remotes).unwrap();
        assert_eq!(picked.name, "upstream");
        assert_eq!(kind, ForgeKind::GitLab);
    }

    #[test]
    fn no_recognized_remote_yields_none_not_an_error() {
        let remotes = vec![remote("origin", "https://internal.example.com/team/repo.git")];
        assert!(pick_forge_remote(&remotes).is_none());
        assert!(GetForgeLink::execute(&remotes, ForgePath::Repository).is_none());
    }

    #[test]
    fn empty_remote_list_yields_none() {
        assert!(GetForgeLink::execute(&[], ForgePath::Repository).is_none());
    }

    #[test]
    fn builds_the_full_link_end_to_end() {
        let remotes = vec![remote("origin", "git@github.com:org/repo.git")];
        let link = GetForgeLink::execute(
            &remotes,
            ForgePath::Branch(BranchName::new("main").unwrap()),
        )
        .unwrap();
        assert_eq!(link, "https://github.com/org/repo/tree/main");
    }
}
