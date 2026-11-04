use super::*;
use crate::client::shell::zellij_keymap::Mode;
use crate::input::TerminalKey;

fn shell() -> ClientShellState {
    let mut config = Config::default();
    config.ui.zellij_keymap = true;
    config.ui.prompt_new_tab_name = false;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

#[test]
fn zellij_tab_creation_returns_to_normal_and_does_not_forward_keys() {
    let mut state = shell();
    assert!(state.handle_input_bytes(&[0x14]).requests.is_empty());
    assert_eq!(state.mode, ClientShellMode::Zellij(Mode::Tab));
    assert!(state.host_keyboard_report_all_requested());
    let result = state.handle_input_bytes(b"n");
    assert!(result.requests.is_empty());
    assert!(
        matches!(&result.actions[..], [ClientShellAction::Endpoint { request, .. }]
        if matches!(request.method, crate::api::schema::Method::TabCreate(_)))
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(!state.handle_input_bytes(b"hello").requests.is_empty());
}

#[test]
fn zellij_modes_switch_toggle_and_consume_unknown_keys() {
    let mut state = shell();
    for (key, mode) in [
        (0x14, Mode::Tab),
        (0x10, Mode::Pane),
        (0x12, Mode::Resize),
        (0x08, Mode::Move),
        (0x0f, Mode::Session),
    ] {
        state.handle_input_bytes(&[key]);
        assert_eq!(state.mode, ClientShellMode::Zellij(mode));
        assert!(state.handle_input_bytes(b"z").requests.is_empty());
    }
    state.handle_input_bytes(&[0x0f]);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    for exit in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char(' ')] {
        state.handle_input_bytes(&[0x10]);
        state.handle_raw_events(vec![RawInputEvent::Key(TerminalKey::new(
            exit,
            KeyModifiers::NONE,
        ))]);
        assert_eq!(state.mode, ClientShellMode::Terminal);
    }
}

#[test]
fn zellij_alt_directions_use_existing_pane_focus_api() {
    use crate::api::schema::{Method, PaneDirection};
    for (key, expected) in [
        ('h', PaneDirection::Left),
        ('j', PaneDirection::Down),
        ('k', PaneDirection::Up),
        ('l', PaneDirection::Right),
    ] {
        let mut state = shell();
        let result = state.handle_raw_events(vec![RawInputEvent::Key(TerminalKey::new(
            KeyCode::Char(key),
            KeyModifiers::ALT,
        ))]);
        assert!(result.requests.is_empty());
        assert!(
            matches!(&result.actions[..], [ClientShellAction::Endpoint { request, .. }]
            if matches!(&request.method, Method::PaneFocusDirection(params) if params.direction == expected))
        );
        assert_eq!(state.mode, ClientShellMode::Terminal);
    }
}

#[test]
fn zellij_directional_commands_persist_and_resize_uses_endpoint_api() {
    let mut state = shell();
    state.handle_input_bytes(&[0x12]);
    let result = state.handle_input_bytes(b"h");
    assert!(
        matches!(&result.actions[..], [ClientShellAction::Endpoint { request, .. }]
        if matches!(&request.method, crate::api::schema::Method::PaneResize(params)
            if params.direction == crate::api::schema::PaneDirection::Left))
    );
    assert_eq!(state.mode, ClientShellMode::Zellij(Mode::Resize));
    state.handle_input_bytes(&[0x10]);
    state.handle_input_bytes(b"h");
    assert_eq!(state.mode, ClientShellMode::Zellij(Mode::Pane));
}

#[test]
fn zellij_locked_mode_forwards_shortcuts_text_and_paste() {
    let mut state = shell();
    state.handle_input_bytes(&[0x07]);
    assert_eq!(state.mode, ClientShellMode::Zellij(Mode::Locked));
    assert!(!state.host_keyboard_report_all_requested());
    assert_eq!(
        state.clipboard_image_target(),
        Some(crate::protocol::ClientClipboardImageTarget::Pane(
            "pane_1".into()
        ))
    );
    for bytes in [&[0x14][..], b"\x1bh", b"\x1b"] {
        let result = state.handle_input_bytes(bytes);
        assert!(result.actions.is_empty());
        assert!(!result.requests.is_empty());
    }
    let paste = state.handle_raw_events(vec![RawInputEvent::Paste("hello".into())]);
    assert!(
        matches!(&paste.requests[..], [ClientMessage::ClientShellPaneInput { events, .. }]
        if matches!(&events[..], [ClientPaneInputEvent::Paste(text)] if text == "hello"))
    );
    state.handle_input_bytes(&[0x07]);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn zellij_rename_overlay_owns_keys_and_consumed_releases_stay_local() {
    let mut state = shell();
    state.handle_input_bytes(b"\x1b[116;5u");
    let release = state.handle_input_bytes(b"\x1b[116;5:3u");
    assert!(release.requests.is_empty());
    let repeat = state.handle_input_bytes(b"\x1b[116;5:2u");
    assert!(repeat.requests.is_empty());
    assert_eq!(state.mode, ClientShellMode::Zellij(Mode::Tab));
    state.handle_input_bytes(b"r");
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Rename(_))));
    let alt = state.handle_input_bytes(b"\x1bh");
    assert!(alt.actions.is_empty());
    assert!(alt.requests.is_empty());
}

#[test]
fn zellij_disabling_keymap_exits_active_mode_before_routing() {
    let mut state = shell();
    state.handle_input_bytes(&[0x10]);
    state.config.zellij_keymap = false;
    let result = state.handle_input_bytes(b"n");
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(!result.requests.is_empty());
    assert!(result.actions.is_empty());
}

#[test]
fn zellij_help_lists_active_shortcuts() {
    let mut state = shell();
    state.handle_input_bytes(b"\x1bOP");
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Help(_))));
    let frame = state.compose(120, 40).expect("help frame");
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Zellij-style keymap"));
    assert!(text.contains("Ctrl+t"));
    assert!(!text.contains("ctrl+b"));
}

#[test]
fn zellij_popup_gets_mode_shortcuts_and_alt_navigation() {
    let mut state = shell();
    state.set_pane_surface(surface_with_popup());
    for bytes in [&[0x14][..], b"\x1bh"] {
        let result = state.handle_input_bytes(bytes);
        assert!(result.actions.is_empty());
        assert!(
            matches!(&result.requests[..], [ClientMessage::ClientShellPopupInput { terminal_id, .. }] if terminal_id == "terminal-popup")
        );
        assert_eq!(state.mode, ClientShellMode::Terminal);
    }
}

#[test]
fn zellij_copy_mode_switch_releases_copy_state() {
    let mut state = shell();
    let mut pane_surface = surface();
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 20,
        viewport_rows: 2,
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    state.handle_input_bytes(&[0x13]);
    assert_eq!(state.mode, ClientShellMode::Copy);
    state.handle_input_bytes(&[0x14]);
    assert_eq!(state.mode, ClientShellMode::Zellij(Mode::Tab));
    assert!(state.copy_mode.is_none());
    state.handle_input_bytes(b"\x1b");
    state.handle_input_bytes(&[0x13]);
    assert_eq!(state.mode, ClientShellMode::Copy);
    state.handle_input_bytes(&[0x13]);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.copy_mode.is_none());
}

#[test]
fn zellij_shifted_help_uses_generated_character() {
    let mut state = shell();
    state.handle_input_bytes(&[0x10]);
    state.handle_raw_events(vec![RawInputEvent::Key(
        TerminalKey::new(KeyCode::Char('/'), KeyModifiers::SHIFT)
            .with_generated_text(Some("?".into())),
    )]);
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Help(_))));
}

#[test]
fn zellij_shifted_alt_plus_resizes_without_forwarding() {
    let mut state = shell();
    let result = state.handle_raw_events(vec![RawInputEvent::Key(TerminalKey::new(
        KeyCode::Char('='),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    ))]);
    assert!(result.requests.is_empty());
    assert!(
        matches!(&result.actions[..], [ClientShellAction::Endpoint { request, .. }]
        if matches!(&request.method, crate::api::schema::Method::PaneResize(params)
            if params.direction == crate::api::schema::PaneDirection::Right))
    );
}
