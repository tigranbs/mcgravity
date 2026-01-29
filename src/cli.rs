//! CLI argument parsing using clap.

use clap::Parser;
use std::path::PathBuf;

/// `McGravity` - AI CLI Orchestrator
///
/// Orchestrates Codex CLI and Claude CLI to process tasks from a plan file.
/// If no input file is provided, opens an interactive text input screen.
#[derive(Parser, Debug)]
#[command(name = "mcgravity", version, about, long_about = None)]
pub struct Args {
    /// Path to the input text file (optional - if omitted, shows text input screen)
    pub input_file: Option<PathBuf>,
}
