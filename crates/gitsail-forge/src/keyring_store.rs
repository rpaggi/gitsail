//! [`ForgeCredentialPort`] backed by the OS-native credential store, via
//! the `keyring` crate (T-244/US-102 criterion 1).
//!
//! Storage layout: every account is stored under one fixed `keyring`
//! service name (`"gitsail-forge"`) with a username of
//! `ForgeAccountId::storage_key()` (`github@github.com`,
//! `gitlab@gitlab.example.com`, ...) — this keeps every GitSail-managed
//! secret grouped under one recognizable service in the OS credential UI
//! (Keychain Access, Credential Manager, Seahorse/KWalletManager, ...)
//! while still keeping distinct forge accounts fully separate entries.
//!
//! No token content ever appears in an error message this adapter
//! constructs; every message is additionally passed through
//! [`gitsail_domain::redact::redact_secrets`] as defense in depth even
//! though nothing here is expected to embed one (AGENTS.md: "never grave
//! token em texto puro").
//!
//! See the crate root doc comment for why this adapter cannot be
//! exercised against a real OS keyring in this workspace's automated
//! tests, and needs manual verification per target OS.

use gitsail_application::{ForgeAccountId, ForgeConnectionStatus, ForgeCredentialPort, ForgeToken};
use gitsail_domain::redact::redact_secrets;
use gitsail_domain::{ErrorCode, GitSailError};
use keyring::Entry;

/// The fixed `keyring` service name every GitSail forge token is stored
/// under (see module docs for the layout).
const SERVICE: &str = "gitsail-forge";

/// The real, OS-secure-storage-backed [`ForgeCredentialPort`] adapter.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringForgeCredentialStore;

impl KeyringForgeCredentialStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(&self, account: &ForgeAccountId) -> Result<Entry, GitSailError> {
        Entry::new(SERVICE, &account.storage_key())
            .map_err(|err| store_error(account, "open", err))
    }
}

fn store_error(account: &ForgeAccountId, action: &str, err: keyring::Error) -> GitSailError {
    let message = redact_secrets(&format!(
        "forge credential store {action} failed for account '{}'",
        account.storage_key()
    ));
    GitSailError::new(ErrorCode::Internal, message)
        .with_remediation("check that an OS credential store is available and unlocked")
        .with_source(err)
}

impl ForgeCredentialPort for KeyringForgeCredentialStore {
    fn connect(&self, account: &ForgeAccountId, token: ForgeToken) -> Result<(), GitSailError> {
        self.entry(account)?
            .set_password(token.expose_secret())
            .map_err(|err| store_error(account, "connect", err))
    }

    fn disconnect(&self, account: &ForgeAccountId) -> Result<(), GitSailError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) => Ok(()),
            // Disconnecting an account with nothing stored is not an
            // error (US-102 criterion 2).
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(store_error(account, "disconnect", err)),
        }
    }

    fn status(&self, account: &ForgeAccountId) -> Result<ForgeConnectionStatus, GitSailError> {
        match self.entry(account)?.get_password() {
            Ok(_) => Ok(ForgeConnectionStatus::Connected),
            Err(keyring::Error::NoEntry) => Ok(ForgeConnectionStatus::NotConnected),
            Err(err) => Err(store_error(account, "status", err)),
        }
    }

    fn token(&self, account: &ForgeAccountId) -> Result<Option<ForgeToken>, GitSailError> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(ForgeToken::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(store_error(account, "token", err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::ForgeKind;

    /// This much *can* be verified without a real OS keyring: entry
    /// construction (service/username naming) never touches the store
    /// itself, so it must succeed the same way on every platform
    /// regardless of whether a backend is actually reachable. Manual
    /// verification of `set_password`/`get_password`/`delete_credential`
    /// against a real store is still required per target OS (see crate
    /// root docs).
    #[test]
    fn entry_construction_never_touches_the_store_and_always_succeeds() {
        let store = KeyringForgeCredentialStore::new();
        let account = ForgeAccountId::new(ForgeKind::GitHub, "github.com");
        assert!(store.entry(&account).is_ok());
    }

    #[test]
    fn store_error_message_identifies_the_account_and_is_redacted() {
        let account = ForgeAccountId::new(ForgeKind::GitHub, "github.com");
        let err = store_error(&account, "connect", keyring::Error::NoEntry);
        assert!(err.message().contains("github@github.com"));

        // `store_error` never interpolates the token itself into the
        // message at all (by construction — nothing here ever has access
        // to one), but every message still passes through
        // `redact_secrets` as defense-in-depth; prove that pass-through
        // actually happens by checking a sentinel `key=value` fragment
        // smuggled in through the one caller-adjacent string this
        // function does format in (the action label) is scrubbed.
        let err = store_error(&account, "connect token=sentinel-fake-abc123", keyring::Error::NoEntry);
        assert!(!err.message().contains("sentinel-fake-abc123"));
    }
}
