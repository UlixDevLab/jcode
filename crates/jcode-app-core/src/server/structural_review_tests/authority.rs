use super::*;

#[tokio::test]
async fn only_designated_independent_reviewer_can_authorize_owner_closure() {
    let (runtime, _store) = runtime();
    let coordinator = runtime.capability("coordinator".to_string());
    let owner = runtime.capability("owner".to_string());
    let reviewer = runtime.capability("reviewer".to_string());
    let other = runtime.capability("other".to_string());
    let request = open(&coordinator, candidate("digest-a")).await;

    for forged_actor in [&owner, &coordinator, &other] {
        assert!(
            forged_actor
                .submit_structural_review(StructuralReviewSubmission {
                    request_id: request.request_id.clone(),
                    disposition: StructuralReviewDisposition::Cohesive,
                    refactor_obligations: Vec::new(),
                })
                .await
                .is_err()
        );
    }
    reviewer
        .submit_structural_review(StructuralReviewSubmission {
            request_id: request.request_id,
            disposition: StructuralReviewDisposition::Cohesive,
            refactor_obligations: Vec::new(),
        })
        .await
        .expect("designated reviewer records verdict");
    assert_eq!(
        owner
            .validate_structural_review(candidate("digest-a"))
            .await
            .expect("owner may close with bound receipt")
            .disposition,
        StructuralReviewDisposition::Cohesive
    );
}

#[tokio::test]
async fn stale_or_changed_review_cannot_authorize_closure() {
    let (runtime, _store) = runtime();
    let coordinator = runtime.capability("coordinator".to_string());
    let owner = runtime.capability("owner".to_string());
    let reviewer = runtime.capability("reviewer".to_string());
    let first = open(&coordinator, candidate("digest-a")).await;
    let second = open(&coordinator, candidate("digest-b")).await;

    assert!(
        reviewer
            .submit_structural_review(StructuralReviewSubmission {
                request_id: first.request_id,
                disposition: StructuralReviewDisposition::Cohesive,
                refactor_obligations: Vec::new(),
            })
            .await
            .is_err()
    );
    reviewer
        .submit_structural_review(StructuralReviewSubmission {
            request_id: second.request_id,
            disposition: StructuralReviewDisposition::Cohesive,
            refactor_obligations: Vec::new(),
        })
        .await
        .expect("current reviewer request succeeds");
    assert!(
        owner
            .validate_structural_review(candidate("post-review-change"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn refactor_required_needs_changed_candidate_and_fresh_cohesive_review() {
    let (runtime, _store) = runtime();
    let coordinator = runtime.capability("coordinator".to_string());
    let owner = runtime.capability("owner".to_string());
    let reviewer = runtime.capability("reviewer".to_string());
    let first = open(&coordinator, candidate("needs-refactor")).await;
    reviewer
        .submit_structural_review(StructuralReviewSubmission {
            request_id: first.request_id,
            disposition: StructuralReviewDisposition::RefactorRequired,
            refactor_obligations: vec!["extract candidate boundary".to_string()],
        })
        .await
        .expect("reviewer records refactor requirement");
    assert!(
        owner
            .validate_structural_review(candidate("needs-refactor"))
            .await
            .is_err()
    );
    let error = coordinator
        .open_structural_review(StructuralReviewOpen {
            owner_session_id: "owner".to_string(),
            reviewer_session_id: "reviewer".to_string(),
            candidate: candidate("needs-refactor"),
        })
        .await
        .expect_err("unchanged refactor-required candidate is rejected");
    assert!(error.to_string().contains("changed candidate"));

    let repaired = open(&coordinator, candidate("repaired")).await;
    reviewer
        .submit_structural_review(StructuralReviewSubmission {
            request_id: repaired.request_id,
            disposition: StructuralReviewDisposition::Cohesive,
            refactor_obligations: Vec::new(),
        })
        .await
        .expect("fresh review accepts repaired candidate");
    assert!(
        owner
            .validate_structural_review(candidate("repaired"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn corrupt_durable_receipt_blocks_closure() {
    let (runtime, _store) = runtime();
    let coordinator = runtime.capability("coordinator".to_string());
    let owner = runtime.capability("owner".to_string());
    let reviewer = runtime.capability("reviewer".to_string());
    let request = open(&coordinator, candidate("durable")).await;
    let receipt = reviewer
        .submit_structural_review(StructuralReviewSubmission {
            request_id: request.request_id,
            disposition: StructuralReviewDisposition::Cohesive,
            refactor_obligations: Vec::new(),
        })
        .await
        .expect("persisted cohesive review");
    std::fs::write(
        runtime
            .record_path(&receipt.request_id)
            .expect("receipt path"),
        b"not-json",
    )
    .expect("corrupt durable receipt");
    assert!(
        owner
            .validate_structural_review(candidate("durable"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn failed_receipt_write_does_not_publish_an_approvable_designation() {
    let (runtime, _store) = runtime();
    let blocked_store = tempfile::NamedTempFile::new().expect("file-backed store root");
    let mut runtime = match Arc::try_unwrap(runtime) {
        Ok(runtime) => runtime,
        Err(_) => panic!("test is the only runtime owner"),
    };
    runtime.store_root = Some(blocked_store.path().to_path_buf());
    let runtime = Arc::new(runtime);
    let coordinator = runtime.capability("coordinator".to_string());
    let owner = runtime.capability("owner".to_string());

    assert!(
        coordinator
            .open_structural_review(StructuralReviewOpen {
                owner_session_id: "owner".to_string(),
                reviewer_session_id: "reviewer".to_string(),
                candidate: candidate("write-failure"),
            })
            .await
            .is_err(),
        "a file cannot become the receipt directory"
    );
    assert!(
        owner
            .validate_structural_review(candidate("write-failure"))
            .await
            .is_err(),
        "a failed pending receipt write must not leave a valid designation"
    );
}

#[tokio::test]
async fn failed_submission_write_after_successful_open_keeps_the_record_unapprovable() {
    let (runtime, _store) = runtime();
    let coordinator = runtime.capability("coordinator".to_string());
    let owner = runtime.capability("owner".to_string());
    let reviewer = runtime.capability("reviewer".to_string());
    let request = open(&coordinator, candidate("submit-write-failure")).await;
    let record_path = runtime.record_path(&request.request_id).unwrap();
    let review_dir = record_path.parent().unwrap().to_path_buf();
    std::fs::remove_file(record_path).unwrap();
    std::fs::remove_dir(&review_dir).unwrap();
    std::fs::write(&review_dir, b"block receipt rewrite").unwrap();

    assert!(
        reviewer
            .submit_structural_review(StructuralReviewSubmission {
                request_id: request.request_id,
                disposition: StructuralReviewDisposition::Cohesive,
                refactor_obligations: Vec::new(),
            })
            .await
            .is_err(),
        "a failed submit write must not publish a verdict after a successful open"
    );
    assert!(
        owner
            .validate_structural_review(candidate("submit-write-failure"))
            .await
            .is_err(),
        "the owner cannot close from a record left pending by failed submission persistence"
    );
}

#[tokio::test]
async fn completed_reviewer_cannot_authorize_owner_closure() {
    let (runtime, _store) = runtime();
    let coordinator = runtime.capability("coordinator".to_string());
    let owner = runtime.capability("owner".to_string());
    let reviewer = runtime.capability("reviewer".to_string());
    let request = open(&coordinator, candidate("reviewer-lifecycle")).await;
    reviewer
        .submit_structural_review(StructuralReviewSubmission {
            request_id: request.request_id,
            disposition: StructuralReviewDisposition::Cohesive,
            refactor_obligations: Vec::new(),
        })
        .await
        .expect("reviewer records verdict while live");
    runtime
        .members
        .write()
        .await
        .get_mut("reviewer")
        .expect("reviewer member")
        .status = "completed".to_string();
    assert!(
        owner
            .validate_structural_review(candidate("reviewer-lifecycle"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn failed_or_crashed_reviewer_cannot_authorize_owner_closure() {
    for terminal_status in ["failed", "crashed"] {
        let (runtime, _store) = runtime();
        let coordinator = runtime.capability("coordinator".to_string());
        let owner = runtime.capability("owner".to_string());
        let reviewer = runtime.capability("reviewer".to_string());
        let request = open(&coordinator, candidate(terminal_status)).await;
        reviewer
            .submit_structural_review(StructuralReviewSubmission {
                request_id: request.request_id,
                disposition: StructuralReviewDisposition::Cohesive,
                refactor_obligations: Vec::new(),
            })
            .await
            .expect("reviewer records verdict while live");
        runtime
            .members
            .write()
            .await
            .get_mut("reviewer")
            .expect("reviewer member")
            .status = terminal_status.to_string();

        assert!(
            owner
                .validate_structural_review(candidate(terminal_status))
                .await
                .is_err(),
            "{terminal_status} reviewer cannot authorize owner closure"
        );
    }
}
