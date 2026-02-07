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

/// Maximum length for a completed-task summary entry in `<COMPLETED_TASKS>`.
const MAX_ENTRY_LENGTH: usize = 500;

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
    extract_task_summary_with_max_len(content, MAX_SUMMARY_LENGTH)
}

/// Extracts a summary from task content with a configurable maximum length.
///
/// Behaves identically to [`extract_task_summary`] but allows callers to
/// specify a custom truncation limit. Use this when the caller's budget is
/// larger than the default `MAX_SUMMARY_LENGTH` (e.g. the 500-character
/// completed-task entry budget).
///
/// # Arguments
///
/// * `content` - The full content of the task file
/// * `max_len` - Maximum character length for the returned summary
///
/// # Returns
///
/// A single-line summary of the task, truncated to `max_len`.
#[must_use]
pub fn extract_task_summary_with_max_len(content: &str, max_len: usize) -> String {
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
    truncate_summary(&summary, max_len)
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

/// Returns `true` if the string contains a path reference that should not appear
/// in a completed-task summary entry.
///
/// Detected patterns:
/// - Absolute Unix paths (`/home/`, `/tmp/`, `/var/`, `/usr/`, `/etc/`)
/// - `.mcgravity/todo/done/` references (forward or back-slash)
/// - `.mcgravity\todo\done\` references (Windows backslash variant)
/// - Current-dir relative paths (`./`, `.\\`)
/// - Parent-relative paths (`../`, `..\\`)
/// - Repo-style paths like `src/`, `docs/`, `tests/` etc.
/// - Windows drive paths (`C:\`, `D:\`, etc.)
#[must_use]
fn contains_path_reference(s: &str) -> bool {
    let unix_prefixes = ["/home/", "/tmp/", "/var/", "/usr/", "/etc/"];
    if unix_prefixes.iter().any(|p| s.contains(p)) {
        return true;
    }
    // .mcgravity paths with forward or back-slash
    if s.contains(".mcgravity/todo/done/") || s.contains(".mcgravity\\todo\\done\\") {
        return true;
    }
    // Relative paths: ./  or .\ (current-dir relative)
    if s.contains("./") || s.contains(".\\") {
        return true;
    }
    // Parent-relative paths: ../ or ..\ (checked explicitly to guard against
    // future refactors that might decouple them from the ./ / .\ check above)
    if s.contains("../") || s.contains("..\\") {
        return true;
    }
    // Repo-style relative paths (word followed by `/` containing path separators deeper)
    // Match patterns like `src/foo`, `docs/bar.md`, `tests/something`
    if has_repo_style_path(s) {
        return true;
    }
    // Windows drive paths: single letter followed by `:\ ` or `:/`
    for window in s.as_bytes().windows(3) {
        if window[0].is_ascii_alphabetic()
            && window[1] == b':'
            && (window[2] == b'\\' || window[2] == b'/')
        {
            return true;
        }
    }
    false
}

/// Detects repo-style relative paths such as `src/core/mod.rs` or `tests/unit.rs`.
///
/// A repo-style path is a word token containing at least one `/` where the segment
/// before the first `/` looks like a common source directory name.
#[must_use]
fn has_repo_style_path(s: &str) -> bool {
    // Common repo directory prefixes that indicate a file path reference
    const REPO_PREFIXES: &[&str] = &[
        "src/",
        "lib/",
        "bin/",
        "tests/",
        "test/",
        "docs/",
        "doc/",
        "pkg/",
        "cmd/",
        "internal/",
        "config/",
        "configs/",
        "build/",
        "target/",
        "dist/",
        "out/",
        "node_modules/",
        "vendor/",
        "crates/",
        "packages/",
    ];
    for prefix in REPO_PREFIXES {
        if s.contains(prefix) {
            return true;
        }
    }
    false
}

/// Normalizes a single completed-task entry line.
///
/// - Strips the line if it contains path references (returns `None`)
/// - Strips bare list markers (`-`, `- `) with no meaningful content
/// - Truncates to `MAX_ENTRY_LENGTH` characters
/// - Returns `None` for empty/whitespace-only lines
#[must_use]
fn normalize_entry_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Reject bare list markers with no meaningful content
    let without_marker = trimmed.strip_prefix('-').unwrap_or(trimmed).trim();
    if without_marker.is_empty() {
        return None;
    }
    if contains_path_reference(trimmed) {
        return None;
    }
    Some(truncate_summary(trimmed, MAX_ENTRY_LENGTH))
}

/// Normalizes the content of a `<COMPLETED_TASKS>` block.
///
/// Each line is checked for path references and truncated. Lines that contain
/// path references are removed entirely. The result contains only short,
/// path-free summary text.
///
/// # Arguments
///
/// * `block_content` - The raw text between `<COMPLETED_TASKS>` and `</COMPLETED_TASKS>` tags
///
/// # Returns
///
/// The cleaned block content with path-containing lines removed and remaining
/// lines truncated to `MAX_ENTRY_LENGTH`.
#[must_use]
pub fn normalize_completed_tasks_block(block_content: &str) -> String {
    block_content
        .lines()
        .filter_map(normalize_entry_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Normalizes a summary string produced by a model or `extract_task_summary`.
///
/// Collapses internal whitespace and newlines into a single line, strips path
/// references, and truncates to `MAX_ENTRY_LENGTH`. Returns `None` if the
/// result is empty after sanitization (caller should fall back to a local
/// summary).
#[must_use]
pub fn normalize_summary_entry(summary: &str) -> Option<String> {
    // Collapse all internal whitespace (including newlines) into single spaces
    let single_line: String = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.is_empty() || contains_path_reference(&single_line) {
        return None;
    }
    Some(truncate_summary(&single_line, MAX_ENTRY_LENGTH))
}

/// Normalizes the `<COMPLETED_TASKS>` block inside a full task text in-place.
///
/// Finds the block, normalizes its content (removing path-containing entries,
/// truncating long entries), and returns the updated task text.
///
/// If no `<COMPLETED_TASKS>` block is found, the text is returned unchanged.
#[must_use]
pub fn normalize_task_text_completed_section(task_text: &str) -> String {
    let Some(open_pos) = task_text.find(COMPLETED_TASKS_OPEN) else {
        return task_text.to_string();
    };
    let Some(close_pos) = task_text.find(COMPLETED_TASKS_CLOSE) else {
        return task_text.to_string();
    };

    let content_start = open_pos + COMPLETED_TASKS_OPEN.len();
    if content_start >= close_pos {
        return task_text.to_string();
    }

    let raw_content = &task_text[content_start..close_pos];
    let normalized = normalize_completed_tasks_block(raw_content);

    let before = &task_text[..content_start];
    let after = &task_text[close_pos..];

    if normalized.is_empty() {
        format!("{before}\n{after}")
    } else {
        format!("{before}\n{normalized}\n{after}")
    }
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
    // extract_task_summary_with_max_len Tests
    // =========================================================================

    mod extract_task_summary_with_max_len_tests {
        use super::*;

        /// Tests that default-length extraction matches `extract_task_summary`.
        #[test]
        fn matches_default_at_100_chars() {
            let content = r"# Task 001: Setup Database

## Objective
Configure the PostgreSQL database connection and create initial schema.
";
            let default_summary = extract_task_summary(content);
            let explicit_100 = extract_task_summary_with_max_len(content, MAX_SUMMARY_LENGTH);
            assert_eq!(default_summary, explicit_100);
        }

        /// Tests that a 500-char budget preserves long objective text that the
        /// default 100-char extraction would truncate.
        #[test]
        fn preserves_long_content_at_500_chars() {
            let long_objective = "A".repeat(250);
            let content =
                format!("# Task 001: Long Objective Task\n\n## Objective\n{long_objective}\n");

            let short = extract_task_summary(&content);
            let long = extract_task_summary_with_max_len(&content, MAX_ENTRY_LENGTH);

            // The default extraction truncates to ~100 chars
            assert!(
                short.len() <= MAX_SUMMARY_LENGTH,
                "Default extraction should be capped at {MAX_SUMMARY_LENGTH}"
            );
            assert!(
                short.ends_with("..."),
                "Default extraction should be truncated"
            );

            // The 500-char extraction should preserve the full content
            assert!(
                long.len() > MAX_SUMMARY_LENGTH,
                "500-char extraction should exceed default 100-char cap; got {} chars",
                long.len()
            );
            assert!(
                long.len() <= MAX_ENTRY_LENGTH,
                "500-char extraction should be capped at {MAX_ENTRY_LENGTH}"
            );
        }

        /// Tests that extraction at the entry budget still truncates content
        /// that exceeds 500 characters.
        #[test]
        fn truncates_at_entry_budget() {
            let very_long_objective = "B".repeat(600);
            let content = format!("# Task 001: Very Long\n\n## Objective\n{very_long_objective}\n");
            let summary = extract_task_summary_with_max_len(&content, MAX_ENTRY_LENGTH);

            assert!(
                summary.len() <= MAX_ENTRY_LENGTH,
                "Should be capped at {MAX_ENTRY_LENGTH}, got {}",
                summary.len()
            );
            assert!(
                summary.ends_with("..."),
                "Should be truncated with ellipsis"
            );
        }

        /// Tests empty content returns empty string regardless of `max_len`.
        #[test]
        fn empty_content_returns_empty() {
            assert!(extract_task_summary_with_max_len("", MAX_ENTRY_LENGTH).is_empty());
            assert!(extract_task_summary_with_max_len("", MAX_SUMMARY_LENGTH).is_empty());
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

        /// Tests extracting summary from task text with `COMPLETED_TASKS` block containing inline summaries.
        #[test]
        fn extracts_existing_summary() {
            let task_text = r"Initial description

<COMPLETED_TASKS>
- Set up PostgreSQL database with connection pooling and initial schema
- Created data models and ORM mappings for user and product entities
</COMPLETED_TASKS>

Other content.
";
            let summary = extract_completed_tasks_summary(task_text);

            assert!(summary.contains("Set up PostgreSQL database"));
            assert!(summary.contains("Created data models"));
            // Summary entries must not contain done-file paths
            assert!(
                !summary.contains(".mcgravity/todo/done/"),
                "Summary should not contain done-file path references"
            );
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
            let summary = "- Set up PostgreSQL database with connection pooling";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("<COMPLETED_TASKS>"));
            assert!(result.contains("</COMPLETED_TASKS>"));
            assert!(result.contains("Set up PostgreSQL database"));
            assert!(
                !result.contains(".mcgravity/todo/done/"),
                "Should not contain done-file path references"
            );
        }

        /// Tests appending to existing `COMPLETED_TASKS` block.
        #[test]
        fn appends_to_existing_block() {
            let task_text = r"# Task 005

<COMPLETED_TASKS>
- Set up PostgreSQL database with connection pooling
</COMPLETED_TASKS>

Some content.
";
            let summary = "- Created data models and ORM mappings";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("Set up PostgreSQL database"));
            assert!(result.contains("Created data models"));
            // Original content should be preserved
            assert!(result.contains("Some content."));
            assert!(
                !result.contains(".mcgravity/todo/done/"),
                "Should not contain done-file path references"
            );
        }

        /// Tests deduplication - same summary is not added twice.
        #[test]
        fn deduplicates_existing_summary() {
            let task_text = r"# Task 005

<COMPLETED_TASKS>
- Set up PostgreSQL database with connection pooling
</COMPLETED_TASKS>
";
            let summary = "- Set up PostgreSQL database with connection pooling";

            let result = upsert_completed_task_summary(task_text, summary);

            // Should return unchanged since summary already exists
            let count = result
                .matches("Set up PostgreSQL database with connection pooling")
                .count();
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
            let summary = "- Set up PostgreSQL database with connection pooling";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("Set up PostgreSQL database"));
            assert!(result.contains("Content here."));
        }

        /// Tests that task text without trailing newline is handled.
        #[test]
        fn handles_no_trailing_newline() {
            let task_text = "# Task 005\n\nContent";
            let summary = "- Set up database schema";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("Set up database schema"));
            assert!(result.contains("<COMPLETED_TASKS>"));
        }

        /// Tests preserving content after `COMPLETED_TASKS` block.
        #[test]
        fn preserves_content_after_block() {
            let task_text = r"<COMPLETED_TASKS>
- Set up database with initial schema
</COMPLETED_TASKS>

## Guidelines
Follow best practices.
";
            let summary = "- Created data models and ORM mappings";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(result.contains("## Guidelines"));
            assert!(result.contains("Follow best practices"));
        }
    }

    // =========================================================================
    // summarize_completed_tasks Tests (Legacy)
    // =========================================================================

    mod summarize_completed_tasks_tests {
        use super::*;

        /// Tests that empty file list returns empty string.
        #[test]
        fn empty_list_returns_empty_string() {
            let summary = summarize_completed_tasks(&[]);
            assert!(summary.is_empty());
        }

        /// Tests that the legacy function still formats file path references correctly.
        /// Note: This function is preserved for backward compatibility but its output
        /// must NOT be used in `<COMPLETED_TASKS>` context (use inline summaries instead).
        #[test]
        fn includes_file_path_reference() {
            let file_path = PathBuf::from("src/core/task_utils.rs");
            let summary = summarize_completed_tasks(&[file_path]);

            assert!(
                summary.contains("src/core/task_utils.rs"),
                "Legacy summary should contain file path reference"
            );
            assert!(
                summary.starts_with("- "),
                "Summary should start with list marker"
            );
        }

        /// Tests handling multiple files.
        #[test]
        fn handles_multiple_files() {
            let file1 = PathBuf::from("src/core/runner.rs");
            let file2 = PathBuf::from("src/core/task_utils.rs");

            let summary = summarize_completed_tasks(&[file1, file2]);

            assert!(summary.contains("src/core/runner.rs"));
            assert!(summary.contains("src/core/task_utils.rs"));
            // Should be on separate lines
            assert!(
                summary.contains('\n'),
                "Multiple files should be on separate lines"
            );
        }

        /// Tests that file paths are formatted as list items.
        #[test]
        fn formats_as_list_items() {
            let file1 = PathBuf::from("src/core/runner.rs");
            let file2 = PathBuf::from("src/core/task_utils.rs");

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

    // =========================================================================
    // Regression: Summary entries must not contain path references
    // =========================================================================

    #[allow(clippy::unwrap_used)]
    mod summary_no_path_regression_tests {
        use super::*;

        /// Regression: `extract_task_summary` must not produce done-file paths.
        #[test]
        fn extract_task_summary_never_contains_done_paths() {
            let content = "# Task 001: Setup Database\n\n## Objective\nConfigure PostgreSQL.";
            let summary = extract_task_summary(content);

            assert!(
                !summary.contains(".mcgravity/todo/done/"),
                "extract_task_summary must not produce done-file paths"
            );
            assert!(
                !summary.contains("/home/"),
                "extract_task_summary must not produce absolute paths"
            );
            assert!(
                !summary.contains("/tmp/"),
                "extract_task_summary must not produce temp paths"
            );
        }

        /// Regression: summary entries stay under 500 characters.
        #[test]
        fn summary_entries_capped_at_500_chars() {
            let long_title = format!("# Task 001: {}", "A".repeat(600));
            let summary = extract_task_summary(&long_title);
            let entry = format!("- {summary}");
            let truncated = truncate_summary(&entry, 500);

            assert!(
                truncated.len() <= 500,
                "Summary entry must be capped at 500 characters, got {}",
                truncated.len()
            );
        }

        /// Regression: `upsert_completed_task_summary` with inline summaries produces no path leakage.
        #[test]
        fn upsert_summary_no_path_leakage() {
            let task_text = "Initial description";
            let summary = "- Added retry logic with exponential backoff for CLI executor";

            let result = upsert_completed_task_summary(task_text, summary);

            assert!(
                !result.contains(".mcgravity/todo/done/"),
                "Upserted task text must not contain done-file paths"
            );
            assert!(
                !result.contains("/home/"),
                "Upserted task text must not contain absolute paths"
            );
            assert!(result.contains("Added retry logic"));
        }

        /// Regression: `extract_completed_tasks_summary` returns inline summaries, not paths.
        #[test]
        fn extract_completed_summary_returns_inline_text() {
            let task_text = r"Task description

<COMPLETED_TASKS>
- Added retry logic with exponential backoff for CLI executor
- Implemented database connection pooling with health checks
</COMPLETED_TASKS>
";
            let summary = extract_completed_tasks_summary(task_text);

            assert!(summary.contains("Added retry logic"));
            assert!(summary.contains("Implemented database connection pooling"));
            assert!(
                !summary.contains(".mcgravity/todo/done/"),
                "Extracted summary must not contain done-file paths"
            );
            assert!(
                !summary.contains("/home/"),
                "Extracted summary must not contain absolute paths"
            );
            assert!(
                !summary.contains("/tmp/"),
                "Extracted summary must not contain temp paths"
            );
        }

        /// Regression: `normalize_completed_tasks_block` removes path-containing lines.
        #[test]
        fn normalize_block_strips_path_lines() {
            let block = "- /home/tigran/projects/ungravity/mcgravity/.mcgravity/todo/done/task-001.md\n\
                          - Added retry logic with exponential backoff\n\
                          - /tmp/scratch/task-002.md\n\
                          - Implemented database pooling";
            let normalized = normalize_completed_tasks_block(block);

            assert!(
                !normalized.contains("/home/"),
                "Normalized block must not contain /home/ paths"
            );
            assert!(
                !normalized.contains("/tmp/"),
                "Normalized block must not contain /tmp/ paths"
            );
            assert!(
                !normalized.contains(".mcgravity/todo/done/"),
                "Normalized block must not contain done-file paths"
            );
            assert!(normalized.contains("Added retry logic"));
            assert!(normalized.contains("Implemented database pooling"));
        }

        /// Regression: `normalize_completed_tasks_block` removes Windows drive paths.
        #[test]
        fn normalize_block_strips_windows_paths() {
            let block = "- C:\\Users\\user\\project\\.mcgravity\\todo\\done\\task-001.md\n\
                          - Set up PostgreSQL database";
            let normalized = normalize_completed_tasks_block(block);

            assert!(
                !normalized.contains("C:\\"),
                "Normalized block must not contain Windows drive paths"
            );
            assert!(normalized.contains("Set up PostgreSQL database"));
        }

        /// Regression: `normalize_completed_tasks_block` truncates long entries.
        #[test]
        fn normalize_block_truncates_long_entries() {
            let long_entry = format!("- {}", "X".repeat(600));
            let normalized = normalize_completed_tasks_block(&long_entry);

            assert!(
                normalized.len() <= MAX_ENTRY_LENGTH,
                "Entry should be truncated to {MAX_ENTRY_LENGTH} chars, got {}",
                normalized.len()
            );
            assert!(normalized.ends_with("..."));
        }

        /// Regression: `normalize_summary_entry` strips paths from model output.
        #[test]
        fn normalize_summary_entry_strips_model_paths() {
            let model_output =
                "Updated src/core/executor.rs to add /home/user/project/src/core/retry.rs";
            let result = normalize_summary_entry(model_output);

            assert!(
                result.is_none(),
                "Summary containing absolute paths should be normalized to None"
            );
        }

        /// Regression: `normalize_summary_entry` preserves clean text.
        #[test]
        fn normalize_summary_entry_preserves_clean_text() {
            let clean = "Added retry logic with exponential backoff for CLI executor";
            let result = normalize_summary_entry(clean);

            assert!(result.is_some());
            assert_eq!(result.unwrap(), clean);
        }

        /// Regression: `normalize_summary_entry` truncates long summaries.
        #[test]
        fn normalize_summary_entry_truncates_long() {
            let long = "A".repeat(600);
            let result = normalize_summary_entry(&long);

            assert!(result.is_some());
            let normalized = result.unwrap();
            assert!(
                normalized.len() <= MAX_ENTRY_LENGTH,
                "Should be truncated to {MAX_ENTRY_LENGTH} chars"
            );
        }

        /// Regression: `normalize_task_text_completed_section` cleans legacy paths in task text.
        #[test]
        fn normalize_task_text_cleans_legacy_paths() {
            let task_text = r"My task description

<COMPLETED_TASKS>
- /home/tigran/projects/ungravity/mcgravity/.mcgravity/todo/done/task-001.md
- /home/tigran/projects/ungravity/mcgravity/.mcgravity/todo/done/task-002.md
- Added retry logic with exponential backoff
</COMPLETED_TASKS>

More content here.
";
            let normalized = normalize_task_text_completed_section(task_text);

            assert!(
                !normalized.contains("/home/"),
                "Normalized task text must not contain /home/ paths"
            );
            assert!(
                !normalized.contains(".mcgravity/todo/done/"),
                "Normalized task text must not contain done-file paths"
            );
            assert!(
                normalized.contains("Added retry logic"),
                "Clean summaries must be preserved"
            );
            assert!(
                normalized.contains("More content here"),
                "Content after block must be preserved"
            );
            assert!(
                normalized.contains("My task description"),
                "Content before block must be preserved"
            );
        }

        /// Regression: `normalize_task_text_completed_section` is a no-op on clean text.
        #[test]
        fn normalize_task_text_noop_on_clean() {
            let task_text = r"My task description

<COMPLETED_TASKS>
- Added retry logic with exponential backoff
- Implemented database pooling
</COMPLETED_TASKS>
";
            let normalized = normalize_task_text_completed_section(task_text);

            assert!(normalized.contains("Added retry logic"));
            assert!(normalized.contains("Implemented database pooling"));
        }

        /// Regression: `normalize_task_text_completed_section` handles missing block.
        #[test]
        fn normalize_task_text_handles_missing_block() {
            let task_text = "Just a task description without completed tasks block.";
            let normalized = normalize_task_text_completed_section(task_text);

            assert_eq!(normalized, task_text);
        }

        /// Regression: `contains_path_reference` detects .mcgravity/todo/done/ references.
        #[test]
        fn contains_path_reference_detects_done_paths() {
            assert!(contains_path_reference(
                "- .mcgravity/todo/done/task-001.md"
            ));
            assert!(contains_path_reference("/home/user/project/file.rs"));
            assert!(contains_path_reference("/tmp/scratch/file.txt"));
            assert!(contains_path_reference("C:\\Users\\user\\file.rs"));
            assert!(!contains_path_reference(
                "Added retry logic with exponential backoff"
            ));
            assert!(!contains_path_reference("Set up PostgreSQL database"));
        }

        /// Regression: `contains_path_reference` detects relative paths (`./`, `../`).
        #[test]
        fn contains_path_reference_detects_relative_paths() {
            assert!(contains_path_reference("Updated ./config/settings.json"));
            assert!(contains_path_reference("Modified ../parent/file.rs"));
            assert!(contains_path_reference(
                ".mcgravity\\todo\\done\\task-001.md"
            ));
            // Parent-relative with Windows backslash
            assert!(contains_path_reference("Read ..\\secrets\\key.pem"));
            // Parent-relative at start of string
            assert!(contains_path_reference("../etc/passwd"));
            assert!(contains_path_reference("..\\Windows\\System32"));
            // Embedded parent-relative patterns
            assert!(contains_path_reference(
                "Loaded data from ../shared/config.toml"
            ));
            assert!(contains_path_reference(
                "Copied ..\\backup\\dump.sql into staging"
            ));
        }

        /// Regression: `contains_path_reference` detects repo-style paths (`src/...`).
        #[test]
        fn contains_path_reference_detects_repo_style_paths() {
            assert!(contains_path_reference(
                "Updated src/core/executor.rs to add retry logic"
            ));
            assert!(contains_path_reference("Changed tests/unit/test_auth.rs"));
            assert!(contains_path_reference("Modified docs/architecture.md"));
            assert!(contains_path_reference("Edited lib/utils.rs"));
        }

        /// Regression: `contains_path_reference` allows clean summary text.
        #[test]
        fn contains_path_reference_allows_clean_text() {
            assert!(!contains_path_reference(
                "Added retry logic with exponential backoff"
            ));
            assert!(!contains_path_reference(
                "Implemented database connection pooling"
            ));
            assert!(!contains_path_reference("Fixed authentication bug"));
            assert!(!contains_path_reference(
                "Refactored error handling for better user experience"
            ));
        }

        /// Regression: `normalize_summary_entry` rejects relative-path summaries.
        #[test]
        fn normalize_summary_entry_rejects_relative_paths() {
            assert!(normalize_summary_entry("Updated ./src/main.rs").is_none());
            assert!(normalize_summary_entry("Modified ../lib/auth.rs").is_none());
            assert!(normalize_summary_entry("Changed src/core/runner.rs to fix bug").is_none());
        }

        /// Regression: `normalize_summary_entry` rejects empty/whitespace input.
        #[test]
        fn normalize_summary_entry_rejects_empty() {
            assert!(normalize_summary_entry("").is_none());
            assert!(normalize_summary_entry("   ").is_none());
        }

        /// Regression: `normalize_completed_tasks_block` strips bare list markers.
        #[test]
        fn normalize_block_strips_bare_list_markers() {
            let block = "- \n- Added retry logic\n-\n- Implemented pooling";
            let normalized = normalize_completed_tasks_block(block);

            assert!(
                !normalized.contains("\n- \n"),
                "Bare list markers should be stripped"
            );
            assert!(
                !normalized.lines().any(|l| l.trim() == "-"),
                "Bare dash-only lines should be stripped"
            );
            assert!(normalized.contains("Added retry logic"));
            assert!(normalized.contains("Implemented pooling"));
        }

        /// Regression: `normalize_completed_tasks_block` strips relative-path entries.
        #[test]
        fn normalize_block_strips_relative_path_entries() {
            let block = "- Updated src/core/runner.rs\n\
                          - Added retry logic with exponential backoff\n\
                          - Modified ./config/settings.json\n\
                          - Implemented database pooling";
            let normalized = normalize_completed_tasks_block(block);

            assert!(
                !normalized.contains("src/core/runner.rs"),
                "Repo-style paths must be stripped"
            );
            assert!(
                !normalized.contains("./config/"),
                "Relative paths must be stripped"
            );
            assert!(normalized.contains("Added retry logic"));
            assert!(normalized.contains("Implemented database pooling"));
        }

        /// Regression: `normalize_completed_tasks_block` strips parent-relative path entries.
        #[test]
        fn normalize_block_strips_parent_relative_entries() {
            let block = "- Refactored ../lib/auth.rs for token refresh\n\
                          - Added retry logic with exponential backoff\n\
                          - Copied ..\\backup\\dump.sql into staging\n\
                          - Implemented database pooling";
            let normalized = normalize_completed_tasks_block(block);

            assert!(
                !normalized.contains("../lib/"),
                "Parent-relative Unix paths must be stripped"
            );
            assert!(
                !normalized.contains("..\\backup\\"),
                "Parent-relative Windows paths must be stripped"
            );
            assert!(normalized.contains("Added retry logic"));
            assert!(normalized.contains("Implemented database pooling"));
        }

        /// Regression: `normalize_summary_entry` rejects parent-relative path summaries.
        #[test]
        fn normalize_summary_entry_rejects_parent_relative_paths() {
            assert!(
                normalize_summary_entry("Refactored ../lib/auth.rs for token refresh").is_none()
            );
            assert!(normalize_summary_entry("Copied ..\\backup\\dump.sql into staging").is_none());
            assert!(normalize_summary_entry("../etc/passwd traversal").is_none());
            assert!(normalize_summary_entry("..\\Windows\\System32 reference").is_none());
        }

        /// Regression: `normalize_summary_entry` collapses multiline input into a single line.
        #[test]
        fn normalize_summary_entry_collapses_multiline() {
            let multiline = "Added retry logic\nwith exponential backoff\nfor CLI executor";
            let result = normalize_summary_entry(multiline);
            assert!(result.is_some());
            let normalized = result.unwrap();
            assert!(
                !normalized.contains('\n'),
                "Normalized entry must not contain newlines, got: {normalized:?}"
            );
            assert_eq!(
                normalized,
                "Added retry logic with exponential backoff for CLI executor"
            );
        }

        /// Regression: `normalize_summary_entry` collapses mixed whitespace and newlines.
        #[test]
        fn normalize_summary_entry_collapses_mixed_whitespace() {
            let messy = "  Implemented\n\n  database \t pooling  \n  with health checks  ";
            let result = normalize_summary_entry(messy);
            assert!(result.is_some());
            let normalized = result.unwrap();
            assert!(
                !normalized.contains('\n'),
                "Normalized entry must not contain newlines"
            );
            assert!(
                !normalized.contains('\t'),
                "Normalized entry must not contain tabs"
            );
            assert!(
                !normalized.contains("  "),
                "Normalized entry must not contain double spaces"
            );
            assert_eq!(
                normalized,
                "Implemented database pooling with health checks"
            );
        }

        /// Regression: `normalize_summary_entry` collapses multiline and still caps length.
        #[test]
        fn normalize_summary_entry_collapses_and_truncates() {
            let long_multiline = format!("Line one\n{}", "X".repeat(600));
            let result = normalize_summary_entry(&long_multiline);
            assert!(result.is_some());
            let normalized = result.unwrap();
            assert!(
                !normalized.contains('\n'),
                "Normalized entry must not contain newlines"
            );
            assert!(
                normalized.len() <= MAX_ENTRY_LENGTH,
                "Should be truncated to {MAX_ENTRY_LENGTH} chars, got {}",
                normalized.len()
            );
        }

        /// Regression: `normalize_summary_entry` rejects multiline input containing paths.
        #[test]
        fn normalize_summary_entry_rejects_multiline_with_paths() {
            let multiline_with_path =
                "Updated executor\nModified src/core/runner.rs\nto add retry logic";
            assert!(
                normalize_summary_entry(multiline_with_path).is_none(),
                "Multiline input containing path references should be rejected"
            );
        }
    }
}
