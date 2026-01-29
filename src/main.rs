//! `McGravity` - TUI-based AI CLI Orchestrator
//!
//! Entry point for the application.

use std::time::Duration;

use clap::Parser;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use mcgravity::app::App;
use mcgravity::cli::Args;
use mcgravity::tui::TerminalEventGuard;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    // Initialize the terminal with crossterm backend
    let mut terminal = ratatui::init();

    // Run the application
    let result = run_app(&mut terminal, args);

    // Restore the terminal
    ratatui::restore();

    result
}

fn run_app(terminal: &mut ratatui::DefaultTerminal, args: Args) -> std::io::Result<()> {
    // Enable terminal event modes (bracketed paste, keyboard enhancement).
    // The guard ensures cleanup even if the application panics.
    //
    // IMPORTANT: This must be initialized inside run_app (after ratatui::run
    // sets up the terminal) because ratatui's terminal initialization can
    // reset terminal flags.
    let _event_guard = TerminalEventGuard::new();

    // Create application (starts in text input mode if no file, else flow running)
    let mut app = App::new(args.input_file).map_err(std::io::Error::other)?;

    // Main event loop
    // Flow will be spawned after user submits task
    loop {
        // Render the UI
        // IMPORTANT: Layout calculation must happen inside the draw closure
        // to ensure it uses the exact same area as rendering
        terminal.draw(|frame| {
            app.update_layout(frame.area());
            app.render(frame);
        })?;

        // Poll for events with a short timeout
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                // Handle key presses
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key);
                }
                // Handle bracketed paste events (multi-line paste)
                Event::Paste(text) => {
                    // Enhanced debug logging for paste events
                    if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
                        let lines = text.lines().count();
                        let has_trailing_newline = text.ends_with('\n');
                        // Show first 20 chars with escaped control characters for debugging
                        let first_chars: String = text
                            .chars()
                            .take(20)
                            .map(|c| {
                                if c == '\n' {
                                    "\\n".to_string()
                                } else if c == '\r' {
                                    "\\r".to_string()
                                } else if c == '\t' {
                                    "\\t".to_string()
                                } else {
                                    c.to_string()
                                }
                            })
                            .collect();
                        eprintln!(
                            "[DEBUG PASTE] len={} lines={} trailing_newline={} first_chars={:?}",
                            text.len(),
                            lines,
                            has_trailing_newline,
                            first_chars
                        );
                    }
                    app.handle_paste(&text);
                }
                _ => {}
            }
        }

        // Process any pending flow events
        app.process_events();

        // Process periodic tasks (autosave, etc.)
        app.tick();

        // Check if we should quit
        if app.should_quit() {
            break;
        }
    }

    // Save any pending changes before exiting
    if let Err(e) = app.save_current_task() {
        eprintln!("Warning: Failed to save task on exit: {e}");
    }

    Ok(())
}
