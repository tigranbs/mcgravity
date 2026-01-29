//! Terminal setup and configuration utilities.
//!
//! This module handles low-level terminal event configuration including:
//! - Bracketed paste mode (for reliable multi-line paste)
//! - Keyboard enhancement protocol (for proper Shift+Enter detection)

use std::io::stdout;

use ratatui::crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;

/// Guard to ensure terminal event modes are disabled on drop.
///
/// This handles:
/// - Bracketed paste mode (for reliable multi-line paste)
/// - Keyboard enhancement protocol (for proper Shift+Enter detection)
///
/// This ensures proper cleanup even if the application panics.
pub struct TerminalEventGuard {
    bracketed_paste_enabled: bool,
    keyboard_enhancement_enabled: bool,
}

impl TerminalEventGuard {
    #[must_use]
    pub fn new() -> Self {
        let mut guard = Self {
            bracketed_paste_enabled: false,
            keyboard_enhancement_enabled: false,
        };

        // Enable Bracketed Paste
        match execute!(stdout(), EnableBracketedPaste) {
            Ok(()) => {
                if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
                    eprintln!("[DEBUG INIT] Bracketed paste mode ENABLED");
                }
                guard.bracketed_paste_enabled = true;
            }
            Err(e) => {
                eprintln!("Warning: Could not enable bracketed paste mode: {e}");
                eprintln!("Multi-line paste may not work correctly.");
                if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
                    eprintln!(
                        "[DEBUG INIT] Bracketed paste mode DISABLED (fallback to rapid input detection)"
                    );
                }
            }
        }

        // Enable Keyboard Enhancement (Kitty Protocol)
        // This is required to reliably detect Shift+Enter vs Enter
        match execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        ) {
            Ok(()) => {
                if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
                    eprintln!("[DEBUG INIT] Keyboard enhancement ENABLED");
                }
                guard.keyboard_enhancement_enabled = true;
            }
            Err(e) => {
                // Not fatal, but Shift+Enter might not work as expected
                if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
                    eprintln!("[DEBUG INIT] Keyboard enhancement FAILED: {e}");
                }
            }
        }

        guard
    }
}

impl Default for TerminalEventGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TerminalEventGuard {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        if self.bracketed_paste_enabled {
            let _ = execute!(stdout(), DisableBracketedPaste);
        }
    }
}
