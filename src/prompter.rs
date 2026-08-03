//! A seam for interactive prompts.
//!
//! The README says: "When operating on a `task_name`, the application will
//! try to match the name - if it encounters the same name in multiple
//! categories, it will prompt the user for which item on which to operate."
//! An earlier attempt wired that up with a bare inline
//! `std::io::stdin().read_line(...)` call, which made it impossible to write
//! a test that exercises the disambiguation flow without a real terminal
//! attached - it was rejected for exactly this reason.
//!
//! `Prompter` factors "ask the user something" behind a trait, so:
//!   - production code uses `StdinPrompter`, which reads real input but
//!     detects the non-interactive case (piped/redirected stdin, CI, no TTY)
//!     up front and returns a typed `PromptError` instead of blocking
//!     forever on a `read_line` that will never receive input;
//!   - `--yes` / `--no-input` swap in `NonInteractivePrompter`, which answers
//!     confirmations from a fixed policy and never reads stdin at all;
//!   - tests use `ScriptedPrompter`, which replays a pre-programmed sequence
//!     of answers with no terminal involved at all.
//!
//! Two questions are modelled, both deliberately generic rather than
//! feature-specific:
//!   - `choose`, "pick one of these labelled options" (the ambiguous-task
//!     flow above, and the deferred `reset` command);
//!   - `confirm`, "yes or no?" (the first-run offer to create the default
//!     "Home"/"Work" categories).

use std::io::{self, BufRead, IsTerminal, Write};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PromptError {
    /// No interactive terminal is attached to stdin (piped input, CI, a
    /// non-interactive script, ...). Rather than block forever on a
    /// `read_line` nobody will ever answer, callers surface this as a clean
    /// instruction to disambiguate a different way (e.g. `--category`).
    ///
    /// The message below is worded for `choose`, because that is the only
    /// caller that reports it to the user: the first-run `confirm` treats it
    /// as "nobody was there to answer" and silently skips the offer instead
    /// (see `main::offer_default_categories`).
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

/// Something that can ask a user a question and report the answer.
pub trait Prompter {
    /// Presents a list of labeled options and returns which one the user
    /// picked, as a 0-based index into `options`.
    fn choose(&mut self, message: &str, options: &[String]) -> Result<usize, PromptError>;

    /// Asks a yes/no question. `default_yes` is the answer an empty response
    /// (a bare Enter) means, and is reflected in the `[Y/n]` / `[y/N]` hint
    /// the interactive implementation prints - so the caller decides which
    /// way the prompt leans rather than each implementation hardcoding it.
    fn confirm(&mut self, question: &str, default_yes: bool) -> Result<bool, PromptError>;
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

/// Interprets a raw line of input as a yes/no answer, returning `None` for
/// anything unrecognized so the caller can re-ask. An empty line (a bare
/// Enter) means the offered default, which is what makes the first-run
/// prompt "default to yes". Factored out for the same reason as
/// `parse_selection`: it is the interesting logic, and it is unit-testable
/// without a terminal.
fn parse_confirmation(line: &str, default_yes: bool) -> Option<bool> {
    match line.trim().to_ascii_lowercase().as_str() {
        "" => Some(default_yes),
        "y" | "yes" => Some(true),
        "n" | "no" => Some(false),
        _ => None,
    }
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

    fn confirm(&mut self, question: &str, default_yes: bool) -> Result<bool, PromptError> {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            return Err(PromptError::NotInteractive);
        }

        let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
        loop {
            print!("{question} {hint} ");
            let _ = io::stdout().flush();

            let mut line = String::new();
            let bytes_read = stdin
                .lock()
                .read_line(&mut line)
                .map_err(|_| PromptError::Eof)?;
            if bytes_read == 0 {
                return Err(PromptError::Eof);
            }

            // Unlike `choose`, an unrecognized answer here is not fatal: the
            // question is cheap to repeat and there is no numbered list the
            // user has to re-read, so re-asking beats aborting whatever
            // command they actually ran.
            if let Some(answer) = parse_confirmation(&line, default_yes) {
                return Ok(answer);
            }
            println!("Please answer 'y' or 'n'.");
        }
    }
}

/// A prompter that never touches stdin, backing the global `--yes` and
/// `--no-input` flags: every confirmation is answered with a fixed policy,
/// so automation and CI can drive the binary without a terminal and without
/// ever blocking.
///
/// `choose` deliberately still reports `NotInteractive`: "assume yes" has no
/// meaningful answer to "which of these three tasks did you mean?", and
/// silently picking one would operate on a task the user never named. Both
/// flags therefore mean the same thing for `choose` - identify the item
/// unambiguously instead.
pub struct NonInteractivePrompter {
    answer: bool,
}

impl NonInteractivePrompter {
    /// `--yes`: take the offer without asking.
    pub fn assuming_yes() -> Self {
        Self { answer: true }
    }

    /// `--no-input`: decline without asking. This is a real answer, not an
    /// absent one - a declined first-run offer is recorded and never made
    /// again (see `main::offer_default_categories`).
    pub fn assuming_no() -> Self {
        Self { answer: false }
    }
}

impl Prompter for NonInteractivePrompter {
    fn choose(&mut self, _message: &str, _options: &[String]) -> Result<usize, PromptError> {
        Err(PromptError::NotInteractive)
    }

    fn confirm(&mut self, _question: &str, _default_yes: bool) -> Result<bool, PromptError> {
        Ok(self.answer)
    }
}

/// Test double that answers with a pre-scripted sequence of results,
/// exercising the exact same `Prompter` seam production code uses - no
/// terminal, no stdin, no flakiness, and no risk of a hung test process.
///
/// The two question kinds get separate queues: a test that scripts a
/// disambiguation choice should not have to know whether some unrelated
/// confirmation happened to be asked first.
#[cfg(test)]
pub struct ScriptedPrompter {
    answers: std::collections::VecDeque<Result<usize, PromptError>>,
    confirmations: std::collections::VecDeque<Result<bool, PromptError>>,
}

#[cfg(test)]
impl ScriptedPrompter {
    pub fn new(answers: Vec<Result<usize, PromptError>>) -> Self {
        Self {
            answers: answers.into(),
            confirmations: std::collections::VecDeque::new(),
        }
    }

    pub fn with_confirmations(confirmations: Vec<Result<bool, PromptError>>) -> Self {
        Self {
            answers: std::collections::VecDeque::new(),
            confirmations: confirmations.into(),
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

    fn confirm(&mut self, _question: &str, _default_yes: bool) -> Result<bool, PromptError> {
        self.confirmations
            .pop_front()
            .unwrap_or(Err(PromptError::Eof))
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

    /// A bare Enter takes whichever default the caller offered - this is what
    /// makes the first-run offer "default to yes".
    #[test]
    fn parse_confirmation_treats_an_empty_line_as_the_offered_default() {
        assert_eq!(parse_confirmation("\n", true), Some(true));
        assert_eq!(parse_confirmation("   \n", true), Some(true));
        assert_eq!(parse_confirmation("", false), Some(false));
    }

    #[test]
    fn parse_confirmation_accepts_both_spellings_in_either_case() {
        for yes in ["y", "Y", "yes", "YES", " Yes \n"] {
            assert_eq!(parse_confirmation(yes, false), Some(true), "{yes:?}");
        }
        for no in ["n", "N", "no", "NO", " No \n"] {
            assert_eq!(parse_confirmation(no, true), Some(false), "{no:?}");
        }
    }

    /// `None` means "ask again" rather than "take the default": silently
    /// creating categories because the user typo'd would be a surprise.
    #[test]
    fn parse_confirmation_rejects_anything_else() {
        assert_eq!(parse_confirmation("maybe", true), None);
        assert_eq!(parse_confirmation("1", true), None);
    }

    /// `--yes` / `--no-input` answer confirmations from a fixed policy and
    /// never read stdin, but must not invent an answer to `choose`.
    #[test]
    fn non_interactive_prompter_answers_confirmations_but_not_choices() {
        let options = vec!["a".to_string(), "b".to_string()];

        let mut yes = NonInteractivePrompter::assuming_yes();
        assert_eq!(yes.confirm("go?", true), Ok(true));
        // The offered default is ignored - the flag is the answer.
        assert_eq!(yes.confirm("go?", false), Ok(true));
        assert_eq!(
            yes.choose("pick", &options),
            Err(PromptError::NotInteractive)
        );

        let mut no = NonInteractivePrompter::assuming_no();
        assert_eq!(no.confirm("go?", true), Ok(false));
        assert_eq!(
            no.choose("pick", &options),
            Err(PromptError::NotInteractive)
        );
    }

    #[test]
    fn scripted_prompter_replays_confirmations_from_their_own_queue() {
        let mut prompter = ScriptedPrompter::with_confirmations(vec![Ok(true), Ok(false)]);
        assert_eq!(prompter.confirm("go?", true), Ok(true));
        assert_eq!(prompter.confirm("go?", true), Ok(false));
        // Exhausted, like `choose`.
        assert_eq!(prompter.confirm("go?", true), Err(PromptError::Eof));
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
