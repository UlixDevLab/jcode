use super::*;

pub(super) fn apply_root_tool_policy_env(config: &mut Config) {
    if let Ok(value) = std::env::var("JCODE_ROOT_TOOL_POLICY_MODE")
        && let Some(mode) = SessionToolPolicyMode::parse(&value)
    {
        config.agents.root_tool_policy_mode = mode;
    }
}
