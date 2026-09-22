use super::{MockProvider, Registry};
use crate::provider::Provider;
use std::sync::Arc;

#[test]
fn test_resolve_skill_aliases_to_skill_manage() {
    assert_eq!(Registry::resolve_tool_name("skill"), "skill_manage");
    assert_eq!(Registry::resolve_tool_name("Skill"), "skill_manage");
    assert_eq!(Registry::resolve_tool_name("skill_manage"), "skill_manage");
}

#[tokio::test]
async fn registry_tool_names_satisfy_provider_function_name_pattern() {
    // OpenAI and Anthropic both reject function names outside
    // `^[a-zA-Z0-9_-]{1,64}$` with a 400 before any model call. A single bad
    // name (e.g. `mission.declare`) breaks every full-tool session at start.
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;
    let defs = registry.definitions(None).await;
    let bad: Vec<String> = defs
        .iter()
        .map(|def| def.name.clone())
        .filter(|name| {
            name.is_empty()
                || name.len() > 64
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .collect();
    assert!(
        bad.is_empty(),
        "tool names rejected by provider APIs: {bad:?}"
    );
}
