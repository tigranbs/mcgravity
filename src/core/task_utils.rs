//! Task summary extraction and manipulation utilities.
//!
//! This module contains functions for extracting, formatting, and upserting
//! task summaries into the `<COMPLETED_TASKS>` block of task text.

use std::path::PathBuf;

use crate::fs::read_file_content;

/// Maximum number of lines to read from each pending task file for the summary.
const MAX_SUMMARY_LINES: usize = 5;

/// Maximum length for a one-line task summary.
const MAX_SUMMARY_LENGTH: usize = 100;

/// Opening tag for the completed tasks section.
const COMPLETED_TASKS_OPEN: &str = "<COMPLETED_TASKS>";
/// Closing tag for the completed tasks section.
const COMPLETED_TASKS_CLOSE: &str = "</COMPLETED_TASKS>";

/// Extracts a concise one-line summary from task content.
///
/// The summary is constructed from:
/// 1. The task title line (e.g., "# Task NNN: Title")
/// 2. The objective line from "## Objective" section
///
/// Falls back to the first non-empty line if the standard format is not found.
/// The result is truncated to `MAX_SUMMARY_LENGTH` characters.
///
/// # Arguments
///
/// * `content` - The full content of the task file
///
/// # Returns
///
/// A single-line summary of the task.
#[must_use]
pub fn extract_task_summary(content: &str) -> String {
    let mut title: Option<&str> = None;
    let mut objective: Option<&str> = None;
    let mut in_objective_section = false;
    let mut first_non_empty: Option<&str> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track first non-empty line as fallback
        if first_non_empty.is_none() && !trimmed.is_empty() {
            first_non_empty = Some(trimmed);
        }

        // Extract title from "# Task NNN: Title" format
        if title.is_none() && trimmed.starts_with("# Task") {
            // Remove the leading "# " and use the rest as title
            title = Some(trimmed.strip_prefix("# ").unwrap_or(trimmed));
        }

        // Check for ## Objective section start
        if trimmed.starts_with("## Objective") {
            in_objective_section = true;
            continue;
        }

        // If in objective section, capture the first non-empty line
        if in_objective_section && objective.is_none() {
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                objective = Some(trimmed);
                break; // We have what we need
            } else if trimmed.starts_with('#') {
                // Hit another section, stop looking for objective
                break;
            }
        }
    }

    // Build the summary
    let summary = match (title, objective) {
        (Some(t), Some(o)) => format!("{t}\n{o}"),
        (Some(t), None) => t.to_string(),
        (None, Some(o)) => o.to_string(),
        (None, None) => first_non_empty.unwrap_or("").to_string(),
    };

    // Truncate to max length safely at char boundary
    truncate_summary(&summary, MAX_SUMMARY_LENGTH)
}

/// Truncates a string to a maximum length, adding "..." if truncated.
/// Ensures truncation happens at a char boundary.
#[must_use]
pub fn truncate_summary(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }

    // Find a safe char boundary
    let mut end = max_len.saturating_sub(3); // Leave room for "..."
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...", &s[..end])
}

/// Extracts the completed tasks summary from a task text's `<COMPLETED_TASKS>` block.
///
/// # Arguments
///
/// * `task_text` - The task text that may contain a `<COMPLETED_TASKS>` block
///
/// # Returns
///
/// The content between `<COMPLETED_TASKS>` and `</COMPLETED_TASKS>` tags, or empty string if not found.
#[must_use]
pub fn extract_completed_tasks_summary(task_text: &str) -> String {
    if let Some(open_pos) = task_text.find(COMPLETED_TASKS_OPEN)
        && let Some(close_pos) = task_text.find(COMPLETED_TASKS_CLOSE)
    {
        let content_start = open_pos + COMPLETED_TASKS_OPEN.len();
        if content_start < close_pos {
            return task_text[content_start..close_pos].trim().to_string();
        }
    }
    String::new()
}

/// Upserts a task summary into the `<COMPLETED_TASKS>` section of task text.
///
/// This function:
/// - Creates the `<COMPLETED_TASKS>` block if it doesn't exist (appended at the end)
/// - Appends the summary line if not already present (deduplication)
/// - Preserves the rest of the task text unchanged
///
/// # Arguments
///
/// * `task_text` - The task text that may or may not contain a `<COMPLETED_TASKS>` block
/// * `summary_line` - The single-line summary to add (format: "- filename.md:\nSummary text")
///
/// # Returns
///
/// The updated task text with the summary added to the `<COMPLETED_TASKS>` block.
#[must_use]
pub fn upsert_completed_task_summary(task_text: &str, summary_line: &str) -> String {
    // Check if the summary line already exists (deduplication)
    let trimmed_summary = summary_line.trim();
    if task_text.contains(trimmed_summary) {
        return task_text.to_string();
    }

    // Check if COMPLETED_TASKS block exists
    if let Some(open_pos) = task_text.find(COMPLETED_TASKS_OPEN)
        && let Some(close_pos) = task_text.find(COMPLETED_TASKS_CLOSE)
    {
        // Block exists, insert before the closing tag
        let before_close = &task_text[..close_pos];
        let after_close = &task_text[close_pos..];

        // Determine if we need a newline before the summary
        let needs_newline = !before_close.ends_with('\n')
            && !before_close[open_pos + COMPLETED_TASKS_OPEN.len()..]
                .trim()
                .is_empty();

        let separator = if needs_newline { "\n" } else { "" };

        // Check if there's existing content and add proper spacing
        let existing_content = &task_text[open_pos + COMPLETED_TASKS_OPEN.len()..close_pos].trim();
        if existing_content.is_empty() {
            return format!("{before_close}{trimmed_summary}\n{after_close}");
        }
        return format!("{before_close}{separator}{trimmed_summary}\n{after_close}");
    }

    // Block doesn't exist, append at the end
    let separator = if task_text.ends_with('\n') { "" } else { "\n" };
    format!(
        "{task_text}{separator}\n{COMPLETED_TASKS_OPEN}\n{trimmed_summary}\n{COMPLETED_TASKS_CLOSE}\n"
    )
}

/// Generates a summary of task files (pending or done) for the planning phase.
///
/// For each task file, this function reads the filename and first few lines
/// to provide context to the AI planner.
///
/// # Arguments
///
/// * `tasks` - List of task file paths
///
/// # Returns
///
/// A formatted string summarizing tasks, or empty string if none exist.
pub async fn summarize_task_files(tasks: &[PathBuf]) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let mut summaries = Vec::with_capacity(tasks.len());

    for task_path in tasks {
        let file_name = task_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Read the file content, take first few lines as a snippet
        let snippet = match read_file_content(task_path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().take(MAX_SUMMARY_LINES).collect();
                let snippet = lines.join("\n");
                if content.lines().count() > MAX_SUMMARY_LINES {
                    format!("{snippet}\n...")
                } else {
                    snippet
                }
            }
            Err(_) => "[Could not read file content]".to_string(),
        };

        summaries.push(format!("- {file_name}:\n{snippet}"));
    }

    summaries.join("\n\n")
}

/// Generates file references for completed task files.
///
/// This function creates a list of relative file references (e.g., `.mcgravity/todo/done/task-001.md`)
/// suitable for inclusion in the `<COMPLETED_TASKS>` context section. The AI model can read these
/// files directly if it needs the full content.
///
/// # Arguments
///
/// * `tasks` - List of completed task file paths
///
/// # Returns
///
/// A formatted string with one file reference per line, or empty string if none exist.
#[must_use]
pub fn summarize_completed_tasks(tasks: &[PathBuf]) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let mut references = Vec::with_capacity(tasks.len());

    for task_path in tasks {
        // Convert to relative path for portability
        let relative_path = task_path
            .strip_prefix(std::env::current_dir().unwrap_or_default())
            .unwrap_or(task_path);

        references.push(format!("- {}", relative_path.display()));
    }

    references.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    // =========================================================================
    // summarize_task_files Tests
    // =========================================================================

    mod summarize_task_files_tests {
        use super::*;

        /// Tests that empty file list returns empty string.
        #[tokio::test]
        async fn empty_list_returns_empty_string() {
            let summary = summarize_task_files(&[]).await;
            assert!(summary.is_empty());
        }

        /// Tests that summary includes filename.
        #[tokio::test]
        async fn includes_filename() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("task-001.md");
            fs::write(&file_path, "Task content").await?;

            let summary = summarize_task_files(&[file_path]).await;

            assert!(
                summary.contains("task-001.md"),
                "Summary should contain filename"
            );
            Ok(())
        }

        /// Tests that summary includes file content snippet.
        #[tokio::test]
        async fn includes_content_snippet() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("task-001.md");
            fs::write(&file_path, "# Task\nThis is the task content").await?;

            let summary = summarize_task_files(&[file_path]).await;

            assert!(
                summary.contains("# Task"),
                "Summary should contain content snippet"
            );
            assert!(
                summary.contains("This is the task content"),
                "Summary should contain content"
            );
            Ok(())
        }

        /// Tests that summary truncates long files.
        #[tokio::test]
        async fn truncates_long_files() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("task-001.md");
            let content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8";
            fs::write(&file_path, content).await?;

            let summary = summarize_task_files(&[file_path]).await;

            assert!(
                summary.contains("Line 1"),
                "Summary should contain first lines"
            );
            assert!(
                summary.contains("Line 5"),
                "Summary should contain up to MAX_SUMMARY_LINES"
            );
            assert!(
                !summary.contains("Line 6"),
                "Summary should NOT contain lines beyond limit"
            );
            assert!(
                summary.contains("..."),
                "Summary should contain truncation indicator"
            );
            Ok(())
        }

        /// Tests that summary handles multiple files.
        #[tokio::test]
        async fn handles_multiple_files() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let file1 = dir.path().join("task-001.md");
            let file2 = dir.path().join("task-002.md");
            fs::write(&file1, "Task 1 content").await?;
            fs::write(&file2, "Task 2 content").await?;

            let summary = summarize_task_files(&[file1, file2]).await;

            assert!(
                summary.contains("task-001.md"),
                "Summary should contain first filename"
            );
            assert!(
                summary.contains("task-002.md"),
                "Summary should contain second filename"
            );
            assert!(
                summary.contains("Task 1 content"),
                "Summary should contain first file content"
            );
            assert!(
                summary.contains("Task 2 content"),
                "Summary should contain second file content"
            );
            Ok(())
        }

        /// Tests that summary handles unreadable files gracefully.
        #[tokio::test]
        async fn handles_unreadable_files() {
            let nonexistent = PathBuf::from("/nonexistent/path/task.md");

            let summary = summarize_task_files(&[nonexistent]).await;

            assert!(
                summary.contains("task.md"),
                "Summary should contain filename"
            );
            assert!(
                summary.contains("[Could not read file content]"),
                "Summary should indicate read failure"
            );
        }
    }

    // =========================================================================
    // extract_task_summary Tests
    // =========================================================================

    mod extract_task_summary_tests {
        use super::*;

        /// Tests extracting summary from standard task format.
        #[test]
        fn extracts_title_and_objective() {
            let content = r"# Task 001: Setup Database

## Objective
Configure the PostgreSQL database connection and create initial schema.

## Context
This is needed for the backend to store data.

## Implementation Steps
1. Add postgres crate
2. Create connection pool
";
            let summary = extract_task_summary(content);
            assert!(summary.contains("Task 001: Setup Database"));
            assert!(summary.contains("Configure the PostgreSQL"));
        }

        /// Tests extracting summary when only title is present.
        #[test]
        fn extracts_title_only() {
            let content = r"# Task 002: Add Authentication

Some description without objective section.
";
            let summary = extract_task_summary(content);
            assert!(summary.contains("Task 002: Add Authentication"));
            assert!(!summary.contains("Some description"));
        }

        /// Tests extracting summary when only objective is present.
        #[test]
        fn extracts_objective_only() {
            let content = r"## Objective
Implement the login flow for users.

## Steps
1. Create form
";
            let summary = extract_task_summary(content);
            assert!(summary.contains("Implement the login flow"));
        }

        /// Tests fallback to first non-empty line.
        #[test]
        fn falls_back_to_first_line() {
            let content = r"This is a simple task description without headers.

More details here.
";
            let summary = extract_task_summary(content);
            assert!(summary.contains("This is a simple task description"));
        }

        /// Tests handling of empty content.
        #[test]
        fn handles_empty_content() {
            let summary = extract_task_summary("");
            assert!(summary.is_empty());
        }

        /// Tests handling of whitespace-only content.
        #[test]
        fn handles_whitespace_content() {
            let summary = extract_task_summary("   \n\n   \n");
            assert!(summary.is_empty());
        }

        /// Tests that objective section with blank lines is handled.
        #[test]
        fn handles_objective_with_blank_lines() {
            let content = r"# Task 003: Fix Bug

## Objective

Fix the null pointer exception in the parser.

## Context
Bug reported by user.
";
            let summary = extract_task_summary(content);
            assert!(summary.contains("Task 003: Fix Bug"));
            assert!(summary.contains("Fix the null pointer exception"));
        }

        /// Tests truncation of long summaries.
        #[test]
        fn truncates_long_summary() {
            let long_title = format!("# Task 001: {}", "A".repeat(200));
            let summary = extract_task_summary(&long_title);
            assert!(summary.len() <= MAX_SUMMARY_LENGTH);
            assert!(summary.ends_with("..."));
        }
    }

    // =========================================================================
    // truncate_summary Tests
    // =========================================================================

    mod truncate_summary_tests {
        use super::*;

        /// Tests that short strings are not truncated.
        #[test]
        fn short_string_unchanged() {
            let result = truncate_summary("Hello", 100);
            assert_eq!(result, "Hello");
        }

        /// Tests that exactly max length strings are not truncated.
        #[test]
        fn exact_length_unchanged() {
            let input = "A".repeat(50);
            let result = truncate_summary(&input, 50);
            assert_eq!(result, input);
        }

        /// Tests that long strings are truncated with ellipsis.
        #[test]
        fn long_string_truncated() {
            let input = "A".repeat(100);
            let result = truncate_summary(&input, 50);
            assert!(result.len() <= 50);
            assert!(result.ends_with("..."));
        }

        /// Tests truncation at char boundary with multibyte chars.
        #[test]
        fn truncates_at_char_boundary() {
            // UTF-8 chars: "こんにちは" is 15 bytes (3 bytes each)
            let input = "こんにちは世界"; // 21 bytes
            let result = truncate_summary(input, 12);
            // Should truncate at a char boundary, not mid-character
            assert!(result.is_char_boundary(result.len() - 3)); // Before "..."
        }

        /// Tests empty string handling.
        #[test]
        fn empty_string_unchanged() {
            let result = truncate_summary("", 100);
            assert!(result.is_empty());
        }
    }

    // =========================================================================
    // extract_completed_tasks_summary Tests
    // =========================================================================

    mod extract_completed_tasks_summary_tests {
        use super::*;

        /// Tests extracting summary from task text with `COMPLETED_TASKS` block.
        #[test]
        fn extracts_existing_summary() {
            let task_text = r"Initial description

<COMPLETED_TASKS>
- task-001.md:
Task 001: Setup Database
- task-002.md:
Task 002: Create Models
</COMPLETED_TASKS>

Other content.
";
            let summary = extract_completed_tasks_summary(task_text);

            assert!(summary.contains("task-001.md"));
            assert!(summary.contains("task-002.md"));
            assert!(summary.contains("Setup Database"));
        }

        /// Tests that empty `COMPLETED_TASKS` block returns empty string.
        #[test]
        fn empty_block_returns_empty_string() {
            let task_text = r"Initial description

<COMPLETED_TASKS>

</COMPLETED_TASKS>

Other content.
";
            let summary = extract_completed_tasks_summary(task_text);
            assert!(summary.is_empty());
        }

        /// Tests that missing `COMPLETED_TASKS` block returns empty string.
        #[test]
        fn missing_block_returns_empty_string() {
            let task_text = "Initial description without COMPLETED_TASKS block";
            let summary = extract_completed_tasks_summary(task_text);
            assert!(summary.is_empty());
        }

        /// Tests that malformed `COMPLETED_TASKS` block (missing close tag) returns empty.
        #[test]
        fn malformed_block_returns_empty_string() {
            let task_text = r"Initial description

<COMPLETED_TASKS>
- task-001.md:
Setup
";
            let summary = extract_completed_tasks_summary(task_text);
            assert!(summary.is_empty());
        }
    }

    // =========================================================================
    // upsert_completed_task_summary Tests
    // =========================================================================

    mod upsert_completed_task_summary_tests {
        use super::*;

        /// Tests creating a new `COMPLETED_TASKS` block when none exists.
        #[test]
        fn creates_block_when_missing() {
            let task_text = "# Task 005: Implement Feature\n\nSome content.";
            let summary = "- task-001.md:\nTask 001: Setup Database";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("<COMPLETED_TASKS>"));
            assert!(result.contains("</COMPLETED_TASKS>"));
            assert!(result.contains("task-001.md"));
        }

        /// Tests appending to existing `COMPLETED_TASKS` block.
        #[test]
        fn appends_to_existing_block() {
            let task_text = r"# Task 005

<COMPLETED_TASKS>
- task-001.md:
Task 001: Setup
</COMPLETED_TASKS>

Some content.
";
            let summary = "- task-002.md:\nTask 002: Create Models";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("task-001.md"));
            assert!(result.contains("task-002.md"));
            // Original content should be preserved
            assert!(result.contains("Some content."));
        }

        /// Tests deduplication - same summary is not added twice.
        #[test]
        fn deduplicates_existing_summary() {
            let task_text = r"# Task 005

<COMPLETED_TASKS>
- task-001.md:
Task 001: Setup Database
</COMPLETED_TASKS>
";
            let summary = "- task-001.md:\nTask 001: Setup Database";

            let result = upsert_completed_task_summary(task_text, summary);

            // Should return unchanged since summary already exists
            // Count occurrences of task-001.md
            let count = result.matches("task-001.md").count();
            assert_eq!(count, 1, "Should not duplicate the summary");
        }

        /// Tests handling of empty `COMPLETED_TASKS` block.
        #[test]
        fn handles_empty_block() {
            let task_text = r"# Task 005

<COMPLETED_TASKS>

</COMPLETED_TASKS>

Content here.
";
            let summary = "- task-001.md:\nTask 001: Setup";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("task-001.md"));
            assert!(result.contains("Content here."));
        }

        /// Tests that task text without trailing newline is handled.
        #[test]
        fn handles_no_trailing_newline() {
            let task_text = "# Task 005\n\nContent";
            let summary = "- task-001.md:\nSetup";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("task-001.md"));
            assert!(result.contains("<COMPLETED_TASKS>"));
        }

        /// Tests preserving content after `COMPLETED_TASKS` block.
        #[test]
        fn preserves_content_after_block() {
            let task_text = r"<COMPLETED_TASKS>
- task-001.md:
Setup
</COMPLETED_TASKS>

## Guidelines
Follow best practices.
";
            let summary = "- task-002.md:\nModels";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("## Guidelines"));
            assert!(result.contains("Follow best practices"));
        }
    }

    // =========================================================================
    // summarize_completed_tasks Tests
    // =========================================================================

    mod summarize_completed_tasks_tests {
        use super::*;

        /// Tests that empty file list returns empty string.
        #[test]
        fn empty_list_returns_empty_string() {
            let summary = summarize_completed_tasks(&[]);
            assert!(summary.is_empty());
        }

        /// Tests that summary includes file path reference.
        #[test]
        fn includes_file_path_reference() {
            let file_path = PathBuf::from(".mcgravity/todo/done/task-001.md");
            let summary = summarize_completed_tasks(&[file_path]);

            assert!(
                summary.contains(".mcgravity/todo/done/task-001.md"),
                "Summary should contain file path reference"
            );
            assert!(
                summary.starts_with("- "),
                "Summary should start with list marker"
            );
        }

        /// Tests that summary contains only file references, not task content.
        #[test]
        fn contains_only_file_references_not_content() {
            let file_path = PathBuf::from(".mcgravity/todo/done/task-001.md");
            let summary = summarize_completed_tasks(&[file_path]);

            // Should contain the file reference
            assert!(summary.contains("task-001.md"));
            // Should NOT contain any task content markers
            assert!(
                !summary.contains("Task 001:"),
                "Should not contain task title - only file reference"
            );
            assert!(
                !summary.contains("Objective"),
                "Should not contain objective section"
            );
            assert!(
                !summary.contains("Setup"),
                "Should not contain task content"
            );
        }

        /// Tests handling multiple files.
        #[test]
        fn handles_multiple_files() {
            let file1 = PathBuf::from(".mcgravity/todo/done/task-001.md");
            let file2 = PathBuf::from(".mcgravity/todo/done/task-002.md");

            let summary = summarize_completed_tasks(&[file1, file2]);

            assert!(summary.contains(".mcgravity/todo/done/task-001.md"));
            assert!(summary.contains(".mcgravity/todo/done/task-002.md"));
            // Should be on separate lines
            assert!(
                summary.contains('\n'),
                "Multiple files should be on separate lines"
            );
            // Should NOT contain task titles or objectives
            assert!(
                !summary.contains("Task 001"),
                "Should not contain task titles"
            );
            assert!(
                !summary.contains("Task 002"),
                "Should not contain task titles"
            );
        }

        /// Tests that file paths are formatted as list items.
        #[test]
        fn formats_as_list_items() {
            let file1 = PathBuf::from(".mcgravity/todo/done/task-001.md");
            let file2 = PathBuf::from(".mcgravity/todo/done/task-002.md");

            let summary = summarize_completed_tasks(&[file1, file2]);

            // Each line should start with "- "
            for line in summary.lines() {
                assert!(
                    line.starts_with("- "),
                    "Each line should be a list item starting with '- '"
                );
            }
        }
    }
}
