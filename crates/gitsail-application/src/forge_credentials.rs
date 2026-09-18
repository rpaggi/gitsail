//! Forge account authorization (SAD/EPIC-20/EPIC-22, T-244/US-102): "quero
//! conectar minha conta de forge opcionalmente, para consultar PRs/MRs
//! privados com controle."
//!
//! ## Scope of this task (documented decision)
//!
//! This module implements exactly:
//! - [`ForgeCredentialPort`] — store/retrieve/remove a token, and report
//!   connection status, per forge account, backed by OS-secure storage.
//! - Thin use cases wrapping it: [`ConnectForgeAccount`],
//!   [`DisconnectForgeAccount`], [`GetForgeConnectionStatus`].
//!
//! It deliberately does **not** implement:
//! - Any real GitHub/GitLab API call. Nothing here validates a token
//!   against the live API, and nothing here lists PRs/MRs — that is
//!   T-245, out of scope for this task. [`ConnectForgeAccount::execute`]
//!   is a "trust the user, store this" operation: it stores whatever
//!   token it is given without reaching out to the network. The task
//!   description explicitly allows this scope cut for v1.0 — T-245's
//!   first real API call is exactly where an invalid/expired/wrong-scope
//!   token would surface, and this port's job stops at "hold the secret
//!   safely and say whether one is currently held".
//! - Any specific "connect account" UI flow. TUI/CLI/Desktop each wire
//!   this port into their own presentation, following the same pattern
//!   already used for `RecentRepositoriesPort` (a port defined here, a
//!   concrete adapter per frontend/binary).
//!
//! ## Token scope GitSail requests (documented decision)
//!
//! v1.0 only ever needs **read** access to pull/merge requests:
//! - GitHub: a fine-grained personal access token with repository
//!   permission "Pull requests: Read-only" (a classic PAT's `repo` scope,
//!   or `public_repo` for public repositories only, covers the same read
//!   paths if fine-grained tokens are not in use).
//! - GitLab: a personal/project/group access token with the `read_api`
//!   scope (the narrower `read_repository` scope is not enough, since
//!   MR/PR listing is served through the API, not the Git transport).
//!
//! No write scope (creating, merging, or commenting on a PR/MR) is ever
//! requested in v1.0. A future capability that needs one (T-246, if/when
//! it ships) must ask for it separately and say so explicitly to the user
//! before requesting it — it must never silently piggyback on the
//! read-only token this module stores.
//!
//! ## Why local Git never depends on this (US-102 criterion 3)
//!
//! [`ForgeCredentialPort`] is a wholly separate port from
//! [`crate::ports::RepositoryReadPort`] and
//! [`crate::write_ports::RepositoryWritePort`]: no read or mutation use
//! case takes or consults it, and nothing in this module touches either of
//! those ports either. A missing account, an expired token, or a
//! credential-store failure can only ever affect the (not-yet-implemented)
//! forge-query capability itself — never `status`, `log`, a commit, a
//! branch operation, or anything else Git. This matches the project wiki's
//! `security-privacy-credentials-rules`: "GitSail must work entirely on
//! local repositories... no account is required for v1.0."
//!
//! [`GetForgeConnectionStatus::execute`] goes one step further and folds
//! any credential-store error into "not connected" rather than
//! propagating it, because the only thing a connection status feeds is
//! "does the UI offer connect or disconnect" — never a Git operation — so
//! there is no reason for a keyring hiccup to become a hard error a caller
//! must handle.

use std::fmt;
use std::sync::Arc;

use gitsail_domain::{ForgeKind, GitSailError};

/// Identifies one connectable forge account: which forge, and which host.
///
/// The host is part of the identity (not just the [`ForgeKind`]) so two
/// different self-hosted GitLab instances — or a self-hosted instance and
/// gitlab.com itself — never collide in storage.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ForgeAccountId {
    pub kind: ForgeKind,
    /// Lowercased hostname, e.g. `github.com`, `gitlab.example.com`.
    host: String,
}

impl ForgeAccountId {
    pub fn new(kind: ForgeKind, host: impl Into<String>) -> Self {
        Self {
            kind,
            host: host.into().to_ascii_lowercase(),
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// A single opaque string identifying this account for a credential
    /// store's own key/service naming (`github@host` / `gitlab@host`).
    /// Never itself a secret — safe to log, unlike [`ForgeToken`].
    pub fn storage_key(&self) -> String {
        let kind = match self.kind {
            ForgeKind::GitHub => "github",
            ForgeKind::GitLab => "gitlab",
        };
        format!("{kind}@{}", self.host)
    }
}

/// An opaque forge access token (a PAT or OAuth token).
///
/// Like [`gitsail_domain::remote::RemoteUrl`], `Debug` is implemented by
/// hand (never derived) so a stray `{:?}` in a log line or a `panic!`
/// message can never leak the raw secret — this is on top of, not instead
/// of, the actual rule: no GitSail code path is meant to log a
/// [`ForgeToken`] at all.
#[derive(Clone, PartialEq, Eq)]
pub struct ForgeToken(String);

impl ForgeToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The raw token, for the one legitimate use: handing it to an
    /// outgoing forge API request (T-245) or to a credential-store
    /// adapter's write call. Named loudly so a call site reads as an
    /// explicit decision, not an accident.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ForgeToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ForgeToken(***)")
    }
}

/// Whether a [`ForgeAccountId`] currently has a token stored, without
/// exposing the token itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeConnectionStatus {
    Connected,
    NotConnected,
}

/// Stores/retrieves forge access tokens in OS-secure storage (US-102
/// criterion 1). Implemented by an adapter outside this crate (e.g.
/// `gitsail-forge`'s `keyring`-backed store for production, or an
/// in-memory double for tests) — this crate only defines the contract.
pub trait ForgeCredentialPort: Send + Sync {
    /// Stores `token` for `account`, overwriting any token already stored
    /// for it. Does not contact the forge's API (see module docs): this
    /// is a "trust the user, store this" operation.
    fn connect(&self, account: &ForgeAccountId, token: ForgeToken) -> Result<(), GitSailError>;

    /// Removes any token stored for `account` — an explicit "disconnect"
    /// (US-102 criterion 2). Idempotent: disconnecting an account with no
    /// stored token is not an error.
    fn disconnect(&self, account: &ForgeAccountId) -> Result<(), GitSailError>;

    /// Whether `account` currently has a token stored.
    fn status(&self, account: &ForgeAccountId) -> Result<ForgeConnectionStatus, GitSailError>;

    /// The stored token for `account`, if any — for a future capability
    /// (T-245) to attach to an outgoing forge API request. Callers must
    /// never log the returned value; [`ForgeToken`]'s own `Debug`
    /// implementation is the last line of defense, not the only one.
    fn token(&self, account: &ForgeAccountId) -> Result<Option<ForgeToken>, GitSailError>;
}

/// Connects a forge account: an explicit, user-initiated action (US-102
/// criterion 2) — never something GitSail does on its own.
pub struct ConnectForgeAccount {
    port: Arc<dyn ForgeCredentialPort>,
}

impl ConnectForgeAccount {
    pub fn new(port: Arc<dyn ForgeCredentialPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, account: &ForgeAccountId, token: ForgeToken) -> Result<(), GitSailError> {
        self.port.connect(account, token)
    }
}

/// Disconnects a forge account: an explicit, user-initiated action that
/// removes local access (US-102 criterion 2) — it deletes the token from
/// the credential store, it does not merely clear an in-memory cache.
pub struct DisconnectForgeAccount {
    port: Arc<dyn ForgeCredentialPort>,
}

impl DisconnectForgeAccount {
    pub fn new(port: Arc<dyn ForgeCredentialPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, account: &ForgeAccountId) -> Result<(), GitSailError> {
        self.port.disconnect(account)
    }
}

/// Reports whether a forge account is connected, never as a hard error
/// (US-102 criterion 3 — see module docs).
pub struct GetForgeConnectionStatus {
    port: Arc<dyn ForgeCredentialPort>,
}

impl GetForgeConnectionStatus {
    pub fn new(port: Arc<dyn ForgeCredentialPort>) -> Self {
        Self { port }
    }

    /// Folds any credential-store failure into [`ForgeConnectionStatus::NotConnected`]
    /// rather than propagating it: a keyring/OS-storage hiccup must never
    /// look like a reason to block anything else in the UI.
    pub fn execute(&self, account: &ForgeAccountId) -> ForgeConnectionStatus {
        self.port
            .status(account)
            .unwrap_or(ForgeConnectionStatus::NotConnected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A minimal in-memory [`ForgeCredentialPort`] double for this
    /// module's own unit tests (mirrors the `InMemoryRecents` pattern used
    /// for `RecentRepositoriesPort` in `apps/desktop/src-tauri`). The
    /// reusable double other crates wire into their own tests lives in
    /// `gitsail-forge` (`InMemoryForgeCredentialStore`), alongside the
    /// real `keyring`-backed adapter it stands in for.
    #[derive(Default)]
    struct InMemoryPort {
        tokens: Mutex<HashMap<ForgeAccountId, ForgeToken>>,
    }

    impl ForgeCredentialPort for InMemoryPort {
        fn connect(&self, account: &ForgeAccountId, token: ForgeToken) -> Result<(), GitSailError> {
            self.tokens.lock().unwrap().insert(account.clone(), token);
            Ok(())
        }

        fn disconnect(&self, account: &ForgeAccountId) -> Result<(), GitSailError> {
            self.tokens.lock().unwrap().remove(account);
            Ok(())
        }

        fn status(&self, account: &ForgeAccountId) -> Result<ForgeConnectionStatus, GitSailError> {
            Ok(if self.tokens.lock().unwrap().contains_key(account) {
                ForgeConnectionStatus::Connected
            } else {
                ForgeConnectionStatus::NotConnected
            })
        }

        fn token(&self, account: &ForgeAccountId) -> Result<Option<ForgeToken>, GitSailError> {
            Ok(self.tokens.lock().unwrap().get(account).cloned())
        }
    }

    struct AlwaysFailingPort;

    impl ForgeCredentialPort for AlwaysFailingPort {
        fn connect(&self, _: &ForgeAccountId, _: ForgeToken) -> Result<(), GitSailError> {
            Err(GitSailError::new(
                gitsail_domain::ErrorCode::Internal,
                "boom",
            ))
        }
        fn disconnect(&self, _: &ForgeAccountId) -> Result<(), GitSailError> {
            Err(GitSailError::new(
                gitsail_domain::ErrorCode::Internal,
                "boom",
            ))
        }
        fn status(&self, _: &ForgeAccountId) -> Result<ForgeConnectionStatus, GitSailError> {
            Err(GitSailError::new(
                gitsail_domain::ErrorCode::Internal,
                "boom",
            ))
        }
        fn token(&self, _: &ForgeAccountId) -> Result<Option<ForgeToken>, GitSailError> {
            Err(GitSailError::new(
                gitsail_domain::ErrorCode::Internal,
                "boom",
            ))
        }
    }

    fn github_account() -> ForgeAccountId {
        ForgeAccountId::new(ForgeKind::GitHub, "GitHub.com")
    }

    #[test]
    fn account_id_lowercases_and_namespaces_the_storage_key() {
        let account = github_account();
        assert_eq!(account.host(), "github.com");
        assert_eq!(account.storage_key(), "github@github.com");

        let gitlab = ForgeAccountId::new(ForgeKind::GitLab, "gitlab.example.com");
        assert_eq!(gitlab.storage_key(), "gitlab@gitlab.example.com");
        assert_ne!(account.storage_key(), gitlab.storage_key());
    }

    #[test]
    fn connect_then_status_reports_connected() {
        let port: Arc<dyn ForgeCredentialPort> = Arc::new(InMemoryPort::default());
        let account = github_account();

        assert_eq!(
            GetForgeConnectionStatus::new(port.clone()).execute(&account),
            ForgeConnectionStatus::NotConnected
        );

        ConnectForgeAccount::new(port.clone())
            .execute(&account, ForgeToken::new("sentinel-fake-token-abc123"))
            .unwrap();

        assert_eq!(
            GetForgeConnectionStatus::new(port.clone()).execute(&account),
            ForgeConnectionStatus::Connected
        );
    }

    /// US-102 criterion 2: disconnecting removes local access — the token
    /// is actually gone from the store, not just uncached.
    #[test]
    fn disconnect_removes_the_token_from_storage() {
        let port: Arc<dyn ForgeCredentialPort> = Arc::new(InMemoryPort::default());
        let account = github_account();
        ConnectForgeAccount::new(port.clone())
            .execute(&account, ForgeToken::new("sentinel-fake-token-abc123"))
            .unwrap();

        DisconnectForgeAccount::new(port.clone())
            .execute(&account)
            .unwrap();

        assert_eq!(
            GetForgeConnectionStatus::new(port.clone()).execute(&account),
            ForgeConnectionStatus::NotConnected
        );
        assert_eq!(port.token(&account).unwrap(), None);
    }

    #[test]
    fn disconnecting_an_account_with_no_token_is_not_an_error() {
        let port: Arc<dyn ForgeCredentialPort> = Arc::new(InMemoryPort::default());
        assert!(DisconnectForgeAccount::new(port)
            .execute(&github_account())
            .is_ok());
    }

    /// US-102 criterion 3: a credential-store failure must never look like
    /// a reason to block anything else — `GetForgeConnectionStatus` folds
    /// it into `NotConnected` instead of propagating an error.
    #[test]
    fn connection_status_never_propagates_a_storage_error() {
        let port: Arc<dyn ForgeCredentialPort> = Arc::new(AlwaysFailingPort);
        assert_eq!(
            GetForgeConnectionStatus::new(port).execute(&github_account()),
            ForgeConnectionStatus::NotConnected
        );
    }

    #[test]
    fn token_debug_never_prints_the_raw_secret() {
        let token = ForgeToken::new("sentinel-fake-token-abc123");
        let debug = format!("{token:?}");
        assert!(!debug.contains("sentinel-fake-token-abc123"));
        assert_eq!(debug, "ForgeToken(***)");
    }
}
