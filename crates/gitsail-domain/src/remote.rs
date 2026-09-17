//! Remote entity (SAD §8).

use std::fmt;

/// A remote URL, kept as an opaque string.
///
/// `Display`/`Debug` always render the [`RemoteUrl::redacted`] form so a
/// stray `{}`/`{:?}` in a log line cannot leak embedded credentials
/// (SAD §11, §28). Use [`RemoteUrl::as_str`] when the raw value is
/// genuinely needed, e.g. to hand it back to a Git process invocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteUrl(String);

impl RemoteUrl {
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The URL with any embedded `user:password@` credentials masked.
    pub fn redacted(&self) -> String {
        let Some(scheme_end) = self.0.find("://") else {
            return self.0.clone();
        };
        let authority_start = scheme_end + 3;
        let rest = &self.0[authority_start..];
        let Some(at) = rest.find('@') else {
            return self.0.clone();
        };
        if rest[..at].find('/').is_some() {
            // The '@' belongs to the path, not credentials.
            return self.0.clone();
        }
        format!("{}***@{}", &self.0[..authority_start], &rest[at + 1..])
    }
}

impl fmt::Display for RemoteUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

/// A configured remote.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Remote {
    pub name: String,
    pub fetch_url: RemoteUrl,
    pub push_url: RemoteUrl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_embedded_credentials() {
        let url = RemoteUrl::new("https://user:secret-token@github.com/org/repo.git");
        assert_eq!(url.redacted(), "https://***@github.com/org/repo.git");
        assert!(!url.to_string().contains("secret-token"));
    }

    #[test]
    fn leaves_urls_without_credentials_untouched() {
        let url = RemoteUrl::new("git@github.com:org/repo.git");
        assert_eq!(url.redacted(), url.as_str());

        let url = RemoteUrl::new("https://github.com/org/repo.git");
        assert_eq!(url.redacted(), url.as_str());
    }
}
