use super::*;
use serde_json::json;

#[test]
fn navigation_policy_allows_only_typed_navigation_and_can_be_opted_out() {
    let default_policy = crate::config::ConsentConfig::default();
    let no_client = context(None);

    let open_page = protected_operation_with_context(
        "open",
        &json!({"action": "open", "target": "https://example.test/docs"}),
        &no_client,
    )
    .expect("explicit http page open is classified");
    assert_eq!(open_page.class(), OperationClass::Navigation);
    assert!(!open_page.requires_native_consent(&default_policy));

    let local_file = tempfile::NamedTempFile::new().expect("temporary local file");
    let open_file = protected_operation_with_context(
        "open",
        &json!({"action": "open", "target": local_file.path()}),
        &no_client,
    )
    .expect("explicit local file open is classified");
    assert_eq!(open_file.class(), OperationClass::Navigation);
    assert!(!open_file.requires_native_consent(&default_policy));

    let app_parent = tempfile::tempdir().expect("temporary app parent");
    let app = app_parent.path().join("Preview.app");
    std::fs::create_dir(&app).expect("temporary app bundle");
    let launch_app = protected_operation_with_context(
        "open",
        &json!({"action": "open", "target": app}),
        &no_client,
    )
    .expect("explicit local app open is classified");
    assert_eq!(launch_app.class(), OperationClass::Navigation);

    let browser_page = protected_operation_with_context(
        "browser",
        &json!({"action": "open", "url": "https://example.test/docs"}),
        &no_client,
    )
    .expect("pure browser URL load is classified");
    assert_eq!(browser_page.class(), OperationClass::Navigation);
    assert!(!browser_page.requires_native_consent(&default_policy));

    for input in [
        json!({"action": "click", "url": "https://example.test/docs"}),
        json!({"action": "open", "url": "https://alice:secret@example.test/docs"}),
        json!({"action": "open", "url": "https://example.test/docs", "selector": "#login"}),
        json!({"action": "open", "url": "https://example.test/docs", "script": "submit()"}),
    ] {
        let operation = protected_operation_with_context("browser", &input, &no_client)
            .expect("browser interaction is protected");
        assert_ne!(operation.class(), OperationClass::Navigation, "{input}");
        assert!(
            operation.requires_native_consent(&default_policy),
            "{input}"
        );
    }

    let keyboard = protected_operation_with_context(
        "macos_computer_use",
        &json!({"action": "key", "keys": "cmd+l"}),
        &no_client,
    )
    .expect("keyboard input is protected");
    assert_eq!(keyboard.class(), OperationClass::DesktopInput);
    assert!(keyboard.requires_native_consent(&default_policy));

    let mutation = protected_operation_with_context(
        "macos_computer_use",
        &json!({"action": "run_applescript", "script": "return 1"}),
        &no_client,
    )
    .expect("desktop mutation is protected");
    assert_eq!(mutation.class(), OperationClass::AppleScript);
    assert!(mutation.requires_native_consent(&default_policy));

    let auth = ClassifiedOperation::from_native(ProtectedOperation::new(
        "gmail",
        "connect",
        "browser OAuth for Gmail account",
    ));
    assert_eq!(auth.class(), OperationClass::Auth);
    assert!(auth.requires_native_consent(&default_policy));

    let unknown =
        ClassifiedOperation::from_native(ProtectedOperation::new("mcp", "other", "target"));
    assert_eq!(unknown.class(), OperationClass::Unknown);
    assert!(unknown.requires_native_consent(&default_policy));

    let opt_out = crate::config::ConsentConfig {
        navigation: crate::config::ConsentNavigationPolicy::Prompt,
    };
    assert!(open_page.requires_native_consent(&opt_out));
    assert!(browser_page.requires_native_consent(&opt_out));
}
