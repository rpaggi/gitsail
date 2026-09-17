//! Correlation identifier carried by every [`crate::Envelope`] (SAD §14).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Correlates a request/response pair across a process boundary.
///
/// Opaque to consumers: they must treat it as a string to echo back or log,
/// never parse its internal shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(String);

impl RequestId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Generates a correlation id unique within this process: wall-clock
    /// nanoseconds plus a monotonic counter, so two ids generated back to
    /// back never collide even when the clock has not advanced (a CLI
    /// invocation runs a single query per process, so process-local
    /// uniqueness is all the correlation contract requires).
    pub fn generate() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self(format!("{nanos:x}-{seq:x}"))
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_unique_within_the_process() {
        let a = RequestId::generate();
        let b = RequestId::generate();
        assert_ne!(a, b);
    }
}
