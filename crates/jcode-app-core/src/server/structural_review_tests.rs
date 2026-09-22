#![deny(dead_code)]

use super::*;
use std::time::Instant;
use tokio::sync::mpsc;

fn member(session_id: &str) -> SwarmMember {
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    SwarmMember {
        session_id: session_id.to_string(),
        event_tx,
        event_txs: HashMap::new(),
        working_dir: None,
        swarm_id: Some("structural-review-test".to_string()),
        swarm_enabled: true,
        status: "ready".to_string(),
        detail: None,
        task_label: None,
        friendly_name: Some(session_id.to_string()),
        report_back_to_session_id: Some("coordinator".to_string()),
        latest_completion_report: None,
        role: "agent".to_string(),
        joined_at: Instant::now(),
        last_status_change: Instant::now(),
        is_headless: false,
        output_tail: None,
        todo_progress: None,
        todo_items: Vec::new(),
        runtime: crate::protocol::SwarmMemberRuntime::default(),
    }
}

fn candidate(digest: &str) -> StructuralReviewCandidate {
    StructuralReviewCandidate {
        project_root: "/test/project".to_string(),
        cycle_id: 7,
        baseline: "base-sha".to_string(),
        candidate_paths: vec!["src/changed.rs".to_string()],
        candidate_digest: digest.to_string(),
        policy_version: "structural-review/v2".to_string(),
        source_changed: true,
        risk_signals: vec!["physical-350-deep-review-marker:src/changed.rs:351".to_string()],
    }
}

fn runtime() -> (Arc<StructuralReviewRuntime>, tempfile::TempDir) {
    let root = tempfile::tempdir().expect("temporary review store");
    let members = Arc::new(RwLock::new(HashMap::from([
        ("coordinator".to_string(), member("coordinator")),
        ("owner".to_string(), member("owner")),
        ("reviewer".to_string(), member("reviewer")),
        ("other".to_string(), member("other")),
    ])));
    let coordinators = Arc::new(RwLock::new(HashMap::from([(
        "structural-review-test".to_string(),
        "coordinator".to_string(),
    )])));
    (
        Arc::new(StructuralReviewRuntime {
            members,
            coordinators,
            state: Arc::new(Mutex::new(ReviewState::default())),
            store_root: Some(root.path().to_path_buf()),
        }),
        root,
    )
}

async fn open(
    coordinator: &Arc<dyn StructuralReviewAuthority>,
    candidate: StructuralReviewCandidate,
) -> StructuralReviewReceipt {
    coordinator
        .open_structural_review(StructuralReviewOpen {
            owner_session_id: "owner".to_string(),
            reviewer_session_id: "reviewer".to_string(),
            candidate,
        })
        .await
        .expect("coordinator opens independent review")
}

#[path = "structural_review_tests/authority.rs"]
mod authority;
