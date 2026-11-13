//! An [mdBook preprocessor]: code blocks fenced as `rain` or `certflow`
//! become diagrams.
//!
//! ````markdown
//! ```rain
//! (Doc)-[edit(Doc)]->(Alice)
//! (Alice)-[read(Doc)]->(Bob)
//! ```
//! ````
//!
//! The fence names the notation, so the block can leave out the `rain` or
//! `certflow` header (a block that starts with one keeps it). For the HTML
//! renderer each block becomes an inline SVG; for any other renderer, text
//! art in a plain code block. Every other block, and all other text, is
//! left exactly as it was.
//!
//! A block that doesn't parse fails the build, naming the chapter, line,
//! and column. Set `on-error = "warn"` to print a warning and show the
//! error in the page instead:
//!
//! ```toml
//! [preprocessor.rain-diagram]
//! on-error = "warn"
//! ```
//!
//! [mdBook preprocessor]: https://rust-lang.github.io/mdBook/for_developers/preprocessors.html

use crate::render::{Problem, Severity, parse_as};
use rain_diagram_core::ast::Kind;
use serde_json::Value;
use std::fmt;
use thiserror::Error;

/// Run the preprocessor on mdBook's input (`[context, book]` as JSON),
/// returning the book to print and the problems to report.
///
/// # Errors
///
/// [`PreprocessError`] if the input isn't what mdBook sends, the options
/// are wrong, or (unless `on-error = "warn"`) a diagram doesn't parse.
pub fn run(input: &str) -> Result<(String, Vec<Problem>), PreprocessError> {
    let Value::Array(mut parts) = serde_json::from_str(input)? else {
        return Err(PreprocessError::Shape);
    };
    let (Some(mut book), Some(context)) = (parts.pop(), parts.pop()) else {
        return Err(PreprocessError::Shape);
    };
    let options = Options::from_context(&context)?;
    let html = context.get("renderer").and_then(Value::as_str) == Some("html");

    let mut problems = Vec::new();
    chapters(&mut book, &mut |path, content| {
        let (out, found) = markdown(content, path, html, options.on_error);
        *content = out;
        problems.extend(found);
    });

    if options.on_error == OnError::Fail && problems.iter().any(|p| p.severity == Severity::Error) {
        return Err(PreprocessError::Diagrams(Problems(problems)));
    }
    Ok((serde_json::to_string(&book)?, problems))
}

/// Whether the preprocessor works with `renderer`: every renderer, since
/// anything but HTML gets text art.
#[must_use]
pub const fn supports(_renderer: &str) -> bool {
    true
}

/// Replace the diagram blocks in one chapter's Markdown.
#[must_use]
pub fn markdown(
    content: &str,
    path: &str,
    html: bool,
    on_error: OnError,
) -> (String, Vec<Problem>) {
    let mut out = String::with_capacity(content.len());
    let mut problems = Vec::new();
    let mut open: Option<Fence<'_>> = None;
    let mut body = String::new();
    let mut raw = String::new();

    for (index, line) in content.split_inclusive('\n').enumerate() {
        match &open {
            None => match Fence::open(line) {
                Some(fence) if fence.notation.is_some() => {
                    open = Some(Fence {
                        line: index + 1,
                        ..fence
                    });
                    raw.push_str(line);
                }
                Some(fence) => {
                    open = Some(Fence {
                        notation: None,
                        ..fence
                    });
                    out.push_str(line);
                }
                None => out.push_str(line),
            },
            Some(fence) if fence.closed_by(line) => {
                match fence.notation {
                    Some(notation) => {
                        let (block, found) =
                            diagram(notation, &body, path, fence.line, html, on_error);
                        out.push_str(&block);
                        problems.extend(found);
                    }
                    None => out.push_str(line),
                }
                open = None;
                body.clear();
                raw.clear();
            }
            Some(fence) => {
                if fence.notation.is_some() {
                    body.push_str(line);
                    raw.push_str(line);
                } else {
                    out.push_str(line);
                }
            }
        }
    }
    // An unclosed diagram fence runs to the end, as in CommonMark; leave it
    // as written rather than guess
    out.push_str(&raw);
    (out, problems)
}

/// One diagram block, rendered, with its problems located in the chapter.
fn diagram(
    notation: &str,
    body: &str,
    path: &str,
    fence_line: usize,
    html: bool,
    on_error: OnError,
) -> (String, Vec<Problem>) {
    let kind = match notation {
        "certflow" => Kind::Certflow,
        _ => Kind::Rain,
    };
    let (diagram, mut problems) = parse_as(path, body, Some(kind));
    for p in &mut problems {
        p.line += fence_line;
    }

    let block = match (diagram, html) {
        (Some(d), true) => {
            // A blank line would end Markdown's HTML block early
            let svg = d.to_svg();
            let svg: Vec<&str> = svg.lines().filter(|l| !l.trim().is_empty()).collect();
            // Scroll rather than shrink when wider than the page
            format!(
                "<div class=\"rain-diagram\" style=\"overflow-x: auto\">\n{}\n</div>\n\n",
                svg.join("\n")
            )
        }
        (Some(d), false) => format!("```text\n{}```\n", d.to_text()),
        (None, _) => {
            let message = problems
                .first()
                .map(ToString::to_string)
                .unwrap_or_default();
            match (on_error, html) {
                (OnError::Fail, _) => String::new(),
                (OnError::Warn, true) => format!(
                    "<pre class=\"rain-diagram-error\">{}</pre>\n\n",
                    escape(&message)
                ),
                (OnError::Warn, false) => format!("```text\n{message}\n```\n"),
            }
        }
    };
    (block, problems)
}

/// An opening code fence: its character and length, and the notation if
/// its info string names one.
#[derive(Debug, Clone, Copy)]
struct Fence<'a> {
    char: char,
    len: usize,
    notation: Option<&'a str>,
    line: usize,
}

impl<'a> Fence<'a> {
    fn open(line: &'a str) -> Option<Self> {
        let rest = indented(line)?;
        let char = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
        let len = rest.chars().take_while(|&c| c == char).count();
        if len < 3 {
            return None;
        }
        let info = rest.get(len..)?.trim();
        // A backtick fence's info string can't hold backticks
        if char == '`' && info.contains('`') {
            return None;
        }
        let notation = info
            .split_whitespace()
            .next()
            .filter(|w| matches!(*w, "rain" | "certflow"));
        Some(Self {
            char,
            len,
            notation,
            line: 0,
        })
    }

    fn closed_by(&self, line: &str) -> bool {
        indented(line).is_some_and(|rest| {
            let len = rest.chars().take_while(|&c| c == self.char).count();
            len >= self.len && rest.get(len..).is_some_and(|tail| tail.trim().is_empty())
        })
    }
}

/// The line without up to three spaces of indent (four makes it code).
fn indented(line: &str) -> Option<&str> {
    let spaces = line.chars().take_while(|&c| c == ' ').count();
    (spaces <= 3).then(|| line.get(spaces..)).flatten()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Every chapter's Markdown in mdBook's book JSON, with its path. Walks the
/// JSON rather than a typed book, so it works with mdBook 0.4 (`sections`)
/// and 0.5 (`items`) alike.
fn chapters(value: &mut Value, f: &mut impl FnMut(&str, &mut String)) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Object(chapter)) = map.get_mut("Chapter") {
                let path = ["source_path", "path", "name"]
                    .iter()
                    .find_map(|k| chapter.get(*k).and_then(Value::as_str))
                    .unwrap_or("chapter")
                    .to_owned();
                if let Some(Value::String(content)) = chapter.get_mut("content") {
                    f(&path, content);
                }
            }
            for child in map.values_mut() {
                chapters(child, f);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| chapters(item, f)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// `[preprocessor.rain-diagram]` in `book.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// What a diagram that doesn't parse does.
    pub on_error: OnError,
}

impl Options {
    fn from_context(context: &Value) -> Result<Self, PreprocessError> {
        let on_error = context
            .pointer("/config/preprocessor/rain-diagram/on-error")
            .map(|v| match v.as_str() {
                Some("fail") => Ok(OnError::Fail),
                Some("warn") => Ok(OnError::Warn),
                _ => Err(PreprocessError::OnError(v.to_string())),
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Self { on_error })
    }
}

/// What a diagram that doesn't parse does to the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnError {
    /// Fail the build, listing every broken diagram.
    #[default]
    Fail,

    /// Warn, and show the error in the page where the diagram would be.
    Warn,
}

/// Every problem found, one per line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problems(Vec<Problem>);

impl fmt::Display for Problems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self
            .0
            .iter()
            .filter(|p| p.severity == Severity::Error)
            .enumerate()
        {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

/// Why the preprocessor couldn't finish.
#[derive(Debug, Error)]
pub enum PreprocessError {
    /// mdBook's input (or the book to print) isn't valid JSON.
    #[error("mdBook's input isn't valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// The input isn't `[context, book]`.
    #[error("mdBook's input isn't [context, book]; is this running as an mdBook preprocessor?")]
    Shape,

    /// An `on-error` that isn't `"fail"` or `"warn"`.
    #[error("[preprocessor.rain-diagram] on-error is {0}; expected \"fail\" or \"warn\"")]
    OnError(String),

    /// Diagrams that don't parse, with `on-error = "fail"`.
    #[error("{0}")]
    Diagrams(Problems),
}

#[cfg(test)]
mod tests {
    use super::*;
    use testresult::TestResult;

    const CHAPTER: &str =
        "# Title\n\nSome text.\n\n```rain\n(Doc)-[edit(Doc)]->(Alice)\n```\n\nAfter.\n";

    #[test]
    fn diagram_blocks_become_svg() {
        let (out, problems) = markdown(CHAPTER, "c.md", true, OnError::Fail);
        assert!(problems.is_empty());
        assert!(out.starts_with(
            "# Title\n\nSome text.\n\n<div class=\"rain-diagram\" style=\"overflow-x: auto\">\n<svg"
        ));
        assert!(out.ends_with("</svg>\n</div>\n\n\nAfter.\n"), "{out}");
        let block = out
            .split("<div")
            .nth(1)
            .and_then(|b| b.split("</div>").next());
        assert!(
            block.is_some_and(|b| !b.contains("\n\n")),
            "no blank line inside the HTML block"
        );
    }

    #[test]
    fn other_renderers_get_text_art() {
        let (out, _) = markdown(CHAPTER, "c.md", false, OnError::Fail);
        assert!(out.contains("```text\n (Doc)"), "{out}");
    }

    #[test]
    fn a_written_header_is_kept_and_any_fence_works() {
        let chapter = "~~~~ certflow  extra words\ncertflow\n(Doc)-[edit(Doc)]->(Alice)\n~~~~\n";
        let (out, problems) = markdown(chapter, "c.md", true, OnError::Fail);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(out.contains("class=\"rain-diagram certflow\""), "{out}");
    }

    #[test]
    fn other_code_is_left_alone() {
        let chapter = "```rust\nlet rain = 1;\n```\n````\n```rain\nnot a diagram\n```\n````\n    ```rain\n    indented code\n";
        assert_eq!(markdown(chapter, "c.md", true, OnError::Fail).0, chapter);
    }

    #[test]
    fn errors_point_into_the_chapter() {
        let chapter = "Intro\n\n```rain\n(Doc)-[edit(Doc)]->(Alice)\n(Alice)-[read]->(Bob)\n```\n";
        let (out, problems) = markdown(chapter, "src/c.md", true, OnError::Warn);
        assert_eq!(
            problems.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["src/c.md:5:14: error: expected the subject after the ability, as in `[edit(Doc)]`"]
        );
        assert!(
            out.contains("<pre class=\"rain-diagram-error\">src/c.md:5:14"),
            "{out}"
        );
    }

    #[test]
    fn unclosed_fences_stay_as_written() {
        let chapter = "```rain\n(Doc)-[edit(Doc)]->(Alice)\n";
        assert_eq!(markdown(chapter, "c.md", true, OnError::Fail).0, chapter);
    }

    fn input(on_error: Option<&str>, content: &str) -> String {
        let config = on_error.map_or_else(
            || serde_json::json!({}),
            |e| serde_json::json!({ "preprocessor": { "rain-diagram": { "on-error": e } } }),
        );
        serde_json::json!([
            { "root": ".", "config": config, "renderer": "html", "mdbook_version": "0.5.2" },
            { "items": [ { "Chapter": {
                "name": "One", "content": content, "number": [1], "path": "one.md",
                "source_path": "one.md", "parent_names": [],
                "sub_items": [ { "Chapter": {
                    "name": "Two", "content": content, "number": [1, 1], "path": "two.md",
                    "source_path": "two.md", "parent_names": ["One"], "sub_items": []
                } } ]
            } }, "Separator" ] }
        ])
        .to_string()
    }

    #[test]
    fn walks_nested_chapters() -> TestResult {
        let (book, problems) = run(&input(None, CHAPTER))?;
        assert!(problems.is_empty());
        let book: Value = serde_json::from_str(&book)?;
        let contents: Vec<&str> = [
            "/items/0/Chapter/content",
            "/items/0/Chapter/sub_items/0/Chapter/content",
        ]
        .iter()
        .filter_map(|p| book.pointer(p).and_then(Value::as_str))
        .collect();
        assert_eq!(contents.len(), 2);
        assert!(contents.iter().all(|c| c.contains("<svg")));
        assert_eq!(book.pointer("/items/1"), Some(&Value::from("Separator")));
        Ok(())
    }

    #[test]
    fn broken_diagrams_fail_the_build_unless_warned() -> TestResult {
        let broken = "```rain\n(A)-[read]->(B)\n```\n";
        let Err(PreprocessError::Diagrams(problems)) = run(&input(None, broken)) else {
            return Err("expected a failure".into());
        };
        assert_eq!(
            problems.to_string(),
            "one.md:2:10: error: expected the subject after the ability, as in `[edit(Doc)]`\n\
             two.md:2:10: error: expected the subject after the ability, as in `[edit(Doc)]`"
        );
        let (_, warned) = run(&input(Some("warn"), broken))?;
        assert_eq!(warned.len(), 2);
        assert!(matches!(
            run(&input(Some("panic"), broken)),
            Err(PreprocessError::OnError(_))
        ));
        assert!(matches!(run("{}"), Err(PreprocessError::Shape)));
        Ok(())
    }

    /// Markdown with no diagram fences passes through untouched.
    #[test]
    fn everything_else_is_untouched() {
        bolero::check!().with_type::<String>().for_each(|text| {
            if text
                .lines()
                .any(|l| Fence::open(l).is_some_and(|f| f.notation.is_some()))
            {
                return;
            }
            assert_eq!(&markdown(text, "c.md", true, OnError::Fail).0, text);
        });
    }
}
