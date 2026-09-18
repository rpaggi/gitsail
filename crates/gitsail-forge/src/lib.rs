//! Adapters for [`gitsail_application::ForgeCredentialPort`] (T-244/US-102).
//!
//! This crate is the one place in the workspace that depends on `keyring`
//! (Ports & Adapters: the port lives in `gitsail-application`, only the
//! concrete OS-storage adapter lives here — see that crate's
//! `forge_credentials` module for the full design rationale and the token
//! scope GitSail documents to users).
//!
//! Two implementations are provided:
//! - [`KeyringForgeCredentialStore`] — the real adapter, backed by the
//!   `keyring` crate's per-OS secure storage (macOS Keychain / Windows
//!   Credential Manager / Linux Secret Service, selected per target OS by
//!   this crate's `Cargo.toml` feature flags).
//! - [`InMemoryForgeCredentialStore`] — a deterministic in-memory double
//!   for tests, exported here (rather than duplicated in every consumer)
//!   so `gitsail-tui`, `gitsail-cli`, and `apps/desktop` can all wire the
//!   same double into their own tests, the same way `gitsail-tui` exports
//!   both `SystemClipboard` and `FakeClipboard` side by side.
//!
//! ## Manual verification needed
//!
//! `keyring`'s Linux backend talks to a running Secret Service daemon
//! (gnome-keyring, KWallet, ...) over D-Bus. No such session exists in a
//! headless CI/sandbox environment, so [`KeyringForgeCredentialStore`]
//! cannot be exercised against a real OS keyring by this workspace's
//! automated tests — the same limitation already documented in this
//! project for Tauri/VS Code end-to-end coverage. Its tests instead assert
//! the shape of what it does (key naming, redaction, idempotent
//! disconnect) using [`InMemoryForgeCredentialStore`] as the port
//! implementation under test everywhere behavior can be verified without
//! a real OS keyring; the `keyring`-calling lines of
//! `KeyringForgeCredentialStore` itself need a manual check on each target
//! OS (macOS Keychain prompt, Windows Credential Manager entry, Linux
//! Secret Service unlock) before shipping.

pub mod keyring_store;
pub mod memory_store;

pub use keyring_store::KeyringForgeCredentialStore;
pub use memory_store::InMemoryForgeCredentialStore;
