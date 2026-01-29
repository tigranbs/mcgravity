//! Shared test utilities for the app module.
//!
//! This module provides helper functions and utilities for testing:
//! - `create_test_app_with_lines` - Creates minimal `App` instances for testing
//! - `create_test_files` - Creates temporary test files
//! - `render_app_to_terminal` - Renders the app to a `TestBackend` for `assert_buffer_lines` assertions
//! - `CwdGuard` - RAII guard for safely changing working directory in tests
//! - Key event helpers (`char_key`, `enter_key`)

use crate::app::input::RapidInputDetector;
use crate::app::{App, AppMode, FlowUiState, LayoutState, SettingsState, TextInputState};
use crate::fs::McgravityPaths;
use crate::tui::widgets::PopupState;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::fs::{self, File};
use std::sync::Mutex;
use tui_textarea::{CursorMove, TextArea};

/// Creates a [`KeyEvent`] for a character key with no modifiers.
pub fn char_key(c: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

/// Creates a [`KeyEvent`] for the Enter key with specified modifiers.
pub fn enter_key(modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Enter,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

/// Mutex to serialize tests that modify the current working directory.
/// This is necessary because `std::env::set_current_dir` is process-global.
/// All tests that change CWD MUST use [`CwdGuard`] to ensure proper serialization.
pub static CWD_MUTEX: Mutex<()> = Mutex::new(());

/// Guard struct that restores the original directory when dropped.
/// This ensures directory restoration even if a test panics.
/// Also holds the mutex guard to ensure serialization.
pub struct CwdGuard {
    original_dir: std::path::PathBuf,
    /// Held to keep the mutex locked until this guard is dropped.
    /// Field is never read directly, but holding it prevents concurrent CWD changes.
    #[allow(dead_code)] // Intentional: RAII pattern - field kept for drop semantics
    mutex_guard: std::sync::MutexGuard<'static, ()>,
}

impl CwdGuard {
    /// Creates a new `CwdGuard`, acquiring the CWD mutex.
    /// The mutex is held until this guard is dropped.
    ///
    /// # Errors
    ///
    /// Returns an error if the current directory cannot be determined.
    pub fn new() -> Result<Self> {
        // Poisoned mutex is intentionally recovered: test isolation requires holding
        // the lock even if a prior test panicked. This prevents concurrent CWD changes.
        let mutex_guard = CWD_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(Self {
            original_dir: std::env::current_dir()?,
            mutex_guard,
        })
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_dir);
    }
}

/// Helper to create a minimal `App` for testing.
///
/// Note: `cursor_col` is the character index (not byte index) for compatibility
/// with `tui-textarea`'s cursor positioning.
///
/// Uses `/tmp` as a safe base path. For tests that need to read/write files
/// in a specific directory, use `create_test_app_with_paths` instead.
pub fn create_test_app_with_lines(lines: &[&str], cursor_row: usize, cursor_col: usize) -> App {
    // Use /tmp as safe base path that always exists, rather than from_cwd() which can fail
    // if another test has changed and deleted the CWD (race condition in parallel tests)
    create_test_app_with_paths(
        lines,
        cursor_row,
        cursor_col,
        McgravityPaths::new(std::env::temp_dir().as_path()),
    )
}

/// Helper to create a minimal `App` for testing with custom paths.
///
/// This variant allows specifying custom paths for file operations, enabling
/// tests to use isolated temporary directories.
pub fn create_test_app_with_paths(
    lines: &[&str],
    cursor_row: usize,
    cursor_col: usize,
    paths: McgravityPaths,
) -> App {
    let (search_tx, _search_rx) = tokio::sync::mpsc::channel(16);

    // Create a TextArea with the given content
    let lines_owned: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();
    let mut textarea = TextArea::new(lines_owned);

    // Position the cursor
    textarea.move_cursor(CursorMove::Top);
    for _ in 0..cursor_row {
        textarea.move_cursor(CursorMove::Down);
    }
    textarea.move_cursor(CursorMove::Head);
    for _ in 0..cursor_col {
        textarea.move_cursor(CursorMove::Forward);
    }

    let mut app = App {
        paths,
        flow: crate::core::FlowState::new_without_file(),
        theme: crate::tui::Theme::default(),
        mode: AppMode::Chat,
        should_quit: false,
        is_running: false,
        event_rx: tokio::sync::mpsc::channel(1).1,
        event_tx: tokio::sync::mpsc::channel(1).0,
        shutdown_tx: tokio::sync::watch::channel(false).0,
        text_input: TextInputState {
            textarea,
            at_token: None,
            file_popup_state: PopupState::default(),
            last_search_query: None,
            last_search_time: None,
            search_tx,
            search_generation: 0,
            rapid_input: RapidInputDetector::new(),
            // Autosave state
            last_edit_time: None,
            is_dirty: false,
            // Slash command state
            command_popup_state: crate::tui::widgets::CommandPopupState::default(),
            slash_token: None,
        },
        settings: SettingsState::default(),
        flow_ui: FlowUiState::default(),
        layout: LayoutState::default(),
        initial_setup: None,
        command_registry: crate::core::CommandRegistry::with_builtins(),
    };

    app.settings.model_availability = crate::core::ModelAvailability {
        codex: true,
        claude: true,
        gemini: true,
    };

    app
}

/// Creates test files in the given directory.
///
/// # Errors
///
/// Returns an error if directory creation or file creation fails.
pub fn create_test_files(dir: &std::path::Path, files: &[&str]) -> Result<()> {
    for file in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        File::create(&path)?;
    }
    Ok(())
}

/// Renders the app to a `TestBackend` terminal for `assert_buffer_lines` assertions.
///
/// This function mimics the main loop behavior by calling `update_layout()`
/// before rendering, ensuring the cached layout is properly initialized.
///
/// # Errors
///
/// Returns an error if terminal creation or rendering fails.
pub fn render_app_to_terminal(
    app: &mut App,
    width: u16,
    height: u16,
) -> Result<Terminal<TestBackend>> {
    use ratatui::layout::Rect;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;

    // Update layout before rendering (mimics main loop behavior)
    app.update_layout(Rect::new(0, 0, width, height));

    terminal.draw(|f| app.render(f))?;

    Ok(terminal)
}

/// Builds styled `Line` values using the actual buffer styles for assertions.
///
/// This supports inline `assert_buffer_lines` expectations by validating
/// symbols/layout while reusing the rendered styles so tests don't hardcode colors.
#[must_use]
pub fn styled_lines_from_buffer(
    terminal: &Terminal<TestBackend>,
    expected_lines: &[&str],
) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::buffer::Buffer;
    use ratatui::text::{Line, Span};

    let actual = terminal.backend().buffer();
    let area = *actual.area();
    let mut expected = Buffer::empty(area);

    for (y, line) in expected_lines.iter().enumerate() {
        let Ok(y) = u16::try_from(y) else {
            break;
        };
        if y >= area.height {
            break;
        }
        let line = Line::from(*line);
        expected.set_line(0, y, &line, area.width);
    }

    let mut lines = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut spans = Vec::new();
        let mut current_style = None;
        let mut current_text = String::new();

        for x in 0..area.width {
            let symbol = expected[(x, y)].symbol();
            let style = actual[(x, y)].style();

            match current_style {
                Some(active_style) if active_style == style => {
                    current_text.push_str(symbol);
                }
                Some(active_style) => {
                    spans.push(Span::styled(
                        std::mem::take(&mut current_text),
                        active_style,
                    ));
                    current_text.push_str(symbol);
                    current_style = Some(style);
                }
                None => {
                    current_style = Some(style);
                    current_text.push_str(symbol);
                }
            }
        }

        if let Some(active_style) = current_style {
            spans.push(Span::styled(current_text, active_style));
        }

        lines.push(Line::from(spans));
    }

    lines
}
