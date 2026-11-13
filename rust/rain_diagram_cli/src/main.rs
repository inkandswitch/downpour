//! `rain-diagram`: render certflow and rain diagrams, or check them.
//!
//! ```console
//! $ rain-diagram render doc.rain                 # text art to stdout
//! $ rain-diagram render doc.rain -o doc.svg      # format from the extension
//! $ rain-diagram render --format certflow-svg --at 3 < doc.rain
//! $ rain-diagram check diagrams/*.rain --deny-warnings
//! ```
//!
//! A `.rain` or `.certflow` file can leave out its header line; the
//! extension supplies it. Problems are printed as `path:line:column: error: message`. Exit status:
//! 0 on success, 1 if a diagram has errors (or warnings, with
//! `--deny-warnings`) or a file can't be read or written, 2 for bad
//! arguments.

use clap::{Parser, Subcommand};
use rain_diagram_cli::render::{Format, RenderError, Severity, notation_for, parse_as};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
use thiserror::Error;

/// Render certflow and rain diagrams, or check them for problems.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Render a diagram to text art, SVG, or JSON
    Render {
        /// The diagram source (`-` or nothing for stdin)
        input: Option<PathBuf>,

        /// The output format (default: from the output file's extension,
        /// else text)
        #[arg(short, long, value_enum)]
        format: Option<Format>,

        /// Draw the SVG as of this many events
        #[arg(long, value_name = "EVENTS")]
        at: Option<usize>,

        /// Where to write it (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Check diagrams, printing every error and warning
    Check {
        /// The diagram sources (`-` for stdin)
        #[arg(required = true)]
        inputs: Vec<PathBuf>,

        /// Fail on warnings too
        #[arg(long)]
        deny_warnings: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rain-diagram: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, CliError> {
    match cli.command {
        Command::Render {
            input,
            format,
            at,
            output,
        } => {
            let (name, source) = read(input.as_deref())?;
            let notation = input.as_deref().and_then(notation_for);
            let (diagram, problems) = parse_as(&name, &source, notation);
            report(&problems);
            let Some(diagram) = diagram else {
                return Ok(ExitCode::FAILURE);
            };
            let format = format
                .or_else(|| output.as_deref().and_then(Format::for_path))
                .unwrap_or_default();
            let mut rendered = format.render(&diagram, at)?;
            if !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            match output {
                Some(path) => std::fs::write(&path, rendered)
                    .map_err(|source| CliError::Write { path, source })?,
                None => io::stdout()
                    .write_all(rendered.as_bytes())
                    .map_err(|source| CliError::Write {
                        path: "<stdout>".into(),
                        source,
                    })?,
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Check {
            inputs,
            deny_warnings,
        } => {
            let mut worst = None;
            for input in &inputs {
                let (name, source) = read(Some(input))?;
                let (_, problems) = parse_as(&name, &source, notation_for(input));
                report(&problems);
                worst = worst.max(problems.iter().map(|p| p.severity).max());
            }
            let failed = match worst {
                Some(Severity::Error) => true,
                Some(Severity::Warning) => deny_warnings,
                None => false,
            };
            Ok(if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

/// A source and the name to report it by.
fn read(input: Option<&Path>) -> Result<(String, String), CliError> {
    match input {
        None => read_stdin(),
        Some(path) if path == Path::new("-") => read_stdin(),
        Some(path) => {
            let source = std::fs::read_to_string(path).map_err(|source| CliError::Read {
                path: path.to_owned(),
                source,
            })?;
            Ok((path.display().to_string(), source))
        }
    }
}

fn read_stdin() -> Result<(String, String), CliError> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|source| CliError::Read {
            path: "<stdin>".into(),
            source,
        })?;
    Ok(("<stdin>".into(), source))
}

fn report(problems: &[rain_diagram_cli::render::Problem]) {
    for p in problems {
        eprintln!("{p}");
    }
}

#[derive(Debug, Error)]
enum CliError {
    #[error("can't read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },

    #[error("can't write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },

    #[error(transparent)]
    Render(#[from] RenderError),
}
