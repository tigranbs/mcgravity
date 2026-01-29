//! Custom TUI widgets.

pub mod command_popup;
pub mod file_popup;
pub mod output;
pub mod status_indicator;

pub use command_popup::{CommandMatch, CommandPopup, CommandPopupState, MAX_COMMAND_POPUP_ROWS};
pub use file_popup::{FileSuggestionPopup, MAX_POPUP_ROWS, PopupState};
pub use output::{
    MAX_OUTPUT_LINES, OutputLine, OutputLineType, OutputWidget, calculate_visual_line_count,
};
pub use status_indicator::StatusIndicatorWidget;
