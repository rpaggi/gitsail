//! Secret redaction for anything that might reach a log line, a diagnostic,
//! or an exported diagnostic bundle (SAD §28, EPIC-22/US-113: "never log
//! access tokens, passwords, credential-bearing remote URLs, or complete
//! file contents by default").
//!
//! This module is intentionally centralized in `gitsail-domain` rather than
//! reimplemented at each call site (US-110 criterion 3's centralization
//! principle applied to redaction as well as sanitization): `gitsail-git`'s
//! process runner, `gitsail-cli`'s debug output, and any future diagnostic
//! export all call through here.
//!
//! Redaction here is deliberately conservative (a superset of what "looks
//! like" a secret is redacted; under-redacting is the actual security bug),
//! mirroring the same design principle `gitsail-tui`'s `sanitize` module
//! documents for escape sequences.

/// Masks `user:password@` credentials embedded in a `scheme://...` string
/// (e.g. a remote URL argument or a URL appearing inside `git`'s own stderr)
/// before it is stored anywhere logs or errors might surface it. Text that
/// does not look like such a URL passes through unchanged.
///
/// This is the single argument/URL-shaped case; [`redact_secrets`] applies
/// this (via [`redact_secrets_in_word`]) to every whitespace-delimited token
/// of a larger diagnostic string, plus broader `key=value`/header patterns.
pub fn redact_credential_url(text: &str) -> String {
    let Some(scheme_end) = text.find("://") else {
        return text.to_string();
    };
    let authority_start = scheme_end + 3;
    let rest = &text[authority_start..];
    let Some(at) = rest.find('@') else {
        return text.to_string();
    };
    if rest[..at].contains('/') {
        // The '@' belongs to the path, not to a credentials segment.
        return text.to_string();
    }
    format!("{}***@{}", &text[..authority_start], &rest[at + 1..])
}

/// Case-insensitive keys treated as secret-bearing when found in a
/// `key=value` or `key: value` shaped fragment (e.g. `token=abcd1234`,
/// `Authorization: Bearer abcd1234`, `password: hunter2`).
const SECRET_KEYS: &[&str] = &[
    "token",
    "access_token",
    "accesstoken",
    "password",
    "passwd",
    "secret",
    "apikey",
    "api_key",
    "authorization",
];

/// Redacts a single whitespace-delimited word: credential-bearing URLs via
/// [`redact_credential_url`], plus a trailing `key=value` fragment whose key
/// matches [`SECRET_KEYS`] (case-insensitive).
fn redact_secrets_in_word(word: &str) -> String {
    let url_redacted = redact_credential_url(word);
    if url_redacted != word {
        return url_redacted;
    }
    if let Some(eq) = word.find('=') {
        let (key, rest) = word.split_at(eq);
        let value = &rest[1..];
        if SECRET_KEYS.iter().any(|k| key.eq_ignore_ascii_case(k)) && !value.is_empty() {
            return format!("{key}=***");
        }
    }
    word.to_string()
}

/// Redacts every secret-shaped fragment found anywhere in free-form
/// diagnostic text (e.g. `git`'s own stderr, which may itself contain a
/// credential-bearing URL such as "fatal: could not read Username for
/// 'https://user:pass@host'"), not just a single already-isolated argument.
///
/// Handles:
/// - `scheme://user:pass@host` URLs anywhere in the text.
/// - `key=value` fragments whose key is one of [`SECRET_KEYS`].
/// - An `Authorization: <scheme> <token>` header line (redacts the token).
///
/// Word/line structure and all non-secret content is preserved so the
/// redacted text remains useful for diagnosis.
pub fn redact_secrets(text: &str) -> String {
    text.lines()
        .map(redact_secrets_in_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_secrets_in_line(line: &str) -> String {
    // `Authorization: Bearer <token>` (or Basic/other schemes): redact only
    // the credential value, keep the header name and scheme visible since
    // they carry no secret by themselves.
    if let Some(rest) = line
        .split_once(':')
        .filter(|(key, _)| key.trim().eq_ignore_ascii_case("authorization"))
    {
        let (key, value) = rest;
        let mut parts = value.trim_start().splitn(2, ' ');
        return match (parts.next(), parts.next()) {
            (Some(scheme), Some(_token)) if !scheme.is_empty() => {
                format!("{key}: {scheme} ***")
            }
            _ => format!("{key}: ***"),
        };
    }

    line.split(' ')
        .map(redact_secrets_in_word)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_from_url_like_text() {
        let redacted = redact_credential_url("https://user:secret-token@github.com/org/repo.git");
        assert_eq!(redacted, "https://***@github.com/org/repo.git");
        assert!(!redacted.contains("secret-token"));
    }

    #[test]
    fn leaves_non_credential_text_untouched() {
        assert_eq!(redact_credential_url("--porcelain=v2"), "--porcelain=v2");
        assert_eq!(
            redact_credential_url("git@github.com:org/repo.git"),
            "git@github.com:org/repo.git"
        );
        assert_eq!(
            redact_credential_url("https://github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
    }

    /// EPIC-22/T-224 DoD sentinel test: a fake secret embedded in text that
    /// would otherwise reach a log line or diagnostic bundle must not survive
    /// redaction, in any of the shapes GitSail is likely to see it.
    #[test]
    fn sentinel_secrets_never_survive_redact_secrets() {
        const SENTINEL: &str = "sentinel-fake-token-9f3c7a";

        let url_case = format!(
            "fatal: could not read Username for 'https://user:{SENTINEL}@github.com/org/repo.git'"
        );
        assert!(!redact_secrets(&url_case).contains(SENTINEL));

        let kv_case = format!("request failed token={SENTINEL} retrying");
        assert!(!redact_secrets(&kv_case).contains(SENTINEL));

        let header_case = format!("Authorization: Bearer {SENTINEL}");
        assert!(!redact_secrets(&header_case).contains(SENTINEL));

        let password_case = format!("password={SENTINEL}");
        assert!(!redact_secrets(&password_case).contains(SENTINEL));
    }

    #[test]
    fn redact_secrets_preserves_surrounding_diagnostic_text() {
        let text = "exit_code=128 args=[\"fetch\"] stderr=fatal: authentication failed";
        let redacted = redact_secrets(text);
        assert!(redacted.contains("exit_code=128"));
        assert!(redacted.contains("authentication failed"));
    }

    #[test]
    fn multiline_text_is_redacted_line_by_line() {
        let text = "line one\nAuthorization: Bearer abc123\nline three".to_string();
        let redacted = redact_secrets(&text);
        assert!(redacted.contains("line one"));
        assert!(redacted.contains("line three"));
        assert!(!redacted.contains("abc123"));
    }
}
