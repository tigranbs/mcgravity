//! Centralized theme and styling.

use ratatui::style::{Color, Modifier, Style};

/// Application theme with consistent colors and styles.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Background color.
    pub bg: Color,
    /// Primary foreground color.
    pub fg: Color,
    /// Accent/highlight color.
    pub accent: Color,
    /// Success color (green).
    pub success: Color,
    /// Warning color (yellow).
    pub warning: Color,
    /// Error color (red).
    pub error: Color,
    /// Muted/secondary text color.
    pub muted: Color,
    /// Progress bar complete portion.
    pub progress_complete: Color,
    /// Progress bar remaining portion.
    pub progress_remaining: Color,
    /// Border color.
    pub border: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::Reset,
            fg: Color::White,
            accent: Color::Cyan,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            muted: Color::DarkGray,
            progress_complete: Color::Cyan,
            progress_remaining: Color::DarkGray,
            border: Color::Gray,
        }
    }
}

impl Theme {
    /// Style for the header/title.
    #[must_use]
    pub fn header_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for normal text.
    #[must_use]
    pub fn normal_style(&self) -> Style {
        Style::default().fg(self.fg)
    }

    /// Style for muted/secondary text.
    #[must_use]
    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    /// Style for success messages.
    #[must_use]
    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }

    /// Style for warning messages.
    #[must_use]
    pub fn warning_style(&self) -> Style {
        Style::default().fg(self.warning)
    }

    /// Style for error messages.
    #[must_use]
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    /// Style for borders.
    #[must_use]
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    /// Style for highlighted/selected items.
    #[must_use]
    pub fn highlight_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for scrollbar thumb.
    #[must_use]
    pub fn scrollbar_thumb_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Style for scrollbar track.
    #[must_use]
    pub fn scrollbar_track_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    /// Style for placeholder text (visible on both light and dark backgrounds).
    ///
    /// Uses `Color::Gray` which is brighter than `DarkGray` and visible on dark terminals,
    /// combined with `DIM` modifier for a subtle appearance.
    #[must_use]
    pub fn placeholder_style(&self) -> Style {
        Style::default().fg(Color::Gray).add_modifier(Modifier::DIM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Default Theme Tests
    // =========================================================================

    mod default_theme {
        use super::*;

        /// Tests that default theme has expected background color.
        #[test]
        fn bg_is_reset() {
            let theme = Theme::default();
            assert_eq!(theme.bg, Color::Reset);
        }

        /// Tests that default theme has white foreground.
        #[test]
        fn fg_is_white() {
            let theme = Theme::default();
            assert_eq!(theme.fg, Color::White);
        }

        /// Tests that default theme has cyan accent.
        #[test]
        fn accent_is_cyan() {
            let theme = Theme::default();
            assert_eq!(theme.accent, Color::Cyan);
        }

        /// Tests that default theme has green success color.
        #[test]
        fn success_is_green() {
            let theme = Theme::default();
            assert_eq!(theme.success, Color::Green);
        }

        /// Tests that default theme has yellow warning color.
        #[test]
        fn warning_is_yellow() {
            let theme = Theme::default();
            assert_eq!(theme.warning, Color::Yellow);
        }

        /// Tests that default theme has red error color.
        #[test]
        fn error_is_red() {
            let theme = Theme::default();
            assert_eq!(theme.error, Color::Red);
        }

        /// Tests that default theme has dark gray muted color.
        #[test]
        fn muted_is_dark_gray() {
            let theme = Theme::default();
            assert_eq!(theme.muted, Color::DarkGray);
        }

        /// Tests that default theme has cyan progress complete color.
        #[test]
        fn progress_complete_is_cyan() {
            let theme = Theme::default();
            assert_eq!(theme.progress_complete, Color::Cyan);
        }

        /// Tests that default theme has dark gray progress remaining.
        #[test]
        fn progress_remaining_is_dark_gray() {
            let theme = Theme::default();
            assert_eq!(theme.progress_remaining, Color::DarkGray);
        }

        /// Tests that default theme has gray border.
        #[test]
        fn border_is_gray() {
            let theme = Theme::default();
            assert_eq!(theme.border, Color::Gray);
        }
    }

    // =========================================================================
    // Style Method Tests
    // =========================================================================

    mod style_methods {
        use super::*;

        /// Tests that header style uses accent color with bold modifier.
        #[test]
        fn header_style_uses_accent_and_bold() {
            let theme = Theme::default();
            let style = theme.header_style();

            assert_eq!(style.fg, Some(theme.accent));
            assert!(style.add_modifier.contains(Modifier::BOLD));
        }

        /// Tests that normal style uses foreground color.
        #[test]
        fn normal_style_uses_fg() {
            let theme = Theme::default();
            let style = theme.normal_style();

            assert_eq!(style.fg, Some(theme.fg));
        }

        /// Tests that muted style uses muted color.
        #[test]
        fn muted_style_uses_muted() {
            let theme = Theme::default();
            let style = theme.muted_style();

            assert_eq!(style.fg, Some(theme.muted));
        }

        /// Tests that success style uses success color.
        #[test]
        fn success_style_uses_success() {
            let theme = Theme::default();
            let style = theme.success_style();

            assert_eq!(style.fg, Some(theme.success));
        }

        /// Tests that warning style uses warning color.
        #[test]
        fn warning_style_uses_warning() {
            let theme = Theme::default();
            let style = theme.warning_style();

            assert_eq!(style.fg, Some(theme.warning));
        }

        /// Tests that error style uses error color.
        #[test]
        fn error_style_uses_error() {
            let theme = Theme::default();
            let style = theme.error_style();

            assert_eq!(style.fg, Some(theme.error));
        }

        /// Tests that border style uses border color.
        #[test]
        fn border_style_uses_border() {
            let theme = Theme::default();
            let style = theme.border_style();

            assert_eq!(style.fg, Some(theme.border));
        }

        /// Tests that highlight style uses accent with bold modifier.
        #[test]
        fn highlight_style_uses_accent_and_bold() {
            let theme = Theme::default();
            let style = theme.highlight_style();

            assert_eq!(style.fg, Some(theme.accent));
            assert!(style.add_modifier.contains(Modifier::BOLD));
        }
    }

    // =========================================================================
    // Custom Theme Tests
    // =========================================================================

    mod custom_theme {
        use super::*;

        /// Creates a custom theme for testing.
        fn custom_theme() -> Theme {
            Theme {
                bg: Color::Black,
                fg: Color::LightYellow,
                accent: Color::Magenta,
                success: Color::LightGreen,
                warning: Color::LightYellow,
                error: Color::LightRed,
                muted: Color::Gray,
                progress_complete: Color::Blue,
                progress_remaining: Color::Black,
                border: Color::White,
            }
        }

        /// Tests that custom theme preserves custom values.
        #[test]
        fn custom_values_are_preserved() {
            let theme = custom_theme();

            assert_eq!(theme.bg, Color::Black);
            assert_eq!(theme.fg, Color::LightYellow);
            assert_eq!(theme.accent, Color::Magenta);
            assert_eq!(theme.success, Color::LightGreen);
        }

        /// Tests that style methods use custom theme colors.
        #[test]
        fn styles_use_custom_colors() {
            let theme = custom_theme();

            assert_eq!(theme.header_style().fg, Some(Color::Magenta));
            assert_eq!(theme.success_style().fg, Some(Color::LightGreen));
            assert_eq!(theme.error_style().fg, Some(Color::LightRed));
        }
    }

    // =========================================================================
    // Trait Implementation Tests
    // =========================================================================

    mod trait_impls {
        use super::*;

        /// Tests that Theme can be cloned.
        #[test]
        fn clone_creates_equal_theme() {
            let original = Theme::default();
            let cloned = original.clone();

            assert_eq!(original.bg, cloned.bg);
            assert_eq!(original.fg, cloned.fg);
            assert_eq!(original.accent, cloned.accent);
            assert_eq!(original.success, cloned.success);
            assert_eq!(original.warning, cloned.warning);
            assert_eq!(original.error, cloned.error);
            assert_eq!(original.muted, cloned.muted);
            assert_eq!(original.border, cloned.border);
        }

        /// Tests that modifying clone doesn't affect original.
        #[test]
        fn clone_is_independent() {
            let original = Theme::default();
            let mut cloned = original.clone();
            cloned.accent = Color::Magenta;

            assert_eq!(original.accent, Color::Cyan);
            assert_eq!(cloned.accent, Color::Magenta);
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_is_readable() {
            let theme = Theme::default();
            let debug_str = format!("{theme:?}");

            assert!(debug_str.contains("Theme"));
            assert!(debug_str.contains("bg"));
            assert!(debug_str.contains("fg"));
            assert!(debug_str.contains("accent"));
        }
    }
}
