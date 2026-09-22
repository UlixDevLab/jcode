use super::*;
use crate::auth::external::{ExternalAuthSource, trust_external_auth_source};
use tempfile::TempDir;

fn write_auth_file(path: &std::path::Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_string(&value).unwrap()).unwrap();
}

#[test]
fn resolver_uses_the_trusted_active_credential() {
    let _guard = crate::storage::lock_test_env();
    let dir = TempDir::new().unwrap();
    let prev = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", dir.path());

    let path = ExternalAuthSource::Hermes.path().unwrap();
    write_auth_file(
        &path,
        serde_json::json!({
            "version": 1,
            "active_provider": "hermes",
            "credential_pool": {
                "hermes": [{
                    "auth_type": "api_key",
                    "access_token": "hermes-local-key"
                }]
            }
        }),
    );

    assert!(load_hermes_api_key().is_none());
    trust_external_auth_source(ExternalAuthSource::Hermes).unwrap();
    assert_eq!(load_hermes_api_key().as_deref(), Some("hermes-local-key"));

    if let Some(prev) = prev {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}
