use crate::config::{Config, SessionToolPolicyMode};
use std::ffi::OsString;

fn restore_env_var(key: &str, previous: Option<OsString>) {
    if let Some(previous) = previous {
        crate::env::set_var(key, previous);
    } else {
        crate::env::remove_var(key);
    }
}

#[test]
fn root_tool_policy_mode_defaults_normal_and_parses_explicit_control_plane_only() {
    assert_eq!(
        Config::default().agents.root_tool_policy_mode,
        SessionToolPolicyMode::Normal
    );
    let config: Config =
        toml::from_str("[agents]\nroot_tool_policy_mode = \"control-plane-only\"\n")
            .expect("explicit control-plane mode should parse");
    assert_eq!(
        config.agents.root_tool_policy_mode,
        SessionToolPolicyMode::ControlPlaneOnly
    );
}

#[test]
fn root_tool_policy_mode_environment_override_enables_control_plane_only() {
    let _guard = crate::storage::lock_test_env();
    let previous = std::env::var_os("JCODE_ROOT_TOOL_POLICY_MODE");
    crate::env::set_var("JCODE_ROOT_TOOL_POLICY_MODE", "control-plane-only");
    let mut config = Config::default();
    config.apply_env_overrides();
    assert_eq!(
        config.agents.root_tool_policy_mode,
        SessionToolPolicyMode::ControlPlaneOnly
    );
    restore_env_var("JCODE_ROOT_TOOL_POLICY_MODE", previous);
}
