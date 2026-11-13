//! `mdbook-rain-diagram`: an mdBook preprocessor that renders ` ```rain `
//! and ` ```certflow ` blocks. Add it to `book.toml`:
//!
//! ```toml
//! [preprocessor.rain-diagram]
//! ```
//!
//! See [`rain_diagram_cli::mdbook`] for the details.

use rain_diagram_cli::mdbook;
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("supports"), Some(renderer)) => {
            return if mdbook::supports(&renderer) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        (None, _) => {}
        (Some(other), _) => {
            eprintln!(
                "mdbook-rain-diagram: unexpected argument {other:?}; mdBook runs this as a preprocessor, with `supports <renderer>` or the book on stdin"
            );
            return ExitCode::from(2);
        }
    }

    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("mdbook-rain-diagram: can't read the book: {e}");
        return ExitCode::FAILURE;
    }
    match mdbook::run(&input) {
        Ok((book, problems)) => {
            for p in problems {
                eprintln!("{p}");
            }
            if let Err(e) = io::stdout().write_all(book.as_bytes()) {
                eprintln!("mdbook-rain-diagram: can't write the book: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
