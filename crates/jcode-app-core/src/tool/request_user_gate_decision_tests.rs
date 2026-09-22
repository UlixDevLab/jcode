use super::*;
use serde_json::json;

#[tokio::test]
async fn requires_decision_narrowing_and_affected_constraints() {
    let _env = jcode_base::storage::lock_test_env();
    let home = tempfile::tempdir().expect("temporary JCODE_HOME");
    let _home = JcodeHomeGuard::set(home.path());
    let tool = RequestUserGateTool::new();
    for field in ["narrows", "affects_constraints"] {
        let mut request = input();
        request
            .as_object_mut()
            .expect("request object")
            .remove(field);
        let error = tool
            .execute(request, context(&format!("missing-{field}")))
            .await
            .expect_err("missing decision field must be rejected");
        assert!(
            format!("{error:#}").contains(field),
            "missing-field error did not identify {field}: {error:#}"
        );
    }

    let mut consent = input();
    consent["narrows"] = json!("none");
    consent["affects_constraints"] = json!([]);
    let error = tool
        .execute(consent, context("consent-not-decision"))
        .await
        .expect_err("a consent request must not enter the decision gate");
    assert!(format!("{error:#}").contains("this is a consent, not a decision"));

    let output = tool
        .execute(input(), context("legitimate-decision"))
        .await
        .expect("narrowing with an affected requirement is accepted");
    assert_eq!(
        output.metadata.expect("waiting metadata")["status"],
        "waiting"
    );
}
