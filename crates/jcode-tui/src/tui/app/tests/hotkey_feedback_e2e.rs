// End-to-end tests for inline hotkey feedback: rare-hotkey notes and
// unknown-chord near-miss suggestions, driven through App::handle_key.

#[test]
fn unknown_ctrl_chord_sets_hotkey_feedback_with_suggestion() {
    let mut app = create_test_app();
    assert!(app.hotkey_feedback.is_none());

    // Ctrl+M is unbound (no control-key handler claims 'm'); the nearest
    // known hotkey is Alt+M (side panel toggle).
    app.handle_key(KeyCode::Char('m'), KeyModifiers::CONTROL)
        .unwrap();

    let (message, _) = app
        .hotkey_feedback
        .clone()
        .expect("unknown chord should set feedback");
    assert!(message.contains("Ctrl+M"), "{message}");
    assert!(message.contains("isn't bound"), "{message}");
    assert!(
        message.contains("Alt+M") || message.contains("⌥+M"),
        "{message}"
    );
    assert!(message.contains("side panel"), "{message}");

    // The renderer consumes the trait accessor; it must surface the same text
    // (and expire it later) so the notification line actually shows it.
    {
        use crate::tui::TuiState as _;
        let visible = app
            .hotkey_feedback()
            .expect("trait accessor should expose fresh feedback");
        assert_eq!(visible, message);
    }
}

#[test]
fn rare_known_hotkey_sets_feedback_and_repeats_stop_once_familiar() {
    let mut app = create_test_app();

    // Ctrl+T toggles queue mode; a fresh JCODE_HOME has no usage history, so
    // the first press is "rare" and should explain itself.
    app.handle_key(KeyCode::Char('t'), KeyModifiers::CONTROL)
        .unwrap();
    let (message, _) = app
        .hotkey_feedback
        .clone()
        .expect("first use of a known hotkey should set feedback");
    assert!(message.contains("Ctrl+T"), "{message}");
    assert!(message.contains("queue mode"), "{message}");

    // After enough uses the action becomes familiar and the note stops.
    for _ in 0..8 {
        app.handle_key(KeyCode::Char('t'), KeyModifiers::CONTROL)
            .unwrap();
    }
    app.hotkey_feedback = None;
    app.handle_key(KeyCode::Char('t'), KeyModifiers::CONTROL)
        .unwrap();
    assert!(
        app.hotkey_feedback.is_none(),
        "familiar hotkeys should not re-announce"
    );
}

#[test]
fn plain_typing_never_sets_hotkey_feedback() {
    let mut app = create_test_app();
    app.handle_key(KeyCode::Char('h'), KeyModifiers::empty())
        .unwrap();
    app.handle_key(KeyCode::Char('I'), KeyModifiers::SHIFT)
        .unwrap();
    assert!(app.hotkey_feedback.is_none());
    assert_eq!(app.input, "hI");
}

#[test]
fn caps_lock_does_not_break_control_shortcuts() {
    let mut app = create_test_app();
    assert!(!app.queue_mode);

    app.handle_key(KeyCode::Char('T'), KeyModifiers::CONTROL)
        .unwrap();

    assert!(
        app.queue_mode,
        "Ctrl+T must work when Caps Lock yields uppercase T"
    );
}

#[test]
fn command_modified_ukrainian_character_is_inserted() {
    let mut app = create_test_app();

    app.handle_key(KeyCode::Char('ї'), KeyModifiers::SUPER)
        .unwrap();
    app.handle_key(KeyCode::Char('Є'), KeyModifiers::SUPER)
        .unwrap();

    assert_eq!(app.input, "їЄ");
}

#[test]
fn unknown_chord_notice_is_rate_limited_per_chord() {
    let mut app = create_test_app();

    // Ctrl+; is unbound with no near suggestion.
    for _ in 0..6 {
        app.handle_key(KeyCode::Char(';'), KeyModifiers::CONTROL)
            .unwrap();
        // Reset the time-based limiter so only the per-chord cap applies.
        app.last_unknown_hotkey_notice = None;
    }
    assert!(app.unknown_hotkey_seen.get("Ctrl+;").copied().unwrap_or(0) <= 3);
}

#[test]
fn ctrl_alt_h_opens_shortcut_help_in_side_panel_with_stats() {
    let mut app = create_test_app();
    app.remote_total_tokens = Some((12_345, 678));
    app.push_display_message(DisplayMessage::user("hello"));
    app.push_display_message(DisplayMessage::assistant("hello from jcode"));
    let tool_input = serde_json::json!({"file_path":"demo.rs","content":"fn main() {}"});
    let expected_tool_chars = "write".chars().count()
        + serde_json::to_string(&tool_input)
            .expect("serialize tool input")
            .chars()
            .count();
    let expected_jcode_chars = "hello from jcode".chars().count() + expected_tool_chars;
    app.push_display_message(DisplayMessage::tool(
        "write",
        crate::message::ToolCall {
            id: "tool-1".to_string(),
            name: "write".to_string(),
            input: tool_input,
            intent: None,
            thought_signature: None,
        },
    ));

    app.handle_key(
        KeyCode::Char('H'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    )
    .unwrap();

    assert_eq!(
        app.side_panel.focused_page_id.as_deref(),
        Some("shortcut-help")
    );
    let page = app.side_panel.focused_page().expect("shortcut help page");
    assert_eq!(page.title, "Shortcuts & stats");
    assert!(page.content.contains("Ctrl+Alt+H"), "{}", page.content);
    assert!(page.content.contains("12,345 input"), "{}", page.content);
    assert!(
        page.content.contains("Your prompts:** 5 characters"),
        "{}",
        page.content
    );
    assert!(
        page.content.contains(&format!(
            "Jcode authored:** {expected_jcode_chars} characters"
        )),
        "{}",
        page.content
    );
    assert!(
        page.content.contains(&format!(
            "Tool arguments included:** {expected_tool_chars} characters"
        )),
        "{}",
        page.content
    );
    assert!(
        !page.content.contains("fn main()"),
        "tool payload must be counted, not printed: {}",
        page.content
    );
}

#[test]
fn remote_caps_lock_and_ukrainian_command_input_follow_the_live_key_path() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        app.is_remote = true;

        rt.block_on(app.handle_remote_key(KeyCode::Char('T'), KeyModifiers::CONTROL, &mut remote))
            .expect("uppercase Ctrl+T should work in the remote path");
        assert!(app.queue_mode);

        rt.block_on(app.handle_remote_key(KeyCode::Char('ї'), KeyModifiers::SUPER, &mut remote))
            .expect("Command-modified Ukrainian text should work in the remote path");
        assert_eq!(app.input, "ї");
    });
}

#[test]
fn remote_ctrl_alt_h_opens_shortcut_help() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        app.is_remote = true;

        rt.block_on(app.handle_remote_key(
            KeyCode::Char('H'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            &mut remote,
        ))
        .expect("Ctrl+Alt+H should work in the remote path");

        assert_eq!(
            app.side_panel.focused_page_id.as_deref(),
            Some("shortcut-help")
        );
    });
}
