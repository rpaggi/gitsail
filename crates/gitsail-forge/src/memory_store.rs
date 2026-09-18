//! An in-memory [`ForgeCredentialPort`] double (T-244/US-102).
//!
//! Exported here — rather than duplicated inside every consumer's test
//! module — so `gitsail-tui`, `gitsail-cli`, and `apps/desktop` can all
//! wire the exact same deterministic double into their own tests, the way
//! `gitsail-tui` exports both `SystemClipboard` and `FakeClipboard` side
//! by side. It is not gated behind `#[cfg(test)]`: a binary crate's own
//! integration tests live in a separate compilation unit, so a
//! `cfg(test)`-only item in this crate would not be visible to them.

use std::collections::HashMap;
use std::sync::Mutex;

use gitsail_application::{ForgeAccountId, ForgeConnectionStatus, ForgeCredentialPort, ForgeToken};
use gitsail_domain::GitSailError;

/// A deterministic, in-process stand-in for OS-secure storage.
///
/// Never persists anything beyond the process (that is the point: tests
/// must not depend on, or pollute, a real OS keyring — see the crate root
/// docs on why the real adapter cannot be exercised in automated tests at
/// all).
#[derive(Debug, Default)]
pub struct InMemoryForgeCredentialStore {
    tokens: Mutex<HashMap<ForgeAccountId, ForgeToken>>,
}

impl InMemoryForgeCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ForgeCredentialPort for InMemoryForgeCredentialStore {
    fn connect(&self, account: &ForgeAccountId, token: ForgeToken) -> Result<(), GitSailError> {
        self.tokens
            .lock()
            .expect("in-memory forge credential store mutex poisoned")
            .insert(account.clone(), token);
        Ok(())
    }

    fn disconnect(&self, account: &ForgeAccountId) -> Result<(), GitSailError> {
        self.tokens
            .lock()
            .expect("in-memory forge credential store mutex poisoned")
            .remove(account);
        Ok(())
    }

    fn status(&self, account: &ForgeAccountId) -> Result<ForgeConnectionStatus, GitSailError> {
        let connected = self
            .tokens
            .lock()
            .expect("in-memory forge credential store mutex poisoned")
            .contains_key(account);
        Ok(if connected {
            ForgeConnectionStatus::Connected
        } else {
            ForgeConnectionStatus::NotConnected
        })
    }

    fn token(&self, account: &ForgeAccountId) -> Result<Option<ForgeToken>, GitSailError> {
        Ok(self
            .tokens
            .lock()
            .expect("in-memory forge credential store mutex poisoned")
            .get(account)
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_application::{
        ConnectForgeAccount, DisconnectForgeAccount, GetForgeConnectionStatus,
    };
    use gitsail_domain::ForgeKind;
    use std::sync::Arc;

    #[test]
    fn round_trips_connect_status_disconnect_through_the_use_cases() {
        let port: Arc<dyn ForgeCredentialPort> = Arc::new(InMemoryForgeCredentialStore::new());
        let account = ForgeAccountId::new(ForgeKind::GitLab, "gitlab.com");

        assert_eq!(
            GetForgeConnectionStatus::new(port.clone()).execute(&account),
            ForgeConnectionStatus::NotConnected
        );

        ConnectForgeAccount::new(port.clone())
            .execute(&account, ForgeToken::new("sentinel-fake-token"))
            .unwrap();
        assert_eq!(
            GetForgeConnectionStatus::new(port.clone()).execute(&account),
            ForgeConnectionStatus::Connected
        );

        DisconnectForgeAccount::new(port.clone())
            .execute(&account)
            .unwrap();
        assert_eq!(
            GetForgeConnectionStatus::new(port).execute(&account),
            ForgeConnectionStatus::NotConnected
        );
    }

    #[test]
    fn distinct_accounts_never_collide() {
        let store = InMemoryForgeCredentialStore::new();
        let github = ForgeAccountId::new(ForgeKind::GitHub, "github.com");
        let gitlab = ForgeAccountId::new(ForgeKind::GitLab, "gitlab.com");

        store
            .connect(&github, ForgeToken::new("gh-sentinel"))
            .unwrap();
        assert_eq!(
            store.status(&gitlab).unwrap(),
            ForgeConnectionStatus::NotConnected
        );
        assert_eq!(
            store.status(&github).unwrap(),
            ForgeConnectionStatus::Connected
        );
    }
}
