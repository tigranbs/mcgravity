//! Tests for the app module.
//!
//! This module is organized into submodules by functionality:
//! - `file_search` - File search popup and selection tests
//! - `flow` - Flow cancellation and state tests
//! - `helpers` - Shared test utilities
//! - `input` - Text input, key bindings, and paste handling
//! - `integration` - End-to-end workflow tests
//! - `persistence` - Task file persistence tests
//! - `settings` - Settings panel tests
//! - `ui` - Output panel and visual line tests
//!
//! ## Test Statistics
//! - Total tests: 686
//! - Last verified: 2026-01-23

#[allow(clippy::unwrap_used, clippy::expect_used)]
mod file_search;
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod flow;
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod helpers;
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod initial_setup;
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod input;
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod integration;
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod persistence;
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod settings;
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod ui;
