// src/ui/multiline_helper.rs
//! Custom helper for multi-line input support

use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Helper};

#[derive(Clone)]
pub struct MultilineHelper;

impl Completer for MultilineHelper {
    type Candidate = String;
}

impl Hinter for MultilineHelper {
    type Hint = String;
}

impl Highlighter for MultilineHelper {}

impl Validator for MultilineHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> Result<ValidationResult, ReadlineError> {
        let input = ctx.input();

        // If ending with \ (and not escaped \\), continue input (multiline)
        if input.ends_with('\\') && !input.ends_with("\\\\") {
            Ok(ValidationResult::Incomplete)
        } else {
            Ok(ValidationResult::Valid(None))
        }
    }

    fn validate_while_typing(&self) -> bool {
        false  // Only validate on Enter press
    }
}

impl Helper for MultilineHelper {}
