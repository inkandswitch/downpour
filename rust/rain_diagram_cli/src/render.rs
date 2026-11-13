//! Output formats, and messages that point into a source file.
//!
//! ```
//! use rain_diagram_cli::render::Format;
//!
//! let diagram = rain_diagram_core::parse("rain\n(Doc)-[edit(Doc)]->(Alice)")?;
//! let svg = Format::Svg.render(&diagram, None)?;
//! assert!(svg.starts_with("<svg"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use rain_diagram_core::{Error, ast::Kind, authority::Diagnostic, diagram::Diagram};
use std::{fmt, path::Path};
use thiserror::Error;

/// What to render a diagram as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    /// Unicode text art.
    #[default]
    Text,

    /// SVG in the notation the header names.
    Svg,

    /// SVG as a rain diagram, whatever the header says.
    RainSvg,

    /// SVG as a certflow diagram, whatever the header says.
    CertflowSvg,

    /// The JSON the Keyhive Lab Notes' renderer reads.
    Json,
}

impl Format {
    /// The format an output file's extension implies: `.svg`, `.json`, or
    /// `.txt`.
    ///
    /// ```
    /// use rain_diagram_cli::render::Format;
    /// use std::path::Path;
    ///
    /// assert_eq!(Format::for_path(Path::new("out/doc.svg")), Some(Format::Svg));
    /// assert_eq!(Format::for_path(Path::new("doc.md")), None);
    /// ```
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "json" => Some(Self::Json),
            "svg" => Some(Self::Svg),
            "txt" => Some(Self::Text),
            _ => None,
        }
    }

    /// Render `diagram`, as of `at` events if given (SVG only).
    ///
    /// # Errors
    ///
    /// [`RenderError::AtNeedsSvg`] if `at` is given for text or JSON, and
    /// [`RenderError::Json`] if JSON serialization fails.
    pub fn render(self, diagram: &Diagram, at: Option<usize>) -> Result<String, RenderError> {
        let kind = match self {
            Self::Svg => diagram.kind(),
            Self::RainSvg => Kind::Rain,
            Self::CertflowSvg => Kind::Certflow,
            Self::Text | Self::Json if at.is_some() => return Err(RenderError::AtNeedsSvg),
            Self::Text => return Ok(diagram.to_text()),
            Self::Json => return Ok(diagram.to_site_json()?),
        };
        let happened = at.unwrap_or(diagram.events().len());
        Ok(match kind {
            Kind::Rain => diagram.to_rain_svg_at(happened),
            Kind::Certflow => diagram.to_certflow_svg_at(happened),
        })
    }
}

/// A problem at a place in a named source: `path:line:column: severity:
/// message`, the shape editors and CI annotate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file (or chapter) it's in.
    pub path: String,

    /// 1-based line.
    pub line: usize,

    /// 1-based column, in characters.
    pub column: usize,

    /// Whether it stops the diagram rendering.
    pub severity: Severity,

    /// What's wrong.
    pub message: String,
}

impl Problem {
    /// A parse or resolve error in `source`.
    #[must_use]
    pub fn error(path: &str, source: &str, error: &Error) -> Self {
        let (line, column) = error.span().line_col(source);
        Self {
            path: path.into(),
            line,
            column,
            severity: Severity::Error,
            message: error.to_string(),
        }
    }

    /// A warning about a diagram parsed from `source`.
    #[must_use]
    pub fn warning(path: &str, source: &str, diagnostic: &Diagnostic) -> Self {
        let (line, column) = diagnostic.span().line_col(source);
        Self {
            path: path.into(),
            line,
            column,
            severity: Severity::Warning,
            message: diagnostic.to_string(),
        }
    }

    /// Every problem with `source`: its error, or its diagram's warnings.
    #[must_use]
    pub fn all(path: &str, source: &str) -> (Option<Diagram>, Vec<Self>) {
        parse_as(path, source, None)
    }
}

/// The notation a file name says: `.rain` or `.certflow`.
///
/// ```
/// use rain_diagram_cli::render::notation_for;
/// use rain_diagram_core::ast::Kind;
/// use std::path::Path;
///
/// assert_eq!(notation_for(Path::new("threads.certflow")), Some(Kind::Certflow));
/// assert_eq!(notation_for(Path::new("notes.txt")), None);
/// ```
#[must_use]
pub fn notation_for(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()? {
        "rain" => Some(Kind::Rain),
        "certflow" => Some(Kind::Certflow),
        _ => None,
    }
}

/// Parse `source` as `notation` (named by its file extension or code
/// fence), with every problem: the header is supplied if the source leaves
/// it out, a header naming the other notation is a warning, and lines are
/// counted in `source` as written.
///
/// ```
/// use rain_diagram_cli::render::parse_as;
/// use rain_diagram_core::ast::Kind;
///
/// let (diagram, problems) = parse_as("doc.rain", "(Doc)-[edit(Doc)]->(Alice)", Some(Kind::Rain));
/// assert!(diagram.is_some() && problems.is_empty());
/// ```
#[must_use]
pub fn parse_as(
    path: &str,
    source: &str,
    notation: Option<Kind>,
) -> (Option<Diagram>, Vec<Problem>) {
    let written = header(source);
    let supplied = match (notation, written) {
        (Some(kind), None) => Some(format!("{}\n{source}", keyword(kind))),
        _ => None,
    };
    let text = supplied.as_deref().unwrap_or(source);

    let (diagram, mut problems) = match rain_diagram_core::parse(text) {
        Ok(diagram) => {
            let warnings = diagram
                .diagnostics()
                .iter()
                .map(|d| Problem::warning(path, text, d))
                .collect();
            (Some(diagram), warnings)
        }
        Err(e) => (None, vec![Problem::error(path, text, &e)]),
    };
    if supplied.is_some() {
        for p in &mut problems {
            p.line -= 1;
        }
    }

    if let (Some(kind), Some((line, said))) = (notation, written)
        && said != keyword(kind)
    {
        problems.insert(
            0,
            Problem {
                path: path.into(),
                line,
                column: 1,
                severity: Severity::Warning,
                message: format!(
                    "this says {said}, but it's named as {}; the header wins",
                    keyword(kind)
                ),
            },
        );
    }
    (diagram, problems)
}

/// The header `source` starts with (`rain` or `certflow`), and its line:
/// the first line that isn't blank or a comment.
#[must_use]
pub fn header(source: &str) -> Option<(usize, &str)> {
    let (index, line) = source.lines().enumerate().find(|(_, l)| {
        let l = l.trim().trim_start_matches('\u{feff}');
        !l.is_empty() && !l.starts_with("//") && !l.starts_with("%%")
    })?;
    let word = line
        .split_whitespace()
        .next()?
        .trim_start_matches('\u{feff}');
    ["rain", "certflow"]
        .into_iter()
        .find(|k| *k == word)
        .map(|k| (index + 1, k))
}

/// How a notation is written in a header, extension, or fence.
const fn keyword(kind: Kind) -> &'static str {
    match kind {
        Kind::Rain => "rain",
        Kind::Certflow => "certflow",
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}: {}",
            self.path, self.line, self.column, self.severity, self.message
        )
    }
}

/// How bad a [`Problem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The diagram resolves but doesn't do what it looks like it does.
    Warning,

    /// The diagram doesn't parse or resolve.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

/// Why a diagram couldn't be rendered in a format.
#[derive(Debug, Error)]
pub enum RenderError {
    /// `--at` picks a moment to draw, which only SVGs show.
    #[error("--at only applies to SVG output")]
    AtNeedsSvg,

    /// JSON serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use testresult::TestResult;

    #[test]
    fn problems_point_at_their_line() {
        let (diagram, problems) = Problem::all("doc.rain", "rain\n(A)-[read]->(B)");
        assert!(diagram.is_none());
        assert_eq!(
            problems.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["doc.rain:2:10: error: expected the subject after the ability, as in `[edit(Doc)]`"]
        );
    }

    #[test]
    fn warnings_come_with_the_diagram() {
        let (diagram, problems) = Problem::all("d", "rain\n(A)-[edit(Doc)]->(B)");
        assert!(diagram.is_some());
        assert_eq!(problems.len(), 1);
        assert_eq!(
            problems.first().map(|p| p.severity),
            Some(Severity::Warning)
        );
    }

    #[test]
    fn extensions_supply_and_check_the_header() {
        let (d, problems) = parse_as(
            "a.certflow",
            "// x\n(A)-[read(B)]->(C)",
            Some(Kind::Certflow),
        );
        assert_eq!(d.map(|d| d.kind()), Some(Kind::Certflow));
        // Lines as written: the supplied header doesn't shift them
        assert_eq!(problems.first().map(|p| p.line), Some(2));

        let (d, problems) = parse_as("a.rain", "certflow\n(A)-[read(A)]->(B)", Some(Kind::Rain));
        assert_eq!(d.map(|d| d.kind()), Some(Kind::Certflow));
        assert_eq!(
            problems.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["a.rain:1:1: warning: this says certflow, but it's named as rain; the header wins"]
        );

        let (_, problems) = parse_as("a.rain", "(A)-[read]->(B)", Some(Kind::Rain));
        assert_eq!(problems.first().map(|p| (p.line, p.column)), Some((1, 10)));
    }

    #[test]
    fn at_only_draws_moments_in_svg() -> TestResult {
        let d = rain_diagram_core::parse("rain\n(Doc)-[edit(Doc)]->(A)\n(A)-[read(Doc)]->(B)")?;
        assert!(matches!(
            Format::Text.render(&d, Some(1)),
            Err(RenderError::AtNeedsSvg)
        ));
        assert_ne!(
            Format::Svg.render(&d, Some(1))?,
            Format::Svg.render(&d, None)?
        );
        assert!(Format::CertflowSvg.render(&d, None)?.contains("certflow"));
        Ok(())
    }
}
