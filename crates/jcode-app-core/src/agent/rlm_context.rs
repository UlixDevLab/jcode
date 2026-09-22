use super::*;
use crate::message::{ContentBlock, Message, Role};

const PILOT_TOKEN_BUDGET: usize = 24_000;

impl Agent {
    pub(super) fn rlm_turn_root_message(&self) -> Option<Message> {
        self.session
            .messages
            .iter()
            .rev()
            .map(crate::session::StoredMessage::to_message)
            .find(is_user_text)
    }

    pub(super) fn prepare_rlm_provider_view(
        &mut self,
        baseline: &[Message],
        root_message: Option<&Message>,
        call_index: u32,
    ) -> crate::rlm::ProviderView {
        if self.rlm_requested_mode() != crate::config::RlmMode::Pilot {
            return crate::rlm::ProviderView::baseline(baseline, None::<String>);
        }
        if let Some(reason) = self.rlm_pilot_fallback_reason.clone() {
            return crate::rlm::ProviderView::baseline(baseline, Some(reason));
        }
        if let Some(reason) = self.rlm_pilot_eligibility_failure(call_index) {
            return self.latch_rlm_fallback(baseline, reason);
        }
        let view = crate::rlm::assemble_pilot_view(baseline, root_message, PILOT_TOKEN_BUDGET);
        if let Some(reason) = view.fallback_reason.clone() {
            return self.latch_rlm_fallback(baseline, reason);
        }
        view
    }

    fn latch_rlm_fallback(
        &mut self,
        baseline: &[Message],
        reason: impl Into<String>,
    ) -> crate::rlm::ProviderView {
        let reason = reason.into();
        self.rlm_pilot_fallback_reason = Some(reason.clone());
        logging::warn(&format!("RLM pilot latched to baseline: {reason}"));
        crate::rlm::ProviderView::baseline(baseline, Some(reason))
    }

    fn rlm_pilot_eligibility_failure(&self, call_index: u32) -> Option<&'static str> {
        let config = &crate::config::config().rlm;
        if !(self.session.is_self_dev() || self.session.is_debug) {
            return Some("not-selfdev-or-debug");
        }
        if self.session.parent_id.is_some() {
            return Some("not-root-session");
        }
        if config.max_depth > 1 || config.max_children != 0 {
            return Some("unsafe-recursion-budget");
        }
        if call_index == 0 || call_index > config.max_provider_calls.max(1) {
            return Some("provider-call-budget-exceeded");
        }
        if self.session.compaction.is_some() {
            return Some("compaction-state-active");
        }
        if self.provider_session_id.is_some() || self.session.provider_session_id.is_some() {
            return Some("provider-resume-active");
        }
        None
    }
}

fn is_user_text(message: &Message) -> bool {
    message.role == Role::User
        && message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { .. }))
}
