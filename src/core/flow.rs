//! Orchestration flow state machine.

use std::borrow::Cow;
use std::path::PathBuf;

/// Phases of the orchestration flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowPhase {
    /// Initial state before starting.
    Idle,
    /// Reading the input file.
    ReadingInput,
    /// Scanning for completed task context from task.md.
    CheckingDoneFiles,
    /// Running the planning phase with the selected model.
    RunningPlanning {
        model_name: Cow<'static, str>,
        attempt: u32,
    },
    /// Checking for todo/*.md files.
    CheckingTodoFiles,
    /// No todo files found - flow complete.
    NoTodoFiles,
    /// Processing todo files.
    ProcessingTodos { current: usize, total: usize },
    /// Running the execution phase on a specific file with the selected model.
    RunningExecution {
        model_name: Cow<'static, str>,
        file_index: usize,
        attempt: u32,
    },
    /// One cycle complete, preparing for next.
    CycleComplete { iteration: u32 },
    /// Updating summary and removing completed todo files.
    MovingCompletedFiles,
    /// All cycles complete successfully.
    Completed,
    /// Flow failed with an error.
    Failed { reason: String },
}

impl FlowPhase {
    /// Returns a human-readable description of the current phase.
    #[must_use]
    pub fn description(&self) -> Cow<'static, str> {
        match self {
            Self::Idle => Cow::Borrowed("Idle"),
            Self::ReadingInput => Cow::Borrowed("Reading input file"),
            Self::CheckingDoneFiles => Cow::Borrowed("Loading completed task context"),
            Self::RunningPlanning {
                model_name,
                attempt,
            } => Cow::Owned(format!("Running {model_name} (attempt {attempt})")),
            Self::CheckingTodoFiles => Cow::Borrowed("Checking for todo files"),
            Self::NoTodoFiles => Cow::Borrowed("No todo files found"),
            Self::ProcessingTodos { current, total } => {
                Cow::Owned(format!("Processing todos ({current}/{total})"))
            }
            Self::RunningExecution {
                model_name,
                file_index,
                attempt,
            } => Cow::Owned(format!(
                "Running {model_name} on file {file_index} (attempt {attempt})"
            )),
            Self::CycleComplete { iteration } => Cow::Owned(format!("Cycle {iteration} complete")),
            Self::MovingCompletedFiles => {
                Cow::Borrowed("Updating summary, removing completed todos")
            }
            Self::Completed => Cow::Borrowed("Completed"),
            Self::Failed { reason } => Cow::Owned(format!("Failed: {reason}")),
        }
    }

    /// Returns true if the flow is in a terminal state.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed { .. } | Self::NoTodoFiles
        )
    }
}

/// State of the orchestration flow.
#[derive(Debug, Clone)]
pub struct FlowState {
    /// Current phase of execution.
    pub phase: FlowPhase,
    /// Content of the input (from file or direct text entry).
    pub input_text: String,
    /// Path to the input file (None if text was entered directly).
    pub input_path: Option<PathBuf>,
    /// List of todo files to process.
    pub todo_files: Vec<PathBuf>,
    /// Current cycle count (how many times we've run the planning phase).
    pub cycle_count: u32,
}

impl FlowState {
    /// Creates a new flow state with the given input path.
    #[must_use]
    pub fn new(input_path: PathBuf) -> Self {
        Self {
            phase: FlowPhase::Idle,
            input_text: String::new(),
            input_path: Some(input_path),
            todo_files: Vec::new(),
            cycle_count: 0,
        }
    }

    /// Creates a new flow state without a file (for direct text input).
    #[must_use]
    pub fn new_without_file() -> Self {
        Self {
            phase: FlowPhase::Idle,
            input_text: String::new(),
            input_path: None,
            todo_files: Vec::new(),
            cycle_count: 0,
        }
    }

    /// Sets the input text directly (for text input mode).
    pub fn set_input_text(&mut self, text: String) {
        self.input_text = text;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // FlowPhase Tests
    // =========================================================================

    mod flow_phase {
        use super::*;

        /// Tests that static phases return borrowed descriptions (zero allocation).
        #[test]
        fn description_static_phases_return_borrowed() {
            let static_phases = [
                FlowPhase::Idle,
                FlowPhase::ReadingInput,
                FlowPhase::CheckingDoneFiles,
                FlowPhase::CheckingTodoFiles,
                FlowPhase::NoTodoFiles,
                FlowPhase::MovingCompletedFiles,
                FlowPhase::Completed,
            ];

            for phase in static_phases {
                let desc = phase.description();
                assert!(
                    matches!(desc, Cow::Borrowed(_)),
                    "Expected Cow::Borrowed for {phase:?}, got Cow::Owned"
                );
            }
        }

        /// Tests that dynamic phases return owned descriptions with correct format.
        #[test]
        fn description_dynamic_phases_return_owned() {
            let planning = FlowPhase::RunningPlanning {
                model_name: Cow::Borrowed("TestModel"),
                attempt: 3,
            };
            assert_eq!(
                planning.description().as_ref(),
                "Running TestModel (attempt 3)"
            );

            let execution = FlowPhase::RunningExecution {
                model_name: Cow::Borrowed("Claude"),
                file_index: 5,
                attempt: 2,
            };
            assert_eq!(
                execution.description().as_ref(),
                "Running Claude on file 5 (attempt 2)"
            );

            let processing = FlowPhase::ProcessingTodos {
                current: 3,
                total: 10,
            };
            assert_eq!(processing.description().as_ref(), "Processing todos (3/10)");

            let cycle = FlowPhase::CycleComplete { iteration: 7 };
            assert_eq!(cycle.description().as_ref(), "Cycle 7 complete");

            let failed = FlowPhase::Failed {
                reason: "timeout".to_string(),
            };
            assert_eq!(failed.description().as_ref(), "Failed: timeout");
        }

        /// Tests terminal state detection for successful completion.
        #[test]
        fn is_terminal_completed_is_true() {
            assert!(FlowPhase::Completed.is_terminal());
        }

        /// Tests terminal state detection for failure.
        #[test]
        fn is_terminal_failed_is_true() {
            let failed = FlowPhase::Failed {
                reason: "error".to_string(),
            };
            assert!(failed.is_terminal());
        }

        /// Tests terminal state detection when no todo files exist.
        #[test]
        fn is_terminal_no_todo_files_is_true() {
            assert!(FlowPhase::NoTodoFiles.is_terminal());
        }

        /// Tests that non-terminal phases return false.
        #[test]
        fn is_terminal_non_terminal_phases_are_false() {
            let non_terminal = [
                FlowPhase::Idle,
                FlowPhase::ReadingInput,
                FlowPhase::CheckingDoneFiles,
                FlowPhase::CheckingTodoFiles,
                FlowPhase::MovingCompletedFiles,
                FlowPhase::RunningPlanning {
                    model_name: Cow::Borrowed("Test"),
                    attempt: 1,
                },
                FlowPhase::RunningExecution {
                    model_name: Cow::Borrowed("Test"),
                    file_index: 1,
                    attempt: 1,
                },
                FlowPhase::ProcessingTodos {
                    current: 0,
                    total: 5,
                },
                FlowPhase::CycleComplete { iteration: 1 },
            ];

            for phase in non_terminal {
                assert!(
                    !phase.is_terminal(),
                    "Expected {phase:?} to not be terminal"
                );
            }
        }

        /// Tests that `FlowPhase` implements Clone correctly.
        #[test]
        fn clone_preserves_all_fields() {
            let original = FlowPhase::RunningExecution {
                model_name: Cow::Borrowed("Codex"),
                file_index: 42,
                attempt: 5,
            };
            let cloned = original.clone();

            assert_eq!(original, cloned);
        }

        /// Tests equality comparison for phases with same values.
        #[test]
        fn eq_same_values_are_equal() {
            let a = FlowPhase::ProcessingTodos {
                current: 3,
                total: 5,
            };
            let b = FlowPhase::ProcessingTodos {
                current: 3,
                total: 5,
            };
            assert_eq!(a, b);
        }

        /// Tests inequality for phases with different values.
        #[test]
        fn eq_different_values_are_not_equal() {
            let a = FlowPhase::ProcessingTodos {
                current: 3,
                total: 5,
            };
            let b = FlowPhase::ProcessingTodos {
                current: 4,
                total: 5,
            };
            assert_ne!(a, b);
        }
    }

    // =========================================================================
    // FlowState Tests
    // =========================================================================

    mod flow_state {
        use super::*;

        /// Tests creating a `FlowState` with an input file path.
        #[test]
        fn new_with_path_sets_correct_defaults() {
            let path = PathBuf::from("/test/input.txt");
            let state = FlowState::new(path.clone());

            assert_eq!(state.phase, FlowPhase::Idle);
            assert!(state.input_text.is_empty());
            assert_eq!(state.input_path, Some(path));
            assert!(state.todo_files.is_empty());
            assert_eq!(state.cycle_count, 0);
        }

        /// Tests creating a `FlowState` without an input file.
        #[test]
        fn new_without_file_sets_correct_defaults() {
            let state = FlowState::new_without_file();

            assert_eq!(state.phase, FlowPhase::Idle);
            assert!(state.input_text.is_empty());
            assert!(state.input_path.is_none());
            assert!(state.todo_files.is_empty());
            assert_eq!(state.cycle_count, 0);
        }

        /// Tests setting input text updates the state correctly.
        #[test]
        fn set_input_text_updates_text() {
            let mut state = FlowState::new_without_file();
            let text = "Build a REST API with authentication".to_string();

            state.set_input_text(text.clone());

            assert_eq!(state.input_text, text);
        }

        /// Tests that `set_input_text` replaces previous text.
        #[test]
        fn set_input_text_replaces_previous() {
            let mut state = FlowState::new_without_file();

            state.set_input_text("First task".to_string());
            state.set_input_text("Second task".to_string());

            assert_eq!(state.input_text, "Second task");
        }

        /// Tests that `FlowState` can be cloned.
        #[test]
        fn clone_creates_independent_copy() {
            let mut original = FlowState::new(PathBuf::from("/test.txt"));
            original.set_input_text("Task".to_string());
            original.cycle_count = 5;

            let mut cloned = original.clone();
            cloned.set_input_text("Modified".to_string());
            cloned.cycle_count = 10;

            assert_eq!(original.input_text, "Task");
            assert_eq!(original.cycle_count, 5);
            assert_eq!(cloned.input_text, "Modified");
            assert_eq!(cloned.cycle_count, 10);
        }

        /// Tests that `todo_files` can be modified.
        #[test]
        fn todo_files_can_be_populated() {
            let mut state = FlowState::new_without_file();

            state.todo_files.push(PathBuf::from("todo/task-001.md"));
            state.todo_files.push(PathBuf::from("todo/task-002.md"));

            assert_eq!(state.todo_files.len(), 2);
        }

        /// Tests that phase can be updated directly.
        #[test]
        fn phase_can_be_updated() {
            let mut state = FlowState::new_without_file();
            assert_eq!(state.phase, FlowPhase::Idle);

            state.phase = FlowPhase::ReadingInput;
            assert_eq!(state.phase, FlowPhase::ReadingInput);

            state.phase = FlowPhase::Completed;
            assert_eq!(state.phase, FlowPhase::Completed);
        }
    }
}
