// sonar-file-size: waive=350 reason=pre-existing lifecycle integration suite; P3a adds focused RLM identity coexistence coverage, and test-suite decomposition is out of scope
use super::*;
use crate::message::{ContentBlock, Message, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use async_trait::async_trait;
use futures::stream;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[path = "client_comm_spawn_adapter_tests.rs"]
mod client_comm_spawn_adapter_tests;

#[tokio::test]
async fn superseded_client_cannot_answer_a_pending_stdin_request() {
    let now = Instant::now();
    let client_connections = Arc::new(RwLock::new(HashMap::from([(
        "conn_new".to_string(),
        ClientConnectionInfo {
            client_id: "conn_new".to_string(),
            session_id: "session_owned".to_string(),
            client_instance_id: None,
            debug_client_id: None,
            connected_at: now,
            last_seen: now,
            is_processing: false,
            current_tool_name: None,
            terminal_env: Vec::new(),
            disconnect_tx: mpsc::unbounded_channel().0,
        },
    )])));
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let stdin_responses = Arc::new(Mutex::new(HashMap::from([(
        "consent_stale".to_string(),
        response_tx,
    )])));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    handle_client_stdin_response(
        41,
        "consent_stale".to_string(),
        "approve".to_string(),
        "conn_old",
        "session_owned",
        &client_connections,
        &stdin_responses,
        &client_event_tx,
    )
    .await;

    assert!(
        response_rx.await.is_err(),
        "superseded owner response must drop rather than approve the operation"
    );
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Error { id: 41, message, .. })
            if message.contains("no longer controls")
    ));
}

#[tokio::test]
async fn current_client_can_answer_its_pending_stdin_request() -> Result<()> {
    let now = Instant::now();
    let client_connections = Arc::new(RwLock::new(HashMap::from([(
        "conn_current".to_string(),
        ClientConnectionInfo {
            client_id: "conn_current".to_string(),
            session_id: "session_owned".to_string(),
            client_instance_id: None,
            debug_client_id: None,
            connected_at: now,
            last_seen: now,
            is_processing: false,
            current_tool_name: None,
            terminal_env: Vec::new(),
            disconnect_tx: mpsc::unbounded_channel().0,
        },
    )])));
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let stdin_responses = Arc::new(Mutex::new(HashMap::from([(
        "consent_current".to_string(),
        response_tx,
    )])));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    handle_client_stdin_response(
        42,
        "consent_current".to_string(),
        "approve".to_string(),
        "conn_current",
        "session_owned",
        &client_connections,
        &stdin_responses,
        &client_event_tx,
    )
    .await;

    assert_eq!(response_rx.await?, "approve");
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Done { id: 42 })
    ));
    Ok(())
}

struct IsolatedRuntimeDir {
    _prev_runtime: Option<std::ffi::OsString>,
    _temp: tempfile::TempDir,
}
struct IsolatedReloadRecoveryEnv {
    prev_home: Option<std::ffi::OsString>,
    prev_runtime: Option<std::ffi::OsString>,
    _home: tempfile::TempDir,
    _runtime: tempfile::TempDir,
}
#[tokio::test]
async fn session_control_handle_does_not_wait_for_busy_agent_lock() {
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let background_signal = InterruptSignal::new();
    let stop_signal = InterruptSignal::new();
    let control = SessionControlHandle::new(
        "session_control_test",
        Arc::clone(&queue),
        background_signal.clone(),
        stop_signal.clone(),
    );

    let _busy_agent_lock = agent.lock().await;

    tokio::time::timeout(Duration::from_millis(100), async {
        assert!(control.queue_soft_interrupt(
            "please stop".to_string(),
            Vec::new(),
            true,
            SoftInterruptSource::User,
        ));
        control.request_cancel();
        assert!(control.request_background_current_tool());
        control.clear_soft_interrupts();
    })
    .await
    .expect("lock-free control operations should not wait for the agent mutex");

    assert!(stop_signal.is_set());
    assert!(background_signal.is_set());
    assert!(queue.lock().expect("queue lock").is_empty());
}

#[tokio::test]
async fn refreshed_session_control_handle_does_not_wait_for_busy_agent_lock() {
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let mut session = crate::session::Session::create_with_id(
        "session_busy_control_refresh".to_string(),
        None,
        None,
    );
    session.model = Some("panic-on-fork".to_string());
    let agent = Arc::new(Mutex::new(Agent::new_with_session(
        provider, registry, session, None,
    )));

    let stop_signal = InterruptSignal::new();
    let soft_interrupt_queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let shutdown_signals = Arc::new(RwLock::new(HashMap::from([(
        "session_busy_control_refresh".to_string(),
        stop_signal.clone(),
    )])));
    let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::from([(
        "session_busy_control_refresh".to_string(),
        soft_interrupt_queue,
    )])));

    let _busy_agent_lock = agent.lock().await;

    tokio::time::timeout(Duration::from_millis(100), async {
        let control = refresh_session_control_handle(
            "session_busy_control_refresh",
            &agent,
            &shutdown_signals,
            &soft_interrupt_queues,
        )
        .await;
        control.request_cancel();
    })
    .await
    .expect("refreshing a session control handle must not wait for the busy agent mutex");

    assert!(stop_signal.is_set());
}

#[tokio::test]
async fn busy_session_background_tool_signal_fires_via_registry_fallback() {
    // Regression: pressing Alt+B/Ctrl+B while a turn owns the agent mutex (e.g.
    // running `await_members`) used to silently no-op because the lock-free
    // `cancel_only` control handle dropped the background-tool signal
    // (BACKGROUND_TOOL_SIGNAL_FIRE result=no_signal_handle). Building a full
    // SessionControlHandle now registers the signal in a process-global registry
    // so the cancel-only fallback can still fire it without the agent lock.
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let session_id = "session_busy_background_signal_registry";
    let mut session = crate::session::Session::create_with_id(session_id.to_string(), None, None);
    session.model = Some("panic-on-fork".to_string());
    let agent = Arc::new(Mutex::new(Agent::new_with_session(
        provider, registry, session, None,
    )));

    let background_signal = {
        let agent_guard = agent.lock().await;
        agent_guard.background_tool_signal()
    };

    // Build a full control handle once (registers the background signal), then
    // simulate the busy-turn reconnect path which yields a cancel-only handle.
    let stop_signal = InterruptSignal::new();
    let soft_interrupt_queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let _full = SessionControlHandle::new(
        session_id,
        Arc::clone(&soft_interrupt_queue),
        background_signal.clone(),
        stop_signal.clone(),
    );

    let cancel_only =
        SessionControlHandle::cancel_only(session_id, soft_interrupt_queue, stop_signal);

    // The cancel-only handle has no directly-held background signal, yet it must
    // still fire the registered one.
    assert!(cancel_only.request_background_current_tool());
    assert!(background_signal.is_set());

    // Cleanup so the global registry does not leak across tests.
    crate::server::state::remove_background_tool_signal(session_id);
}

#[tokio::test]
async fn busy_agent_request_rejection_does_not_wait_for_agent_lock() {
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();

    let busy_agent_lock = agent.lock().await;
    let rejected = tokio::time::timeout(Duration::from_millis(100), async {
        reject_if_agent_busy_for_request(
            17,
            "rename_session",
            "session_busy_reject",
            true,
            &agent,
            &client_event_tx,
        )
    })
    .await
    .expect("busy-agent request rejection must not wait for the agent mutex");
    assert!(rejected);
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Error {
            id: 17,
            retry_after_secs: Some(1),
            ..
        })
    ));

    drop(busy_agent_lock);
    assert!(!reject_if_agent_busy_for_request(
        18,
        "rename_session",
        "session_busy_reject",
        false,
        &agent,
        &client_event_tx,
    ));
    assert!(client_event_rx.try_recv().is_err());
}

#[tokio::test]
async fn context_message_persists_without_starting_turn() {
    let _guard = crate::storage::lock_test_env();
    let _env = IsolatedReloadRecoveryEnv::new();
    let session_id = "session_context_only_no_reply";
    let forked = Arc::new(AtomicBool::new(false));
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::clone(&forked),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let mut session = crate::session::Session::create_with_id(session_id.to_string(), None, None);
    session.model = Some("panic-on-fork".to_string());
    let agent = Arc::new(Mutex::new(Agent::new_with_session(
        provider, registry, session, None,
    )));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let before = agent.lock().await.message_count();

    append_context_message(
        77,
        "remember this context",
        vec![("image/png".to_string(), "AAA".to_string())],
        session_id,
        false,
        &agent,
        &client_event_tx,
    )
    .await;

    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::ContextMessageAdded { id: 77 })
    ));
    assert!(client_event_rx.try_recv().is_err());
    assert!(!forked.load(Ordering::SeqCst));

    let persisted = crate::session::Session::load(session_id).expect("persisted session");
    assert_eq!(persisted.messages.len(), before + 1);
    let message = persisted.messages.last().unwrap();
    assert_eq!(format!("{:?}", message.role), "User");
    assert!(matches!(
        &message.content[0],
        ContentBlock::Image { media_type, data }
            if media_type == "image/png" && data == "AAA"
    ));
    assert!(matches!(
        &message.content[1],
        ContentBlock::Text { text, .. } if text == "remember this context"
    ));
}

#[tokio::test]
async fn context_message_rejects_while_busy_without_waiting_for_agent_lock() {
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let _busy_agent_lock = agent.lock().await;

    tokio::time::timeout(Duration::from_millis(100), async {
        append_context_message(
            78,
            "too busy",
            Vec::new(),
            "session_context_busy",
            true,
            &agent,
            &client_event_tx,
        )
        .await;
    })
    .await
    .expect("busy rejection must not wait for the agent mutex");

    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Error {
            id: 78,
            retry_after_secs: Some(1),
            ..
        })
    ));
}

#[tokio::test]
async fn cancel_without_local_task_still_signals_session_control() {
    let soft_interrupt_queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let stop_signal = InterruptSignal::new();
    let control = SessionControlHandle::cancel_only(
        "session_detached_cancel",
        soft_interrupt_queue,
        stop_signal.clone(),
    );
    // The point of this path is a turn this connection does not own (attach
    // after reload, server-initiated turn). Without a registered active turn
    // the cancel is a deliberate no-op, because arming the signal with nothing
    // running only kills the *next* message.
    let _active_turn = crate::turn_cancel_registry::register_active_turn(
        "session_detached_cancel",
        InterruptSignal::new(),
    );
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let mut client_is_processing = true;
    let mut message_id = Some(99);
    let mut session_id = Some("session_detached_cancel".to_string());
    let mut task = None;

    cancel_processing_message(
        &mut ProcessingState {
            client_is_processing: &mut client_is_processing,
            message_id: &mut message_id,
            session_id: &mut session_id,
            task: &mut task,
        },
        &control,
        &client_event_tx,
        &SwarmStatusRefs {
            members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            event_tx: &swarm_event_tx,
        },
        Some(99),
        None,
    )
    .await;

    assert!(stop_signal.is_set());
    assert!(!client_is_processing);
    assert!(message_id.is_none());
    assert!(session_id.is_none());
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Interrupted)
    ));
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Done { id: 99 })
    ));
}

#[tokio::test]
async fn cancel_request_stops_live_background_tasks_owned_by_the_session() {
    let session_id = "session_cancel_background_task";
    let task = crate::background::global()
        .spawn_with_notify(
            "bash",
            Some("must stop on Escape".to_string()),
            session_id,
            false,
            false,
            |_output_path| async move {
                std::future::pending::<anyhow::Result<crate::background::TaskResult>>().await
            },
        )
        .await;

    let control = SessionControlHandle::cancel_only(
        session_id,
        Arc::new(std::sync::Mutex::new(Vec::new())),
        InterruptSignal::new(),
    );
    let (client_event_tx, _client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let mut client_is_processing = false;
    let mut message_id = None;
    let mut active_session_id = Some(session_id.to_string());
    let mut processing_task = None;

    cancel_processing_message(
        &mut ProcessingState {
            client_is_processing: &mut client_is_processing,
            message_id: &mut message_id,
            session_id: &mut active_session_id,
            task: &mut processing_task,
        },
        &control,
        &client_event_tx,
        &SwarmStatusRefs {
            members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            event_tx: &swarm_event_tx,
        },
        Some(42),
        None,
    )
    .await;

    let status = crate::background::global()
        .status(&task.task_id)
        .await
        .expect("cancelled task status should exist");
    assert_eq!(status.status, crate::bus::BackgroundTaskStatus::Failed);
    assert_eq!(status.error.as_deref(), Some("Cancelled by user"));
}

/// Regression for issue #428: the detached-turn cancel path schedules a
/// deferred reset of the shared stop signal. That reset must be epoch-guarded:
/// if a newer cancel fires during the reset window (rapid repeated Esc), the
/// stale timer must not clear it, otherwise the running turn never observes
/// the interrupt and keeps generating.
#[tokio::test]
async fn deferred_cancel_reset_does_not_erase_newer_cancel() {
    let soft_interrupt_queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let stop_signal = InterruptSignal::new();
    let control = SessionControlHandle::cancel_only(
        "session_detached_cancel_race",
        Arc::clone(&soft_interrupt_queue),
        stop_signal.clone(),
    );
    // A turn owned by another connection is what makes this the signalling
    // path rather than the idle no-op; see the sibling test.
    let _active_turn = crate::turn_cancel_registry::register_active_turn(
        "session_detached_cancel_race",
        InterruptSignal::new(),
    );
    let (client_event_tx, _client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);

    let cancel_via_no_task_path = async |request_id: u64| {
        let mut client_is_processing = true;
        let mut message_id = Some(request_id);
        let mut session_id = Some("session_detached_cancel_race".to_string());
        let mut task = None;
        cancel_processing_message(
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut message_id,
                session_id: &mut session_id,
                task: &mut task,
            },
            &control,
            &client_event_tx,
            &SwarmStatusRefs {
                members: &swarm_members,
                swarms_by_id: &swarms_by_id,
                event_history: &event_history,
                event_counter: &event_counter,
                event_tx: &swarm_event_tx,
            },
            Some(request_id),
            None,
        )
        .await;
    };

    // First Esc: fires the signal and schedules a 500ms deferred reset.
    cancel_via_no_task_path(1).await;
    assert!(stop_signal.is_set());

    // 400ms later the user presses Esc again (turn still hasn't stopped).
    tokio::time::sleep(Duration::from_millis(400)).await;
    cancel_via_no_task_path(2).await;
    assert!(stop_signal.is_set());

    // The first press's timer expires now. It must NOT clear the second
    // press's still-unobserved cancel.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        stop_signal.is_set(),
        "stale deferred reset erased a newer cancel (issue #428)"
    );

    // The second press's own timer may still clear it afterwards.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        !stop_signal.is_set(),
        "the newest cancel's deferred reset should eventually clear the flag"
    );
}

impl IsolatedRuntimeDir {
    fn new() -> Self {
        let temp = tempfile::TempDir::new().expect("runtime dir");
        let prev_runtime = std::env::var_os("JCODE_RUNTIME_DIR");
        crate::env::set_var("JCODE_RUNTIME_DIR", temp.path());
        crate::server::clear_reload_marker();
        Self {
            _prev_runtime: prev_runtime,
            _temp: temp,
        }
    }
}

impl IsolatedReloadRecoveryEnv {
    fn new() -> Self {
        let home = tempfile::TempDir::new().expect("jcode home");
        let runtime = tempfile::TempDir::new().expect("runtime dir");
        let prev_home = std::env::var_os("JCODE_HOME");
        let prev_runtime = std::env::var_os("JCODE_RUNTIME_DIR");
        crate::env::set_var("JCODE_HOME", home.path());
        crate::env::set_var("JCODE_RUNTIME_DIR", runtime.path());
        crate::server::clear_reload_marker();
        Self {
            prev_home,
            prev_runtime,
            _home: home,
            _runtime: runtime,
        }
    }
}

impl Drop for IsolatedReloadRecoveryEnv {
    fn drop(&mut self) {
        crate::server::clear_reload_marker();
        if let Some(prev_home) = self.prev_home.take() {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
        if let Some(prev_runtime) = self.prev_runtime.take() {
            crate::env::set_var("JCODE_RUNTIME_DIR", prev_runtime);
        } else {
            crate::env::remove_var("JCODE_RUNTIME_DIR");
        }
    }
}

impl Drop for IsolatedRuntimeDir {
    fn drop(&mut self) {
        crate::server::clear_reload_marker();
        if let Some(prev_runtime) = self._prev_runtime.take() {
            crate::env::set_var("JCODE_RUNTIME_DIR", prev_runtime);
        } else {
            crate::env::remove_var("JCODE_RUNTIME_DIR");
        }
    }
}

/// Regression for issue #428: a turn actively streaming in this session but
/// NOT owned by the cancelling connection (no local task handle: post-reload
/// reattach, server-initiated wake turns, headless recovery) must abort
/// promptly even when the control handle's stop signal is a *different
/// instance* from the streaming agent's own `graceful_shutdown` signal.
///
/// Before the fix, `cancel_processing_message` hit the NO_LOCAL_TASK branch,
/// fired the stale handle-local signal (which nothing was listening to),
/// emitted `Interrupted` immediately, and the provider stream kept generating
/// for minutes ("Interrupting..." disappears, model keeps going, eventually
/// "Interrupted [x66]").
#[test]
fn cancel_aborts_detached_streaming_turn_with_stale_stop_signal() -> anyhow::Result<()> {
    let _lock = crate::storage::lock_test_env();
    let _env = IsolatedReloadRecoveryEnv::new();
    let session_id = "session_detached_streaming_cancel_428";

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let provider: Arc<dyn Provider> = Arc::new(NeverEndingStreamProvider::default());
        let registry = Registry::new(Arc::clone(&provider)).await;
        let mut session =
            crate::session::Session::create_with_id(session_id.to_string(), None, None);
        session.model = Some("never-ending-stream".to_string());
        let agent = Arc::new(Mutex::new(Agent::new_with_session(
            provider, registry, session, None,
        )));

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ServerEvent>();

        // Start the turn the way server-initiated paths do: no entry in any
        // connection's processing-task map.
        let turn_agent = Arc::clone(&agent);
        let turn = tokio::spawn(async move {
            process_message_streaming_mpsc(turn_agent, "stream forever", Vec::new(), None, event_tx)
                .await
        });

        // Wait until the provider stream is actively producing output.
        loop {
            match tokio::time::timeout(Duration::from_secs(5), event_rx.recv()).await {
                Ok(Some(ServerEvent::TextDelta { .. })) => break,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("event channel closed before streaming started"),
                Err(_) => panic!("turn never started streaming"),
            }
        }

        // The common streaming owner must hold the generation lease for this
        // server-initiated shape, not only for normal client tasks. An idle
        // recovery therefore declines before any exec/cooldown reservation.
        assert!(
            matches!(
                crate::server::reserve_idle_stalled_recovery().await,
                crate::server::IdleRecoveryReservation::ActiveGenerations { count } if count >= 1
            ),
            "an active common streaming turn must block idle recovery"
        );

        // Esc arrives on a connection that does not own the task. Its control
        // handle holds a stop signal instance that is NOT the streaming
        // agent's graceful_shutdown signal (stale/lost registration).
        let stale_stop_signal = InterruptSignal::new();
        let control = SessionControlHandle::cancel_only(
            session_id,
            Arc::new(std::sync::Mutex::new(Vec::new())),
            stale_stop_signal.clone(),
        );
        let (client_event_tx, _client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);
        let mut client_is_processing = false;
        let mut message_id = None;
        let mut cancel_session_id = None;
        let mut task = None;

        cancel_processing_message(
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut message_id,
                session_id: &mut cancel_session_id,
                task: &mut task,
            },
            &control,
            &client_event_tx,
            &SwarmStatusRefs {
                members: &swarm_members,
                swarms_by_id: &swarms_by_id,
                event_history: &event_history,
                event_counter: &event_counter,
                event_tx: &swarm_event_tx,
            },
            Some(1),
            None,
        )
        .await;

        // The streaming turn must observe the cancel and stop promptly, not
        // minutes later when the provider happens to finish (issue #428).
        let result = tokio::time::timeout(Duration::from_secs(2), turn)
            .await
            .expect("streaming turn must abort promptly after cancel (issue #428)")
            .expect("turn task join");
        result.expect("cancelled turn should checkpoint cleanly");

        // The turn is over, so its cancel registration must be gone and the
        // agent's own signal must be reset so the *next* turn is not aborted
        // by the consumed cancel.
        assert!(
            crate::turn_cancel_registry::active_turn_signals(session_id).is_empty(),
            "finished turn must unregister its cancel signal"
        );
        let agent_signal = {
            let agent_guard = agent.lock().await;
            agent_guard.graceful_shutdown_signal()
        };
        assert!(
            !agent_signal.is_set(),
            "consumed cancel must not leak into the next turn"
        );
    });
    Ok(())
}

#[test]
fn system_display_role_streaming_generation_blocks_idle_recovery() -> anyhow::Result<()> {
    let _lock = crate::storage::lock_test_env();
    let _env = IsolatedReloadRecoveryEnv::new();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let provider: Arc<dyn Provider> = Arc::new(NeverEndingStreamProvider::default());
        let registry = Registry::new(Arc::clone(&provider)).await;
        let mut session =
            crate::session::Session::create_with_id("session_system_w13".to_string(), None, None);
        session.model = Some("never-ending-system-stream".to_string());
        let agent = Arc::new(Mutex::new(Agent::new_with_session(
            provider, registry, session, None,
        )));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ServerEvent>();
        let turn_agent = Arc::clone(&agent);
        let turn = tokio::spawn(async move {
            process_message_streaming_mpsc_with_display_role(
                turn_agent,
                "system generation",
                Vec::new(),
                None,
                event_tx,
                Some(crate::session::StoredDisplayRole::System),
            )
            .await
        });

        loop {
            match tokio::time::timeout(Duration::from_secs(5), event_rx.recv()).await {
                Ok(Some(ServerEvent::TextDelta { .. })) => break,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("system stream closed before generation began"),
                Err(_) => panic!("system display-role generation never began"),
            }
        }

        assert!(
            matches!(
                crate::server::reserve_idle_stalled_recovery().await,
                crate::server::IdleRecoveryReservation::ActiveGenerations { count } if count >= 1
            ),
            "an active system display-role generation must block idle recovery"
        );

        turn.abort();
        assert!(
            turn.await
                .expect_err("aborted system turn must cancel")
                .is_cancelled()
        );
        write_active_recovery_cooldown();
        assert!(
            matches!(
                crate::server::reserve_idle_stalled_recovery().await,
                crate::server::IdleRecoveryReservation::CoolingDown
            ),
            "cancelling a system stream must release its generation lease before recovery checks"
        );
        let persisted = crate::session::Session::load("session_system_w13")
            .expect("system display-role generation must persist its session");
        assert_eq!(
            persisted
                .messages
                .last()
                .and_then(|message| message.display_role),
            Some(crate::session::StoredDisplayRole::System),
            "the lease-owned boundary must preserve system display-role metadata"
        );
    });
    Ok(())
}

#[test]
fn capture_generation_blocks_idle_recovery_and_releases_on_cancel() -> anyhow::Result<()> {
    let _lock = crate::storage::lock_test_env();
    let _env = IsolatedReloadRecoveryEnv::new();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let started = Arc::new(AtomicBool::new(false));
        let provider: Arc<dyn Provider> = Arc::new(NeverEndingStreamProvider {
            started: Arc::clone(&started),
        });
        let registry = Registry::new(Arc::clone(&provider)).await;
        let mut session =
            crate::session::Session::create_with_id("session_capture_w13".to_string(), None, None);
        session.model = Some("never-ending-capture".to_string());
        let agent = Arc::new(Mutex::new(Agent::new_with_session(
            provider, registry, session, None,
        )));
        let turn_agent = Arc::clone(&agent);
        let turn = tokio::spawn(async move {
            let mut agent = turn_agent.lock().await;
            agent.run_once_capture("capture generation").await
        });

        tokio::time::timeout(Duration::from_secs(5), async {
            while !started.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("capture generation never reached the provider");

        assert!(
            matches!(
                crate::server::reserve_idle_stalled_recovery().await,
                crate::server::IdleRecoveryReservation::ActiveGenerations { count } if count >= 1
            ),
            "an active capture generation must block idle recovery"
        );

        turn.abort();
        assert!(
            turn.await
                .expect_err("aborted capture turn must cancel")
                .is_cancelled()
        );
        write_active_recovery_cooldown();
        assert!(
            matches!(
                crate::server::reserve_idle_stalled_recovery().await,
                crate::server::IdleRecoveryReservation::CoolingDown
            ),
            "cancelling a capture turn must release its generation lease before recovery checks"
        );
    });
    Ok(())
}

fn write_active_recovery_cooldown() {
    crate::storage::write_json(
        &crate::server::reload_state::stalled_recovery_cooldown_path(),
        &serde_json::json!({
            "pid": std::process::id(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    )
    .expect("write active recovery cooldown");
}

/// A cancel that arrives while the session is idle must not arm the cancel
/// signal at all.
///
/// The no-local-task branch cannot tell an idle session from one whose turn
/// another connection owns, so it used to fire the signal and clear it on a
/// 500ms timer. Any message sent inside that window began with the flag
/// already set and was aborted the instant it started: no reply, no error,
/// just a message that vanished. Pressing Esc on an idle prompt and typing
/// immediately is an ordinary thing to do, so this must be a true no-op.
#[test]
fn idle_cancel_does_not_arm_the_signal_for_the_next_turn() -> anyhow::Result<()> {
    let _lock = crate::storage::lock_test_env();
    let _env = IsolatedReloadRecoveryEnv::new();
    let session_id = "session_idle_cancel_noop";

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let stop_signal = InterruptSignal::new();
        let control = SessionControlHandle::cancel_only(
            session_id,
            Arc::new(std::sync::Mutex::new(Vec::new())),
            stop_signal.clone(),
        );
        let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);
        let mut client_is_processing = false;
        let mut message_id = None;
        let mut cancel_session_id = None;
        let mut task = None;

        assert!(
            !crate::turn_cancel_registry::has_active_turn(session_id),
            "test precondition: the session must be idle"
        );

        cancel_processing_message(
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut message_id,
                session_id: &mut cancel_session_id,
                task: &mut task,
            },
            &control,
            &client_event_tx,
            &SwarmStatusRefs {
                members: &swarm_members,
                swarms_by_id: &swarms_by_id,
                event_history: &event_history,
                event_counter: &event_counter,
                event_tx: &swarm_event_tx,
            },
            Some(1),
            None,
        )
        .await;

        assert!(
            !stop_signal.is_set(),
            "an idle cancel must not arm the stop signal; the next turn would die instantly"
        );
        // The client still learns the cancel was handled, so a UI showing
        // "Interrupting..." resolves rather than hanging.
        match client_event_rx.try_recv() {
            Ok(ServerEvent::Interrupted) => {}
            other => panic!("idle cancel must still report Interrupted, got {other:?}"),
        }
    });
    Ok(())
}

struct PanicOnForkProvider {
    forked: Arc<AtomicBool>,
}

/// Streams text deltas forever (one every 20ms) until dropped. Stands in for
/// a live provider stream that only stops when the turn observes a cancel.
#[derive(Default)]
struct NeverEndingStreamProvider {
    started: Arc<AtomicBool>,
}

/// Emits one frame large enough to exceed a non-draining Unix peer's buffer.
/// It is fully local, so the W13 transport test never needs a real provider.
struct LargeDeltaProvider {
    emitted: Arc<AtomicBool>,
}

#[async_trait]
impl Provider for LargeDeltaProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let emitted = Arc::clone(&self.emitted);
        Ok(Box::pin(stream::unfold(0_u8, move |step| {
            let emitted = Arc::clone(&emitted);
            async move {
                match step {
                    0 => {
                        emitted.store(true, Ordering::Release);
                        Some((Ok(StreamEvent::TextDelta("x".repeat(16 * 1024 * 1024))), 1))
                    }
                    1 => Some((Ok(StreamEvent::MessageEnd { stop_reason: None }), 2)),
                    _ => None,
                }
            }
        })))
    }

    fn name(&self) -> &str {
        "large-delta"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            emitted: Arc::clone(&self.emitted),
        })
    }
}

#[async_trait]
impl Provider for NeverEndingStreamProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.started.store(true, Ordering::Release);
        Ok(Box::pin(stream::unfold(0u64, |n| async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Some((Ok(StreamEvent::TextDelta(format!("token{} ", n))), n + 1))
        })))
    }

    fn name(&self) -> &str {
        "never-ending-stream"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            started: Arc::clone(&self.started),
        })
    }
}

#[derive(Clone, Default)]
struct CompleteImmediatelyProvider;

#[async_trait]
impl Provider for CompleteImmediatelyProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        Ok(Box::pin(stream::iter(vec![Ok(StreamEvent::MessageEnd {
            stop_reason: None,
        })])))
    }

    fn name(&self) -> &str {
        "complete-immediately"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

#[derive(Clone, Default)]
struct FanoutStreamProvider;

#[async_trait]
impl Provider for FanoutStreamProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        Ok(Box::pin(stream::unfold(0_u8, |step| async move {
            match step {
                0 => Some((Ok(StreamEvent::TextDelta("before attach".to_string())), 1)),
                1 => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Some((Ok(StreamEvent::TextDelta("after attach".to_string())), 2))
                }
                2 => Some((
                    Ok(StreamEvent::MessageEnd {
                        stop_reason: Some("end_turn".to_string()),
                    }),
                    3,
                )),
                _ => None,
            }
        })))
    }

    fn name(&self) -> &str {
        "fanout-stream"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Provider for PanicOnForkProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        panic!("complete should never run in lightweight control test")
    }

    fn name(&self) -> &str {
        "panic-on-fork"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        self.forked.store(true, Ordering::SeqCst);
        panic!("fork should not run for lightweight control requests")
    }
}

#[test]
fn ping_request_is_lightweight_control_request() {
    assert!((Request::Ping { id: 1 }).is_lightweight_control_request());
}

fn subscribe_request(working_dir: Option<&str>) -> Request {
    Request::Subscribe {
        supports_pdf_panels: false,
        id: 1,
        working_dir: working_dir.map(str::to_string),
        selfdev: None,
        target_session_id: None,
        client_instance_id: None,
        rlm_client: None,
        client_has_local_history: false,
        allow_session_takeover: false,
        crash_on_disconnect: false,
        continue_on_disconnect: false,
        terminal_env: Vec::new(),
    }
}

#[test]
fn initial_subscribe_requires_an_absolute_client_working_dir() {
    for invalid in [None, Some(""), Some("relative/project")] {
        let error = initial_subscribe_working_dir(&subscribe_request(invalid))
            .expect_err("invalid client cwd must be rejected before session creation");
        assert!(error.contains("working_dir") || error.contains("working directory"));
    }

    let absolute = std::env::temp_dir().join("jcode-client-project");
    assert_eq!(
        initial_subscribe_working_dir(&subscribe_request(absolute.to_str()))
            .expect("absolute client cwd"),
        absolute.to_string_lossy()
    );

    let error = initial_subscribe_working_dir(&Request::GetState { id: 2 })
        .expect_err("stateful requests must not create an unbound session");
    assert!(error.contains("must Subscribe"));
}

#[test]
fn remote_subscribe_requires_an_existing_server_directory() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("not-a-directory");
    std::fs::write(&file, "file")?;
    let missing = directory.path().join("missing");
    for path in [&file, &missing] {
        let mut request = subscribe_request(path.to_str());
        assert!(
            initial_subscribe_working_dir(&request).is_ok(),
            "local subscription behavior remains unchanged"
        );
        if let Request::Subscribe {
            continue_on_disconnect,
            ..
        } = &mut request
        {
            *continue_on_disconnect = true;
        }
        assert!(
            initial_subscribe_working_dir(&request)
                .unwrap_err()
                .contains("must exist and be a directory on the server")
        );
    }
    assert_eq!(
        validated_subscribe_working_dir(directory.path().to_str(), true)
            .expect("existing directory"),
        directory.path().to_str().unwrap()
    );
    Ok(())
}

#[tokio::test]
async fn new_client_agent_stamps_client_cwd_into_initial_context() {
    let provider: Arc<dyn Provider> = Arc::new(CompleteImmediatelyProvider);
    let registry = Registry::new(Arc::clone(&provider)).await;
    let client_cwd = std::env::temp_dir().join("jcode-authoritative-client-project");
    let client_cwd = client_cwd.to_string_lossy();
    let agent = Agent::new_with_initial_working_dir(provider, registry, Some(&client_cwd));

    assert_eq!(agent.working_dir(), Some(client_cwd.as_ref()));
    let context = agent.messages()[0].content_preview();
    assert!(
        context.contains(&format!("Working directory: {client_cwd}")),
        "initial context must be created from the client cwd: {context}"
    );
}

#[test]
fn server_reload_starting_is_true_only_for_recent_starting_marker() {
    let _guard = crate::storage::lock_test_env();
    let _runtime = IsolatedRuntimeDir::new();

    assert!(!server_reload_starting());

    crate::server::write_reload_state(
        "reload-lifecycle-test",
        "test-hash",
        crate::server::ReloadPhase::Starting,
        Some("session_test_reload".to_string()),
    );
    assert!(server_reload_starting());

    crate::server::write_reload_state(
        "reload-lifecycle-test",
        "test-hash",
        crate::server::ReloadPhase::SocketReady,
        Some("session_test_reload".to_string()),
    );
    assert!(!server_reload_starting());
}

#[test]
fn reload_starting_rejects_new_turn_without_spawning_processing_task() {
    let _guard = crate::storage::lock_test_env();
    let _runtime = IsolatedRuntimeDir::new();
    crate::server::write_reload_state(
        "reload-lifecycle-starting",
        "test-hash",
        crate::server::ReloadPhase::Starting,
        Some("session_guard".to_string()),
    );

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let forked = Arc::new(AtomicBool::new(false));
        let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
            forked: Arc::clone(&forked),
        });
        let registry = Registry::new(Arc::clone(&provider)).await;
        let mut session =
            crate::session::Session::create_with_id("session_guard".to_string(), None, None);
        session.model = Some("panic-on-fork".to_string());
        let agent = Arc::new(Mutex::new(Agent::new_with_session(
            provider, registry, session, None,
        )));

        let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
        let (processing_done_tx, mut processing_done_rx) = mpsc::unbounded_channel();
        let mut client_is_processing = false;
        let mut processing_message_id = None;
        let mut processing_session_id = None;
        let mut processing_task = None;
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);

        start_processing_message(
            ProcessingMessage {
                id: 42,
                content: "do not start during reload".to_string(),
                images: Vec::new(),
                system_reminder: None,
                active_skill: None,
            },
            "session_guard",
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut processing_message_id,
                session_id: &mut processing_session_id,
                task: &mut processing_task,
            },
            &agent,
            &client_event_tx,
            &processing_done_tx,
            Vec::new(),
            &SwarmStatusRefs {
                members: &swarm_members,
                swarms_by_id: &swarms_by_id,
                event_history: &event_history,
                event_counter: &event_counter,
                event_tx: &swarm_event_tx,
            },
        )
        .await;

        let event = client_event_rx
            .recv()
            .await
            .expect("reload event should be sent to client");
        assert!(matches!(event, ServerEvent::Reloading { new_socket: None }));
        assert!(
            client_event_rx.try_recv().is_err(),
            "reload guard should only emit the reload notification"
        );
        assert!(!client_is_processing);
        assert_eq!(processing_message_id, None);
        assert_eq!(processing_session_id, None);
        assert!(processing_task.is_none());
        assert!(processing_done_rx.try_recv().is_err());
        assert!(
            !forked.load(Ordering::SeqCst),
            "rejecting during reload should not fork or invoke provider work"
        );
    });
}

#[tokio::test]
async fn client_initiated_turn_fans_out_stream_and_terminal_events_to_live_attachments() {
    let _guard = crate::storage::lock_test_env();
    let _runtime = IsolatedRuntimeDir::new();
    let session_id = "session_live_attachment_fanout";

    let provider: Arc<dyn Provider> = Arc::new(FanoutStreamProvider);
    let registry = Registry::new(Arc::clone(&provider)).await;
    let mut session = crate::session::Session::create_with_id(session_id.to_string(), None, None);
    session.model = Some("fanout-stream".to_string());
    let agent = Arc::new(Mutex::new(Agent::new_with_session(
        provider, registry, session, None,
    )));

    let (origin_tx, mut origin_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let (attached_tx, mut attached_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(
        session_id.to_string(),
        SwarmMember {
            session_id: session_id.to_string(),
            event_tx: origin_tx.clone(),
            event_txs: HashMap::from([("origin".to_string(), origin_tx.clone())]),
            working_dir: None,
            swarm_id: None,
            swarm_enabled: false,
            status: "ready".to_string(),
            detail: None,
            task_label: None,
            friendly_name: None,
            report_back_to_session_id: None,
            latest_completion_report: None,
            role: "agent".to_string(),
            joined_at: Instant::now(),
            last_status_change: Instant::now(),
            is_headless: false,
            output_tail: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        },
    )])));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let (processing_done_tx, mut processing_done_rx) = mpsc::unbounded_channel();
    let mut client_is_processing = false;
    let mut processing_message_id = None;
    let mut processing_session_id = None;
    let mut processing_task = None;

    start_processing_message(
        ProcessingMessage {
            id: 479,
            content: "stream to every attachment".to_string(),
            images: Vec::new(),
            system_reminder: None,
            active_skill: None,
        },
        session_id,
        &mut ProcessingState {
            client_is_processing: &mut client_is_processing,
            message_id: &mut processing_message_id,
            session_id: &mut processing_session_id,
            task: &mut processing_task,
        },
        &agent,
        &origin_tx,
        &processing_done_tx,
        Vec::new(),
        &SwarmStatusRefs {
            members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            event_tx: &swarm_event_tx,
        },
    )
    .await;

    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), origin_rx.recv())
            .await
            .expect("origin should receive the initial stream event promptly")
            .expect("origin event channel should remain open");
        if matches!(event, ServerEvent::TextDelta { ref text } if text == "before attach") {
            break;
        }
    }

    crate::server::register_session_event_sender(
        &swarm_members,
        session_id,
        "attached",
        attached_tx,
    )
    .await;

    for rx in [&mut origin_rx, &mut attached_rx] {
        let mut saw_post_attach_delta = false;
        let mut saw_message_end = false;
        let mut saw_done = false;
        while !saw_post_attach_delta || !saw_message_end || !saw_done {
            let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("attachment should receive streamed event promptly")
                .expect("attachment event channel should remain open");
            saw_post_attach_delta |= matches!(
                event,
                ServerEvent::TextDelta { ref text } if text == "after attach"
            );
            if matches!(event, ServerEvent::MessageEnd { .. }) {
                assert!(!saw_done, "MessageEnd must precede the terminal Done event");
                saw_message_end = true;
            }
            saw_done |= matches!(event, ServerEvent::Done { id: 479 });
        }
    }

    let (done_id, result, _) =
        tokio::time::timeout(Duration::from_secs(2), processing_done_rx.recv())
            .await
            .expect("processing should complete promptly")
            .expect("processing completion channel should remain open");
    assert_eq!(done_id, 479);
    result.expect("turn should complete successfully");

    if let Some(handle) = processing_task.take() {
        handle.await.expect("processing task join");
    }
}

#[test]
fn accepted_reload_recovery_continuation_marks_intent_delivered() -> anyhow::Result<()> {
    let _lock = crate::storage::lock_test_env();
    let _env = IsolatedReloadRecoveryEnv::new();
    let session_id = "session_accepted_reload_recovery";
    let continuation = "stored continuation accepted by server";

    super::super::reload_recovery::persist_intent(
        "reload-accepted-continuation",
        session_id,
        super::super::reload_recovery::ReloadRecoveryRole::InterruptedPeer,
        crate::tool::selfdev::ReloadRecoveryDirective {
            reconnect_notice: Some("stored notice".to_string()),
            continuation_message: continuation.to_string(),
        },
        "synthetic accepted continuation test",
    )?;
    assert!(super::super::reload_recovery::has_pending_for_session(
        session_id
    ));

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let provider: Arc<dyn Provider> = Arc::new(CompleteImmediatelyProvider);
        let registry = Registry::new(Arc::clone(&provider)).await;
        let mut session =
            crate::session::Session::create_with_id(session_id.to_string(), None, None);
        session.model = Some("complete-immediately".to_string());
        let agent = Arc::new(Mutex::new(Agent::new_with_session(
            provider, registry, session, None,
        )));

        let (client_event_tx, _client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
        let (processing_done_tx, mut processing_done_rx) = mpsc::unbounded_channel();
        let mut client_is_processing = false;
        let mut processing_message_id = None;
        let mut processing_session_id = None;
        let mut processing_task = None;
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);

        start_processing_message(
            ProcessingMessage {
                id: 77,
                content: "continue after reload".to_string(),
                images: Vec::new(),
                system_reminder: Some(continuation.to_string()),
                active_skill: None,
            },
            session_id,
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut processing_message_id,
                session_id: &mut processing_session_id,
                task: &mut processing_task,
            },
            &agent,
            &client_event_tx,
            &processing_done_tx,
            Vec::new(),
            &SwarmStatusRefs {
                members: &swarm_members,
                swarms_by_id: &swarms_by_id,
                event_history: &event_history,
                event_counter: &event_counter,
                event_tx: &swarm_event_tx,
            },
        )
        .await;

        assert!(client_is_processing);
        assert_eq!(processing_message_id, Some(77));
        assert_eq!(processing_session_id.as_deref(), Some(session_id));
        assert!(processing_task.is_some());
        assert!(
            !super::super::reload_recovery::has_pending_for_session(session_id),
            "server acceptance of the exact hidden continuation should consume the durable intent"
        );

        let (done_id, result, _report) =
            tokio::time::timeout(std::time::Duration::from_secs(5), processing_done_rx.recv())
                .await
                .expect("processing task should finish")
                .expect("processing task should report completion");
        assert_eq!(done_id, 77);
        result?;
        if let Some(handle) = processing_task.take() {
            handle.await.expect("processing task join");
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

#[test]
fn reload_starting_rejects_new_turns_for_multiple_sessions() {
    let _guard = crate::storage::lock_test_env();
    let _runtime = IsolatedRuntimeDir::new();
    crate::server::write_reload_state(
        "reload-lifecycle-multi-starting",
        "test-hash",
        crate::server::ReloadPhase::Starting,
        Some("session_alpha".to_string()),
    );

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let forked = Arc::new(AtomicBool::new(false));
        let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
            forked: Arc::clone(&forked),
        });
        let registry = Registry::new(Arc::clone(&provider)).await;
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);

        for (message_id, session_id) in [
            (101, "session_alpha"),
            (102, "session_beta"),
            (103, "session_gamma"),
        ] {
            let mut session =
                crate::session::Session::create_with_id(session_id.to_string(), None, None);
            session.model = Some("panic-on-fork".to_string());
            let agent = Arc::new(Mutex::new(Agent::new_with_session(
                Arc::clone(&provider),
                registry.clone(),
                session,
                None,
            )));

            let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
            let (processing_done_tx, mut processing_done_rx) = mpsc::unbounded_channel();
            let mut client_is_processing = false;
            let mut processing_message_id = None;
            let mut processing_session_id = None;
            let mut processing_task = None;

            start_processing_message(
                ProcessingMessage {
                    id: message_id,
                    content: format!("do not start {session_id} during reload"),
                    images: Vec::new(),
                    system_reminder: None,
                    active_skill: None,
                },
                session_id,
                &mut ProcessingState {
                    client_is_processing: &mut client_is_processing,
                    message_id: &mut processing_message_id,
                    session_id: &mut processing_session_id,
                    task: &mut processing_task,
                },
                &agent,
                &client_event_tx,
                &processing_done_tx,
                Vec::new(),
                &SwarmStatusRefs {
                    members: &swarm_members,
                    swarms_by_id: &swarms_by_id,
                    event_history: &event_history,
                    event_counter: &event_counter,
                    event_tx: &swarm_event_tx,
                },
            )
            .await;

            let event = tokio::time::timeout(
                std::time::Duration::from_millis(250),
                client_event_rx.recv(),
            )
            .await
            .expect("reload guard should emit promptly for every session")
            .expect("reload event should be sent to client");
            assert!(
                matches!(event, ServerEvent::Reloading { new_socket: None }),
                "expected Reloading event for {session_id}, got {event:?}"
            );
            assert!(
                client_event_rx.try_recv().is_err(),
                "reload guard should only emit one reload notification for {session_id}"
            );
            assert!(
                !client_is_processing,
                "{session_id} should not enter processing during reload"
            );
            assert_eq!(processing_message_id, None);
            assert_eq!(processing_session_id, None);
            assert!(
                processing_task.is_none(),
                "{session_id} should not spawn a processing task during reload"
            );
            assert!(processing_done_rx.try_recv().is_err());
        }

        assert!(
            !forked.load(Ordering::SeqCst),
            "rejecting multiple sessions during reload should not fork or invoke provider work"
        );
    });
}

#[tokio::test]
async fn lightweight_comm_request_skips_full_session_initialization() {
    let (server_stream, client_stream) = crate::transport::Stream::pair().expect("socket pair");
    let forked = Arc::new(AtomicBool::new(false));
    let provider_template: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::clone(&forked),
    });

    let sessions: SessionAgents = Arc::new(RwLock::new(HashMap::new()));
    let global_session_id = Arc::new(RwLock::new(String::new()));
    let client_count = Arc::new(RwLock::new(0usize));
    let client_connections = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let shared_context = Arc::new(RwLock::new(HashMap::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::new()));
    let structural_review_runtime = crate::server::StructuralReviewRuntime::new(
        Arc::clone(&swarm_members),
        Arc::clone(&swarm_coordinators),
    );
    let file_touch = FileTouchService::new();
    let channel_subscriptions = Arc::new(RwLock::new(HashMap::new()));
    let channel_subscriptions_by_session = Arc::new(RwLock::new(HashMap::new()));
    let client_debug_state = Arc::new(RwLock::new(ClientDebugState::default()));
    let (_debug_response_tx, _) = broadcast::channel(8);
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let (_global_event_tx, _) = broadcast::channel(8);
    let global_is_processing = Arc::new(RwLock::new(false));
    let shutdown_signals = Arc::new(RwLock::new(HashMap::new()));
    let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::new()));
    let mcp_pool = Arc::new(crate::mcp::SharedMcpPool::from_default_config());

    let server_task = tokio::spawn(handle_client(
        server_stream,
        Arc::clone(&sessions),
        _global_event_tx,
        provider_template,
        global_is_processing,
        global_session_id,
        client_count,
        Arc::clone(&client_connections),
        swarm_members,
        swarms_by_id,
        shared_context,
        swarm_plans,
        swarm_coordinators,
        file_touch,
        channel_subscriptions,
        channel_subscriptions_by_session,
        client_debug_state,
        _debug_response_tx,
        event_history,
        event_counter,
        swarm_event_tx,
        "jcode-test".to_string(),
        "🧪".to_string(),
        mcp_pool,
        shutdown_signals,
        soft_interrupt_queues,
        AwaitMembersRuntime::default(),
        SwarmMutationRuntime::default(),
        structural_review_runtime,
    ));

    let (client_reader, mut client_writer) = client_stream.into_split();
    let mut client_reader = BufReader::new(client_reader);
    let request = Request::CommList {
        id: 7,
        session_id: "not-in-swarm".to_string(),
    };
    let payload = serde_json::to_string(&request).expect("serialize request") + "\n";
    client_writer
        .write_all(payload.as_bytes())
        .await
        .expect("write request");

    let mut line = String::new();
    client_reader
        .read_line(&mut line)
        .await
        .expect("read ack bytes");
    let ack = decode_request_or_event(&line);
    assert!(matches!(ack, ServerEvent::Ack { id: 7 }));

    line.clear();
    client_reader
        .read_line(&mut line)
        .await
        .expect("read terminal response");
    let response = decode_request_or_event(&line);
    match response {
        ServerEvent::Error { id, message, .. } => {
            assert_eq!(id, 7);
            assert!(message.contains("Not in a swarm"));
        }
        other => panic!("expected error response, got {other:?}"),
    }

    line.clear();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), client_reader.read_line(&mut line))
            .await
            .expect("non-Ping lightweight command must close its one-shot connection")
            .expect("read EOF"),
        0,
    );
    drop(client_writer);
    server_task
        .await
        .expect("server task join")
        .expect("server task result");

    assert!(
        !forked.load(Ordering::SeqCst),
        "lightweight control request should not fork a provider"
    );
    assert!(
        client_connections.read().await.is_empty(),
        "lightweight control request should not register a live client session"
    );
    assert!(
        sessions.read().await.is_empty(),
        "lightweight control request should not allocate a live agent session"
    );
}

/// Integration boundary for W13. One real client connection stops reading after
/// a local provider emits a 16MiB frame. Its event-forwarder deadline must
/// trigger connection cleanup, while another real client still completes a
/// provider-free CommList request through the same live server state.
#[tokio::test]
async fn non_draining_event_peer_is_cleaned_up_without_blocking_peer_control_request() {
    let sessions: SessionAgents = Arc::new(RwLock::new(HashMap::new()));
    let global_session_id = Arc::new(RwLock::new(String::new()));
    let client_count = Arc::new(RwLock::new(0usize));
    let client_connections = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let shared_context = Arc::new(RwLock::new(HashMap::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::new()));
    let structural_review_runtime = crate::server::StructuralReviewRuntime::new(
        Arc::clone(&swarm_members),
        Arc::clone(&swarm_coordinators),
    );
    let file_touch = FileTouchService::new();
    let channel_subscriptions = Arc::new(RwLock::new(HashMap::new()));
    let channel_subscriptions_by_session = Arc::new(RwLock::new(HashMap::new()));
    let client_debug_state = Arc::new(RwLock::new(ClientDebugState::default()));
    let (debug_response_tx, _) = broadcast::channel(8);
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let (global_event_tx, _) = broadcast::channel(8);
    let global_is_processing = Arc::new(RwLock::new(false));
    let shutdown_signals = Arc::new(RwLock::new(HashMap::new()));
    let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::new()));
    let mcp_pool = Arc::new(crate::mcp::SharedMcpPool::from_default_config());

    let spawn_client = |stream: crate::transport::Stream, provider_template: Arc<dyn Provider>| {
        tokio::spawn(handle_client(
            stream,
            Arc::clone(&sessions),
            global_event_tx.clone(),
            provider_template,
            Arc::clone(&global_is_processing),
            Arc::clone(&global_session_id),
            Arc::clone(&client_count),
            Arc::clone(&client_connections),
            Arc::clone(&swarm_members),
            Arc::clone(&swarms_by_id),
            Arc::clone(&shared_context),
            Arc::clone(&swarm_plans),
            Arc::clone(&swarm_coordinators),
            file_touch.clone(),
            Arc::clone(&channel_subscriptions),
            Arc::clone(&channel_subscriptions_by_session),
            Arc::clone(&client_debug_state),
            debug_response_tx.clone(),
            Arc::clone(&event_history),
            Arc::clone(&event_counter),
            swarm_event_tx.clone(),
            "w13-test".to_string(),
            "🧪".to_string(),
            Arc::clone(&mcp_pool),
            Arc::clone(&shutdown_signals),
            Arc::clone(&soft_interrupt_queues),
            AwaitMembersRuntime::default(),
            SwarmMutationRuntime::default(),
            Arc::clone(&structural_review_runtime),
        ))
    };

    let (blocked_server, blocked_client) =
        crate::transport::Stream::pair().expect("blocked client pair");
    let emitted = Arc::new(AtomicBool::new(false));
    let blocked_provider: Arc<dyn Provider> = Arc::new(LargeDeltaProvider {
        emitted: Arc::clone(&emitted),
    });
    let blocked_task = spawn_client(blocked_server, blocked_provider);
    let (_blocked_reader, mut blocked_writer) = blocked_client.into_split();
    let working_dir = std::env::current_dir()
        .expect("current directory")
        .to_string_lossy()
        .into_owned();
    for request in [
        Request::Subscribe {
            continue_on_disconnect: false,
            crash_on_disconnect: false,
            supports_pdf_panels: false,
            id: 1,
            working_dir: Some(working_dir),
            selfdev: None,
            target_session_id: None,
            client_instance_id: None,
            rlm_client: None,
            client_has_local_history: false,
            allow_session_takeover: false,
            terminal_env: Vec::new(),
        },
        Request::SetModel {
            id: 2,
            model: "large-delta".to_string(),
        },
        Request::Message {
            active_skill: None,
            id: 3,
            content: "fill peer buffer".to_string(),
            images: Vec::new(),
            system_reminder: None,
            no_reply: false,
        },
    ] {
        let payload = serde_json::to_string(&request).expect("serialize blocked request") + "\n";
        blocked_writer
            .write_all(payload.as_bytes())
            .await
            .expect("write blocked request");
    }

    tokio::time::timeout(Duration::from_secs(1), async {
        while !emitted.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("local provider must emit the backpressure frame");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (peer_server, peer_client) = crate::transport::Stream::pair().expect("peer client pair");
    let peer_provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let peer_task = spawn_client(peer_server, peer_provider);
    let (peer_reader, mut peer_writer) = peer_client.into_split();
    let mut peer_reader = BufReader::new(peer_reader);
    let payload = serde_json::to_string(&Request::CommList {
        id: 9,
        session_id: "not-in-swarm".to_string(),
    })
    .expect("serialize peer control request")
        + "\n";
    peer_writer
        .write_all(payload.as_bytes())
        .await
        .expect("write peer control request");
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(1), peer_reader.read_line(&mut line))
        .await
        .expect("peer ack deadline")
        .expect("read peer ack");
    assert!(matches!(
        decode_request_or_event(&line),
        ServerEvent::Ack { id: 9 }
    ));
    line.clear();
    tokio::time::timeout(Duration::from_secs(1), peer_reader.read_line(&mut line))
        .await
        .expect("peer terminal deadline")
        .expect("read peer terminal response");
    assert!(matches!(
        decode_request_or_event(&line),
        ServerEvent::Error { id: 9, .. }
    ));
    drop(peer_writer);
    peer_task
        .await
        .expect("peer task join")
        .expect("peer task result");

    tokio::time::timeout(Duration::from_secs(4), async {
        while !client_connections.read().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("timed-out event peer must reach connection cleanup");
    drop(blocked_writer);
    blocked_task
        .await
        .expect("blocked task join")
        .expect("blocked task result");
}

#[tokio::test]
async fn lightweight_comm_spawned_agent_can_reach_structural_review_authority() {
    let _env = IsolatedReloadRecoveryEnv::new();
    let (server_stream, client_stream) = crate::transport::Stream::pair().expect("socket pair");
    let provider_template: Arc<dyn Provider> = Arc::new(CompleteImmediatelyProvider);
    let swarm_id = "lightweight-authority-swarm".to_string();
    let (coordinator_event_tx, _coordinator_event_rx) = mpsc::unbounded_channel();
    let now = Instant::now();
    let sessions: SessionAgents = Arc::new(RwLock::new(HashMap::new()));
    let sessions_for_assertion = Arc::clone(&sessions);
    let global_session_id = Arc::new(RwLock::new(String::new()));
    let client_count = Arc::new(RwLock::new(0usize));
    let client_connections = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(
        "coordinator".to_string(),
        SwarmMember {
            session_id: "coordinator".to_string(),
            event_tx: coordinator_event_tx,
            event_txs: HashMap::new(),
            working_dir: None,
            swarm_id: Some(swarm_id.clone()),
            swarm_enabled: true,
            status: "ready".to_string(),
            detail: None,
            task_label: None,
            friendly_name: Some("coordinator".to_string()),
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
    )])));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::from([(
        swarm_id.clone(),
        HashSet::from(["coordinator".to_string()]),
    )])));
    let shared_context = Arc::new(RwLock::new(HashMap::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::new()));
    let structural_review_runtime = crate::server::StructuralReviewRuntime::new(
        Arc::clone(&swarm_members),
        Arc::clone(&swarm_coordinators),
    );
    let file_touch = FileTouchService::new();
    let channel_subscriptions = Arc::new(RwLock::new(HashMap::new()));
    let channel_subscriptions_by_session = Arc::new(RwLock::new(HashMap::new()));
    let client_debug_state = Arc::new(RwLock::new(ClientDebugState::default()));
    let (_debug_response_tx, _) = broadcast::channel(8);
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let (_global_event_tx, _) = broadcast::channel(8);
    let global_is_processing = Arc::new(RwLock::new(false));
    let shutdown_signals = Arc::new(RwLock::new(HashMap::new()));
    let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::new()));
    let mcp_pool = Arc::new(crate::mcp::SharedMcpPool::from_default_config());

    let server_task = tokio::spawn(handle_client(
        server_stream,
        sessions,
        _global_event_tx,
        provider_template,
        global_is_processing,
        global_session_id,
        client_count,
        Arc::clone(&client_connections),
        swarm_members,
        swarms_by_id,
        shared_context,
        swarm_plans,
        swarm_coordinators,
        file_touch,
        channel_subscriptions,
        channel_subscriptions_by_session,
        client_debug_state,
        _debug_response_tx,
        event_history,
        event_counter,
        swarm_event_tx,
        "jcode-test".to_string(),
        "🧪".to_string(),
        mcp_pool,
        shutdown_signals,
        soft_interrupt_queues,
        AwaitMembersRuntime::default(),
        SwarmMutationRuntime::default(),
        structural_review_runtime,
    ));

    let (client_reader, mut client_writer) = client_stream.into_split();
    let mut client_reader = BufReader::new(client_reader);
    let request = Request::CommSpawn {
        id: 8,
        session_id: "coordinator".to_string(),
        working_dir: None,
        initial_message: None,
        request_nonce: Some("lightweight-authority-contract".to_string()),
        spawn_mode: Some("headless".to_string()),
        model: None,
        effort: None,
        label: None,
        mission_binding: None,
        dispatch_intent: None,
    };
    let payload = serde_json::to_string(&request).expect("serialize request") + "\n";
    client_writer
        .write_all(payload.as_bytes())
        .await
        .expect("write request");

    let child_session_id = tokio::time::timeout(Duration::from_secs(5), async {
        let mut line = String::new();
        loop {
            line.clear();
            client_reader
                .read_line(&mut line)
                .await
                .expect("read spawn event");
            match decode_request_or_event(&line) {
                ServerEvent::CommSpawnResponse { new_session_id, .. } => break new_session_id,
                ServerEvent::Error { message, .. } => panic!("spawn failed: {message}"),
                _ => {}
            }
        }
    })
    .await
    .expect("receive spawned session id");

    drop(client_writer);
    server_task
        .await
        .expect("server task join")
        .expect("server task result");

    let child = sessions_for_assertion
        .read()
        .await
        .get(&child_session_id)
        .cloned()
        .expect("actual lightweight CommSpawn retains its headless Agent");
    let error = child
        .lock()
        .await
        .execute_tool(
            "structural_review",
            serde_json::json!({
                "action": "submit",
                "request_id": "missing-request",
                "disposition": "COHESIVE",
                "refactor_obligations": []
            }),
        )
        .await
        .expect_err("actual spawned Agent reaches the authority-bound tool path");
    assert!(
        !error
            .to_string()
            .contains("structural review authority is unavailable"),
        "spawned Agent must receive the server-issued capability: {error:#}"
    );
    assert!(
        error.to_string().contains("structural review request"),
        "the server-issued child capability must reach receipt validation: {error:#}"
    );
}

fn decode_request_or_event(line: &str) -> ServerEvent {
    serde_json::from_str(line.trim()).expect("decode server event")
}

#[test]
fn oversized_catalog_compaction_preserves_dynamic_antigravity_route_identity() {
    let event = ServerEvent::AvailableModelsUpdated {
        provider_name: Some("Antigravity".to_string()),
        provider_model: Some("chat_20706".to_string()),
        available_models: vec!["chat_20706".to_string()],
        available_model_routes: vec![
            crate::provider::ModelRoute {
                usage: None,
                model: "chat_20706".to_string(),
                provider: "Antigravity".to_string(),
                api_method: "https".to_string(),
                available: true,
                detail: "dynamic catalog detail".to_string(),
                cheapness: None,
            },
            crate::provider::ModelRoute {
                usage: None,
                model: "chat_20706".to_string(),
                provider: "OpenRouter".to_string(),
                api_method: "openrouter".to_string(),
                available: true,
                detail: "other provider detail".to_string(),
                cheapness: None,
            },
        ],
    };

    let compacted =
        compact_available_models_event(&event).expect("route identities should survive compaction");
    let ServerEvent::AvailableModelsUpdated {
        available_model_routes,
        ..
    } = compacted
    else {
        panic!("expected AvailableModelsUpdated");
    };
    assert_eq!(available_model_routes.len(), 2);
    assert_eq!(available_model_routes[0].model, "chat_20706");
    assert_eq!(available_model_routes[0].provider, "Antigravity");
    assert_eq!(available_model_routes[0].api_method, "https");
    assert!(available_model_routes[0].detail.is_empty());
    assert_eq!(available_model_routes[1].provider, "OpenRouter");
    assert_eq!(available_model_routes[1].api_method, "openrouter");
    assert!(available_model_routes[1].detail.is_empty());
}

#[test]
fn soft_interrupt_dispatch_starts_idle_session_and_queues_busy_session() {
    assert!(should_start_idle_soft_interrupt(false, false, false));
    assert!(!should_start_idle_soft_interrupt(true, false, false));
    assert!(!should_start_idle_soft_interrupt(false, true, false));
    assert!(!should_start_idle_soft_interrupt(false, false, true));
}
