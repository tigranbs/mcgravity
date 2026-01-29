//! Flow cancellation and state tests.
//!
//! Tests for the `McGravity` flow lifecycle including:
//! - Escape key cancellation behavior
//! - Flow state transitions
//! - Shutdown signal handling

use super::helpers::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn esc_cancels_running_flow_without_quitting() {
    let mut app = create_test_app_with_lines(&["task text"], 0, 9);
    app.set_running(true);
    let shutdown_rx = app.shutdown_tx.subscribe();

    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    app.handle_key(key);

    assert!(!app.should_quit());
    assert!(*shutdown_rx.borrow());
}

#[test]
fn esc_does_not_quit_when_idle() {
    let mut app = create_test_app_with_lines(&["task text"], 0, 9);
    let shutdown_rx = app.shutdown_tx.subscribe();

    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    app.handle_key(key);

    assert!(!app.should_quit());
    assert!(!*shutdown_rx.borrow());
}

#[test]
fn ctrl_c_quits_from_chat_mode() {
    let mut app = create_test_app_with_lines(&["task text"], 0, 9);
    let shutdown_rx = app.shutdown_tx.subscribe();

    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    app.handle_key(key);

    assert!(app.should_quit());
    assert!(*shutdown_rx.borrow());
}

/// Regression test: shutdown signal must be cleared after ESC cancellation
/// even when no receivers exist.
///
/// After ESC cancellation, the flow task drops its shutdown receiver. The old
/// `send(false)` approach fails silently when there are no receivers, leaving
/// the shutdown flag stuck at `true`. A subsequent run subscribes and immediately
/// exits because it observes the stale `true` value.
///
/// This test ensures `reset_shutdown()` clears the flag receiver-independently.
#[test]
fn shutdown_resets_after_cancel_without_receivers() {
    let app = create_test_app_with_lines(&["task text"], 0, 9);

    // Simulate a running flow: create a receiver and trigger shutdown
    let shutdown_rx = app.shutdown_receiver();
    app.trigger_shutdown();
    assert!(
        *shutdown_rx.borrow(),
        "shutdown flag should be true after trigger"
    );

    // Flow task completes and drops its receiver
    drop(shutdown_rx);

    // Reset the shutdown flag (must work without any receivers)
    app.reset_shutdown();

    // A new flow subscribes - it must observe `false`
    let new_rx = app.shutdown_receiver();
    assert!(
        !*new_rx.borrow(),
        "shutdown flag should be false after reset, even when reset had no receivers"
    );
}
