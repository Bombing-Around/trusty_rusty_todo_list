//! A seam for interactive "which one did you mean?" prompts.
//!
//! The README says: "When operating on a `task_name`, the application will
//! try to match the name - if it encounters the same name in multiple
//! categories, it will prompt the user for which item on which to operate."
//! A prior attempt at this (PR #15) wired that up with a bare inline
//! `std::io::stdin().read_line(...)` call, which made it impossible to write
//! a test that exercises the disambiguation flow without a real terminal
//! attached - that PR was rejected for exactly this reason.
//!
//! `Prompter` factors the "ask the user to choose one of several options"
//! step behind a trait, so:
//!   - production code uses `StdinPrompter`, which reads real input but
//!     detects the non-interactive case (piped/redirected stdin, CI, no TTY)
//!     up front and returns a typed `PromptError` instead of blocking
//!     forever on a `read_line` that will never receive input;
//!   - tests use `ScriptedPrompter`, which replays a pre-programmed sequence
//!     of answers with no terminal involved at all.
//!
//! This is deliberately generic (`choose` over a list of string labels)
//! rather than task-specific: issue #27 and the deferred `reset` command are
//! expected to need this exact same "pick one of several matches" flow, so
//! it is built once here rather than re-invented per feature.

use std::io::{self, BufRead, IsTerminal, Write};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PromptError {
    /// No interactive terminal is attached to stdin (piped input, CI, a
    /// non-interactive script, ...). Rather than block forever on a
    /// `read_line` nobody will ever answer, callers surface this as a clean
    /// instruction to disambiguate a different way (e.g. `--category`).
    #[error(
        "multiple matches were found and no terminal is attached to ask which one; \
         re-run interactively, or identify the item unambiguously (by ID, or with \
         --category) instead of by name"
    )]
    NotInteractive,
    #[error("input closed before a selection was made")]
    Eof,
    #[error("'{0}' is not a valid selection")]
    InvalidSelection(String),
}

/// Something that can present a list of labeled options to a user and
/// return which one they picked (as a 0-based index into `options`).
pub trait Prompter {
    fn choose(&mut self, message: &str, options: &[String]) -> Result<usize, PromptError>;
}

/// Parses and range-checks a raw line of input against `option_count`.
/// Factored out of `StdinPrompter::choose` so the parsing/validation logic
/// is unit-testable without needing to fake stdin.
fn parse_selection(line: &str, option_count: usize) -> Result<usize, PromptError> {
    let trimmed = line.trim();
    let choice: usize = trimmed
        .parse()
        .map_err(|_| PromptError::InvalidSelection(trimmed.to_string()))?;
    if choice == 0 || choice > option_count {
        return Err(PromptError::InvalidSelection(trimmed.to_string()));
    }
    Ok(choice - 1)
}

/// The real, interactive prompter used by the CLI binary.
pub struct StdinPrompter;

impl Prompter for StdinPrompter {
    fn choose(&mut self, message: &str, options: &[String]) -> Result<usize, PromptError> {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            return Err(PromptError::NotInteractive);
        }

        println!("{message}");
        for (i, option) in options.iter().enumerate() {
            println!("  {}: {}", i + 1, option);
        }
        print!("Enter a number: ");
        let _ = io::stdout().flush();

        let mut line = String::new();
        let bytes_read = stdin
            .lock()
            .read_line(&mut line)
            .map_err(|_| PromptError::Eof)?;
        if bytes_read == 0 {
            return Err(PromptError::Eof);
        }

        parse_selection(&line, options.len())
    }
}

/// Test double that answers with a pre-scripted sequence of results,
/// exercising the exact same `Prompter` seam production code uses - no
/// terminal, no stdin, no flakiness, and no risk of a hung test process.
#[cfg(test)]
pub struct ScriptedPrompter {
    answers: std::collections::VecDeque<Result<usize, PromptError>>,
}

#[cfg(test)]
impl ScriptedPrompter {
    pub fn new(answers: Vec<Result<usize, PromptError>>) -> Self {
        Self {
            answers: answers.into(),
        }
    }
}

#[cfg(test)]
impl Prompter for ScriptedPrompter {
    fn choose(&mut self, _message: &str, options: &[String]) -> Result<usize, PromptError> {
        match self.answers.pop_front() {
            Some(Ok(idx)) if idx < options.len() => Ok(idx),
            Some(Ok(idx)) => Err(PromptError::InvalidSelection(idx.to_string())),
            Some(Err(e)) => Err(e),
            None => Err(PromptError::Eof),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selection_accepts_in_range_one_indexed_input() {
        assert_eq!(parse_selection("1\n", 3), Ok(0));
        assert_eq!(parse_selection(" 3 ", 3), Ok(2));
    }

    #[test]
    fn parse_selection_rejects_zero_out_of_range_and_garbage() {
        assert_eq!(
            parse_selection("0", 3),
            Err(PromptError::InvalidSelection("0".into()))
        );
        assert_eq!(
            parse_selection("4", 3),
            Err(PromptError::InvalidSelection("4".into()))
        );
        assert_eq!(
            parse_selection("nope", 3),
            Err(PromptError::InvalidSelection("nope".into()))
        );
    }

    #[test]
    fn scripted_prompter_replays_answers_in_order() {
        let mut prompter = ScriptedPrompter::new(vec![Ok(1), Ok(0)]);
        let options = vec!["a".to_string(), "b".to_string()];
        assert_eq!(prompter.choose("pick", &options), Ok(1));
        assert_eq!(prompter.choose("pick", &options), Ok(0));
    }

    #[test]
    fn scripted_prompter_reports_eof_when_script_is_exhausted() {
        let mut prompter = ScriptedPrompter::new(vec![]);
        let options = vec!["a".to_string()];
        assert_eq!(prompter.choose("pick", &options), Err(PromptError::Eof));
    }

    #[test]
    fn scripted_prompter_can_replay_a_scripted_error() {
        let mut prompter = ScriptedPrompter::new(vec![Err(PromptError::NotInteractive)]);
        let options = vec!["a".to_string()];
        assert_eq!(
            prompter.choose("pick", &options),
            Err(PromptError::NotInteractive)
        );
    }
}
