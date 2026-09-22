use crate::server::StructuralReviewRuntime;
use jcode_tool_core::{StructuralReviewCandidate, StructuralReviewOpen};

#[tokio::test]
async fn handle_resume_session_restores_member_events_and_review_authority() -> Result<()> {
    let _guard = crate::storage::lock_test_env();
    let (_runtime, prev_runtime) = setup_runtime_dir()?;

    let target_session_id = "session_restore_target";
    let temp_session_id = "session_restore_temp";

    let mut persisted = crate::session::Session::create_with_id(
        target_session_id.to_string(),
        None,
        Some("Resume Registration Ordering".to_string()),
    );
    persisted.save()?;

    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Arc::new(Mutex::new(build_test_agent_with_id(
        provider.clone(),
        registry.clone(),
        temp_session_id,
        Vec::new(),
    )));

    let sessions = Arc::new(RwLock::new(HashMap::from([(
        temp_session_id.to_string(),
        Arc::clone(&agent),
    )])));
    let shutdown_signals = Arc::new(RwLock::new(HashMap::<String, InterruptSignal>::new()));
    let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::new()));
    let now = Instant::now();
    let client_connections = Arc::new(RwLock::new(HashMap::from([(
        "conn_restore".to_string(),
        ClientConnectionInfo {
            client_id: "conn_restore".to_string(),
            session_id: temp_session_id.to_string(),
            client_instance_id: None,
            debug_client_id: Some("debug_restore".to_string()),
            connected_at: now,
            last_seen: now,
            is_processing: false,
            current_tool_name: None,
            terminal_env: Vec::new(),
            disconnect_tx: mpsc::unbounded_channel().0,
        },
    )])));
    let client_debug_state = Arc::new(RwLock::new(ClientDebugState::default()));
    let (placeholder_event_tx, _placeholder_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let (owner_event_tx, _owner_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let (reviewer_event_tx, _reviewer_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let swarm_id = "restore-authority-swarm";
    let owner_session_id = "restore-authority-owner";
    let reviewer_session_id = "restore-authority-reviewer";
    let swarm_members = Arc::new(RwLock::new(HashMap::from([
        (
            temp_session_id.to_string(),
            SwarmMember {
                session_id: temp_session_id.to_string(),
                event_tx: placeholder_event_tx,
                event_txs: HashMap::new(),
                working_dir: None,
                swarm_id: Some(swarm_id.to_string()),
                swarm_enabled: true,
                status: "ready".to_string(),
                detail: None,
                task_label: None,
                friendly_name: Some("restore".to_string()),
                report_back_to_session_id: None,
                latest_completion_report: None,
                role: "coordinator".to_string(),
                joined_at: now,
                last_status_change: now,
                is_headless: false,
                output_tail: None,
                todo_progress: None,
                todo_items: Vec::new(),
                runtime: crate::protocol::SwarmMemberRuntime::default(),
            },
        ),
        (
            owner_session_id.to_string(),
            SwarmMember {
                session_id: owner_session_id.to_string(),
                event_tx: owner_event_tx,
                event_txs: HashMap::new(),
                working_dir: None,
                swarm_id: Some(swarm_id.to_string()),
                swarm_enabled: true,
                status: "ready".to_string(),
                detail: None,
                task_label: None,
                friendly_name: Some("owner".to_string()),
                report_back_to_session_id: Some(temp_session_id.to_string()),
                latest_completion_report: None,
                role: "agent".to_string(),
                joined_at: now,
                last_status_change: now,
                is_headless: false,
                output_tail: None,
                todo_progress: None,
                todo_items: Vec::new(),
                runtime: crate::protocol::SwarmMemberRuntime::default(),
            },
        ),
        (
            reviewer_session_id.to_string(),
            SwarmMember {
                session_id: reviewer_session_id.to_string(),
                event_tx: reviewer_event_tx,
                event_txs: HashMap::new(),
                working_dir: None,
                swarm_id: Some(swarm_id.to_string()),
                swarm_enabled: true,
                status: "ready".to_string(),
                detail: None,
                task_label: None,
                friendly_name: Some("reviewer".to_string()),
                report_back_to_session_id: Some(temp_session_id.to_string()),
                latest_completion_report: None,
                role: "agent".to_string(),
                joined_at: now,
                last_status_change: now,
                is_headless: false,
                output_tail: None,
                todo_progress: None,
                todo_items: Vec::new(),
                runtime: crate::protocol::SwarmMemberRuntime::default(),
            },
        ),
    ])));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::from([(
        swarm_id.to_string(),
        HashSet::from([
            temp_session_id.to_string(),
            owner_session_id.to_string(),
            reviewer_session_id.to_string(),
        ]),
    )])));
    let file_touch = FileTouchService::new();
    let channel_subscriptions = Arc::new(RwLock::new(HashMap::<
        String,
        HashMap<String, HashSet<String>>,
    >::new()));
    let channel_subscriptions_by_session = Arc::new(RwLock::new(HashMap::<
        String,
        HashMap<String, HashSet<String>>,
    >::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::<String, VersionedPlan>::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::from([(
        swarm_id.to_string(),
        temp_session_id.to_string(),
    )])));
    let structural_review_runtime =
        StructuralReviewRuntime::new(Arc::clone(&swarm_members), Arc::clone(&swarm_coordinators));
    agent.lock().await.set_structural_review_authority(
        structural_review_runtime.capability(temp_session_id.to_string()),
    );
    let client_count = Arc::new(RwLock::new(1usize));
    let (writer, _peer_stream) = test_writer()?;
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let event_history = Arc::new(RwLock::new(VecDeque::<SwarmEvent>::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _swarm_event_rx) = broadcast::channel::<SwarmEvent>(8);
    let mcp_pool = Arc::new(crate::mcp::SharedMcpPool::from_default_config());

    let mut client_selfdev = false;
    let mut client_session_id = temp_session_id.to_string();

    let failed_resume = handle_resume_session(
        45,
        "session_restore_missing".to_string(),
        None,
        None,
        false,
        false,
        &mut client_selfdev,
        &mut client_session_id,
        "conn_restore",
        &agent,
        &provider,
        &registry,
        &sessions,
        &shutdown_signals,
        &soft_interrupt_queues,
        &client_connections,
        &client_debug_state,
        &swarm_members,
        &swarms_by_id,
        &file_touch,
        &channel_subscriptions,
        &channel_subscriptions_by_session,
        &swarm_plans,
        &swarm_coordinators,
        &structural_review_runtime,
        &client_count,
        &writer,
        "test-server",
        "🌿",
        &client_event_tx,
        &mcp_pool,
        &event_history,
        &event_counter,
        &swarm_event_tx,
        None,
        false,
    )
    .await?;
    assert!(Arc::ptr_eq(&failed_resume, &agent));
    assert_eq!(client_session_id, temp_session_id);
    assert!(
        !swarm_members
            .read()
            .await
            .contains_key("session_restore_missing"),
        "a failed restore must not register a nonexistent target member"
    );
    assert!(
        matches!(
            client_event_rx.try_recv(),
            Ok(ServerEvent::Error { id: 45, .. })
        ),
        "failed restore should report its failure without changing authority"
    );
    let temporary_authority = agent
        .lock()
        .await
        .structural_review_authority()
        .expect("failed restore must retain the temporary authority");
    temporary_authority
        .open_structural_review(StructuralReviewOpen {
            owner_session_id: owner_session_id.to_string(),
            reviewer_session_id: reviewer_session_id.to_string(),
            candidate: StructuralReviewCandidate {
                project_root: "/test/restore-authority".to_string(),
                cycle_id: 40,
                baseline: "restore-authority-base".to_string(),
                candidate_paths: vec!["src/failed-restore.rs".to_string()],
                candidate_digest: "failed-restore-authority-candidate".to_string(),
                policy_version: "structural-review/v2".to_string(),
                source_changed: true,
                risk_signals: Vec::new(),
            },
        })
        .await?;

    let writer_guard = writer.lock().await;

    let resume_task = tokio::spawn({
        let agent = Arc::clone(&agent);
        let provider = Arc::clone(&provider);
        let registry = registry.clone();
        let sessions = Arc::clone(&sessions);
        let shutdown_signals = Arc::clone(&shutdown_signals);
        let soft_interrupt_queues = Arc::clone(&soft_interrupt_queues);
        let client_connections = Arc::clone(&client_connections);
        let client_debug_state = Arc::clone(&client_debug_state);
        let swarm_members = Arc::clone(&swarm_members);
        let swarms_by_id = Arc::clone(&swarms_by_id);
        let file_touch = file_touch.clone();
        let channel_subscriptions = Arc::clone(&channel_subscriptions);
        let channel_subscriptions_by_session = Arc::clone(&channel_subscriptions_by_session);
        let swarm_plans = Arc::clone(&swarm_plans);
        let swarm_coordinators = Arc::clone(&swarm_coordinators);
        let structural_review_runtime = Arc::clone(&structural_review_runtime);
        let client_count = Arc::clone(&client_count);
        let writer = Arc::clone(&writer);
        let client_event_tx = client_event_tx.clone();
        let mcp_pool = Arc::clone(&mcp_pool);
        let event_history = Arc::clone(&event_history);
        let event_counter = Arc::clone(&event_counter);
        let swarm_event_tx = swarm_event_tx.clone();
        async move {
            handle_resume_session(
                46,
                target_session_id.to_string(),
                None,
                None,
                false,
                false,
                &mut client_selfdev,
                &mut client_session_id,
                "conn_restore",
                &agent,
                &provider,
                &registry,
                &sessions,
                &shutdown_signals,
                &soft_interrupt_queues,
                &client_connections,
                &client_debug_state,
                &swarm_members,
                &swarms_by_id,
                &file_touch,
                &channel_subscriptions,
                &channel_subscriptions_by_session,
                &swarm_plans,
                &swarm_coordinators,
                &structural_review_runtime,
                &client_count,
                &writer,
                "test-server",
                "🌿",
                &client_event_tx,
                &mcp_pool,
                &event_history,
                &event_counter,
                &swarm_event_tx,
                None,
                false,
            )
            .await
        }
    });

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let registered = {
                let members = swarm_members.read().await;
                members
                    .get(target_session_id)
                    .map(|member| member.event_txs.contains_key("conn_restore"))
                    .unwrap_or(false)
            };
            if registered {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| anyhow!("live event sender should register before history replay completes"))?;

    assert!(
        !resume_task.is_finished(),
        "resume should still be blocked on history replay while writer is locked"
    );

    drop(writer_guard);

    resume_task
        .await
        .map_err(|e| anyhow!("resume task join: {e}"))??;

    let events = collect_events_until_done(&mut client_event_rx, 46).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ServerEvent::Done { id } if *id == 46)),
        "expected Done event for restore resume, got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerEvent::Error { .. })),
        "restore resume should not emit error events: {events:?}"
    );

    {
        let members = swarm_members.read().await;
        assert!(
            !members.contains_key(temp_session_id),
            "the temporary member must be removed after normal restore"
        );
        assert_eq!(
            members
                .get(target_session_id)
                .map(|member| member.session_id.as_str()),
            Some(target_session_id),
            "normal restore must register the final member identity"
        );
    }
    assert_eq!(
        swarm_coordinators
            .read()
            .await
            .get(swarm_id)
            .map(String::as_str),
        Some(target_session_id),
        "normal restore must move the server coordinator identity"
    );
    let authority = agent
        .lock()
        .await
        .structural_review_authority()
        .expect("fresh temporary authority remains installed on the resumed agent");
    let receipt = authority
        .open_structural_review(StructuralReviewOpen {
            owner_session_id: owner_session_id.to_string(),
            reviewer_session_id: reviewer_session_id.to_string(),
            candidate: StructuralReviewCandidate {
                project_root: "/test/restore-authority".to_string(),
                cycle_id: 41,
                baseline: "restore-authority-base".to_string(),
                candidate_paths: vec!["src/restore.rs".to_string()],
                candidate_digest: "restore-authority-candidate".to_string(),
                policy_version: "structural-review/v2".to_string(),
                source_changed: true,
                risk_signals: Vec::new(),
            },
        })
        .await?;
    assert_eq!(receipt.owner_session_id, owner_session_id);
    assert_eq!(receipt.reviewer_session_id, reviewer_session_id);

    restore_runtime_dir(prev_runtime);
    Ok(())
}
