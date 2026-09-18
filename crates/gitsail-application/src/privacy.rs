//! Local-first privacy policy types (SAD §28; EPIC-22/T-225/US-114).
//!
//! GitSail has no telemetry and no crash-reporting upload implemented as of
//! this module (confirmed by the absence of any HTTP/network client
//! dependency anywhere in this workspace's `Cargo.toml`s). These two types
//! exist anyway, as a reserved, explicit configuration surface, so that if
//! either capability is ever built, its default is opt-in from the very
//! first line of code that reads a `TelemetryPreference`/
//! `CrashReportConsent` — never a default silently added later. This is a
//! policy decision, documented in ADR-018
//! (`docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`), enforced here by
//! `#[derive(Default)]` deliberately resolving to the disabled/not-granted
//! variant.

/// Whether GitSail may send anonymous usage telemetry. No telemetry
/// implementation exists yet; when one is built, it must read this
/// preference and default to [`TelemetryPreference::Disabled`] — never send
/// anything unless the person has explicitly opted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TelemetryPreference {
    #[default]
    Disabled,
    Enabled,
}

impl TelemetryPreference {
    pub const fn is_enabled(self) -> bool {
        matches!(self, TelemetryPreference::Enabled)
    }
}

/// Whether GitSail may transmit a crash report. No crash-reporting upload
/// exists yet; a local panic is always logged locally regardless (see
/// `gitsail-cli`'s panic hook), independent of this preference — this type
/// only ever gates a future *network transmission* of that report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrashReportConsent {
    #[default]
    NotGranted,
    Granted,
}

impl CrashReportConsent {
    pub const fn is_granted(self) -> bool {
        matches!(self, CrashReportConsent::Granted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DoD: "teste de configuração limpa (padrão de fábrica) verifica
    /// ausência de qualquer tentativa de envio espontâneo" — a fresh,
    /// untouched configuration must never enable either kind of
    /// spontaneous network transmission.
    #[test]
    fn factory_default_configuration_never_authorizes_spontaneous_transmission() {
        assert_eq!(
            TelemetryPreference::default(),
            TelemetryPreference::Disabled
        );
        assert!(!TelemetryPreference::default().is_enabled());

        assert_eq!(
            CrashReportConsent::default(),
            CrashReportConsent::NotGranted
        );
        assert!(!CrashReportConsent::default().is_granted());
    }
}
