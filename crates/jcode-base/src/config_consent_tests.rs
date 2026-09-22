use super::{Config, ConsentNavigationPolicy};

#[test]
fn consent_navigation_defaults_to_allow_and_parses_prompt_opt_out() {
    assert!(Config::default().consent.navigation.allows_navigation());

    let config: Config = toml::from_str("[consent]\nnavigation = \"prompt\"\n")
        .expect("consent navigation opt-out should parse");
    assert_eq!(config.consent.navigation, ConsentNavigationPolicy::Prompt);
    assert!(!config.consent.navigation.allows_navigation());
}
