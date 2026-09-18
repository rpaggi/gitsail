//! Opens a URL in the user's default browser (T-243/US-101).
//!
//! Deliberately a thin OS-process adapter, not domain logic — the URL it is
//! given is expected to already be the trusted output of
//! [`gitsail_application::GetForgeLink::execute`] /
//! [`gitsail_domain::forge::build_web_url`], which only ever produces a
//! `https://<known-forge-host>/...` string (see that module's doc comment
//! for the full security argument). [`open_url`] still re-checks the
//! scheme itself before spawning anything, as defense in depth — mirrors
//! `gitsail-tui`'s own `browser` module exactly (kept as a small, separate
//! copy here rather than a shared library crate, since Desktop and the TUI
//! are two independent binaries and this adapter is a handful of lines).
//!
//! The URL is always passed as a single, direct process argument, never
//! interpolated into a shell string (`security-privacy-credentials-rules`
//! in the project wiki).

use gitsail_domain::{ErrorCode, GitSailError};

pub fn open_url(url: &str) -> Result<(), GitSailError> {
    if !url.starts_with("https://") {
        return Err(GitSailError::new(
            ErrorCode::Internal,
            "refusing to open a non-https URL",
        ));
    }
    spawn_opener(url)
}

#[cfg(target_os = "macos")]
fn spawn_opener(url: &str) -> Result<(), GitSailError> {
    run("open", url)
}

#[cfg(target_os = "windows")]
fn spawn_opener(url: &str) -> Result<(), GitSailError> {
    run_with_args("cmd", &["/C", "start", "", url])
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn spawn_opener(url: &str) -> Result<(), GitSailError> {
    run("xdg-open", url)
}

#[cfg(not(target_os = "windows"))]
fn run(program: &str, url: &str) -> Result<(), GitSailError> {
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            GitSailError::new(
                ErrorCode::ProcessFailure,
                format!("could not launch '{program}' to open the browser"),
            )
            .with_remediation("check that a default browser opener is installed")
            .with_source(err)
        })
}

#[cfg(target_os = "windows")]
fn run_with_args(program: &str, args: &[&str]) -> Result<(), GitSailError> {
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            GitSailError::new(
                ErrorCode::ProcessFailure,
                format!("could not launch '{program}' to open the browser"),
            )
            .with_remediation("check that a default browser opener is installed")
            .with_source(err)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_non_https_url_without_spawning_anything() {
        assert_eq!(
            open_url("javascript:alert(1)").unwrap_err().code(),
            ErrorCode::Internal
        );
        assert_eq!(
            open_url("file:///etc/passwd").unwrap_err().code(),
            ErrorCode::Internal
        );
        assert_eq!(
            open_url("http://example.com").unwrap_err().code(),
            ErrorCode::Internal
        );
    }
}
