mod assembler;
mod debug;
mod manifest;
mod selector;

#[cfg(test)]
mod assembler_tests;

#[cfg(test)]
mod selector_tests;

pub(crate) use assembler::{ProviderView, assemble_pilot_view};
pub use debug::run_manifest_command;
pub use manifest::{
    ObservationOutcome, RlmStatusSnapshot, latest_status_snapshot, observe_provider_request,
};
pub(crate) use selector::try_select_counterfactual;
pub use selector::{CounterfactualSelection, select_counterfactual};

#[cfg(test)]
pub(crate) use assembler::{AssemblyOutcome, PilotEligibility, assemble_selected_context};
#[cfg(test)]
pub(crate) use selector::SelectionFailure;

use crate::config::RlmMode;

const BINARY_IDENTITY_ENV: &str = "JCODE_RLM_BINARY";

/// Launcher capability supplied by a connecting client and retained on its Agent.
/// The wire format is deliberately a boolean for backward compatibility, but the
/// server never consults its own process environment to derive this value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClientIdentity {
    #[default]
    Ordinary,
    Rlm,
}

impl ClientIdentity {
    pub const fn is_rlm(self) -> bool {
        matches!(self, Self::Rlm)
    }
}

impl From<bool> for ClientIdentity {
    fn from(rlm_client: bool) -> Self {
        if rlm_client {
            Self::Rlm
        } else {
            Self::Ordinary
        }
    }
}

/// Read launcher identity only in the client/CLI process before a Subscribe.
pub fn launcher_identity() -> ClientIdentity {
    match std::env::var(BINARY_IDENTITY_ENV) {
        Ok(value) => {
            let value = value.trim();
            (!value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")).into()
        }
        Err(_) => ClientIdentity::Ordinary,
    }
}

pub fn requested_mode_for(identity: ClientIdentity, configured: RlmMode) -> RlmMode {
    if identity.is_rlm() {
        configured
    } else {
        RlmMode::Off
    }
}

pub fn effective_mode_for(identity: ClientIdentity, configured: RlmMode) -> RlmMode {
    requested_mode_for(identity, configured)
}

pub fn context_mode_tag_for(identity: ClientIdentity, configured: RlmMode) -> &'static str {
    if !identity.is_rlm() {
        return "transcript";
    }
    match requested_mode_for(identity, configured) {
        RlmMode::Off => "rlm-off",
        RlmMode::Observe => "rlm-observe",
        RlmMode::Pilot => "rlm-pilot",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_identities_coexist_without_mode_leakage() {
        let ordinary = ClientIdentity::Ordinary;
        let rlm = ClientIdentity::Rlm;

        assert_eq!(requested_mode_for(ordinary, RlmMode::Observe), RlmMode::Off);
        assert_eq!(requested_mode_for(rlm, RlmMode::Observe), RlmMode::Observe);
        assert_eq!(effective_mode_for(ordinary, RlmMode::Pilot), RlmMode::Off);
        assert_eq!(effective_mode_for(rlm, RlmMode::Pilot), RlmMode::Pilot);
    }

    #[test]
    fn ordinary_jcode_is_always_transcript_mode() {
        assert_eq!(
            effective_mode_for(ClientIdentity::Ordinary, RlmMode::Observe),
            RlmMode::Off
        );
        assert_eq!(
            context_mode_tag_for(ClientIdentity::Ordinary, RlmMode::Pilot),
            "transcript"
        );
    }

    #[test]
    fn pilot_request_is_isolated_by_client_identity() {
        assert_eq!(
            effective_mode_for(ClientIdentity::Rlm, RlmMode::Pilot),
            RlmMode::Pilot
        );
        assert_eq!(
            context_mode_tag_for(ClientIdentity::Rlm, RlmMode::Pilot),
            "rlm-pilot"
        );
    }
}
