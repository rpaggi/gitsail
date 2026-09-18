//! Opens a URL in the user's default browser (T-243/US-101).
//!
//! This is a thin OS-process adapter, not domain logic — the URL it is
//! given is expected to already be the trusted output of
//! [`gitsail_application::GetForgeLink::execute`] /
//! [`gitsail_domain::forge::build_web_url`], which only ever produces a
//! `https://<known-forge-host>/...` string (see that module's doc comment
//! for the full security argument). [`open_url`] still re-checks the
//! scheme itself before spawning anything, as defense in depth against a
//! future caller passing it something else by mistake — this function is
//! the last point before an external process actually runs, so it is the
//! right place for a final, cheap check even though the real guarantee
//! lives upstream.
//!
//! The URL is always passed as a single, direct process argument, never
//! interpolated into a shell string, matching the same rule this project
//! applies to every other external command (`security-privacy-credentials-rules`
//! in the project wiki: "never interpolated into a shell, never executed
//! as a command").

use gitsail_domain::{ErrorCode, GitSailError};

/// Opens `url` in the OS default browser.
///
/// Returns an error instead of spawning anything when `url` is not an
/// `https://` URL — this can only happen if a caller ever passes
/// something other than [`gitsail_application::GetForgeLink`]'s output,
/// which never happens in this crate today, but a silent no-op here would
/// be a worse failure mode than a clear, refused error.
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
    // `cmd /C start "" <url>` is the standard way to hand a URL to the
    // shell's own URL-protocol dispatch without invoking a shell over the
    // URL itself: `url` is still passed as `start`'s own argument, not
    // concatenated into a command string `cmd` re-parses.
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
        let err = open_url("javascript:alert(1)").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);

        let err = open_url("file:///etc/passwd").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);

        let err = open_url("http://example.com").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
    }
}
