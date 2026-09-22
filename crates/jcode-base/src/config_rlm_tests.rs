use super::*;

#[test]
fn missing_rlm_section_preserves_off_default() {
    let config: Config = toml::from_str("").expect("parse empty config");
    assert_eq!(config.rlm, RlmConfig::default());
}

#[test]
fn observe_overlay_parses_without_changing_other_defaults() {
    let config: Config = toml::from_str(
        r#"
[rlm]
mode = "observe"
max_depth = 1
max_children = 0
max_provider_calls = 12
max_tool_rounds_without_evidence = 3
manifest_retention = "session-scoped-redacted"
"#,
    )
    .expect("parse RLM overlay");
    assert_eq!(config.rlm.mode, RlmMode::Observe);
    assert_eq!(config.rlm.max_depth, 1);
    assert_eq!(config.rlm.max_children, 0);
    assert_eq!(config.provider.default_model, None);
    assert_eq!(config.provider.default_provider, None);
    assert_eq!(
        config.provider.openai_reasoning_effort.as_deref(),
        Some("low")
    );
    assert_eq!(config.provider.openai_service_tier.as_deref(), Some("off"));
}
