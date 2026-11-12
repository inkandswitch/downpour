//! Hand-written, line-oriented parser from source text to [`Document`].
//!
//! The grammar is small and line-based, so a hand-written parser stays short,
//! adds no dependencies (keeping the Wasm small), and can point every error
//! at an exact byte span.
//!
//! ```text
//! document    = header { line }
//! header      = "rain" | "certflow"
//! line        = directive | statement
//! directive   = "title" text | "participant" name { "," name }
//! statement   = [ cert-id ":" ] ( delegate | invoke | revoke ) [ ":" text ]
//! delegate    = principal "-[" level [ principal ] [ "via" cert-id { "," cert-id } ] "]->" principal
//! invoke      = principal "=[[" level "]]=>" principal
//! revoke      = principal "-<revoke>-x" "[" cert-id "]"
//! principal   = "(" name ")"
//! ```
//!
//! `%%` starts a comment that runs to the end of the line, as in Mermaid.

use crate::{
    ast::{
        CertId, Delegation, Document, InvalidCertId, InvalidName, InvalidText, Invocation, Kind,
        Name, Revocation, Span, Spanned, Statement, Text,
    },
    level::{Level, UnknownLevel},
};
use alloc::{string::String, vec::Vec};
use thiserror::Error;

/// Parse source text into a [`Document`].
///
/// ```
/// use rain_diagram_core::{ast::Kind, parser};
///
/// let doc = parser::document("certflow\n(Carol)-[admin]->(Alice)\n")?;
/// assert_eq!(doc.kind, Kind::Certflow);
/// assert_eq!(doc.statements.len(), 1);
/// # Ok::<(), parser::ParseError>(())
/// ```
///
/// # Errors
///
/// Returns a [`ParseError`] pointing at the first malformed line.
pub fn document(source: &str) -> Result<Document, ParseError> {
    let mut lines = lines(source);

    let (header_span, header) = lines
        .next()
        .ok_or_else(|| ParseError::new(Span::at(source.len()), ParseErrorKind::MissingHeader))?;

    let kind = match header {
        "rain" => Kind::Rain,
        "certflow" => Kind::Certflow,
        other => {
            return Err(ParseError::new(
                header_span,
                ParseErrorKind::UnknownKind(other.into()),
            ));
        }
    };

    let mut doc = Document {
        kind,
        title: None,
        participants: Vec::new(),
        statements: Vec::new(),
    };

    for (span, line) in lines {
        if let Some(rest) = directive(line, "title") {
            if doc.title.is_some() {
                return Err(ParseError::new(span, ParseErrorKind::DuplicateTitle));
            }

            let node = Text::new(rest.trim()).map_err(|e| ParseError::new(span, e.into()))?;
            doc.title = Some(Spanned { node, span });
        } else if let Some(rest) = directive(line, "participant") {
            let mut offset = span.end - rest.len();
            for piece in rest.split(',') {
                let piece_span = Span {
                    start: offset,
                    end: offset + piece.len(),
                };
                offset = piece_span.end + 1;

                let node =
                    Name::new(piece.trim()).map_err(|e| ParseError::new(piece_span, e.into()))?;
                doc.participants.push(Spanned {
                    node,
                    span: piece_span,
                });
            }
        } else {
            let node = statement(&mut Cursor::new(line, span.start))?;
            doc.statements.push(Spanned { node, span });
        }
    }

    Ok(doc)
}

/// Non-blank lines with comments and surrounding whitespace removed.
fn lines(source: &str) -> impl Iterator<Item = (Span, &str)> {
    let mut offset = 0;
    source.split_inclusive('\n').filter_map(move |raw| {
        let start = offset;
        offset += raw.len();

        let code = raw.split_once("%%").map_or(raw, |(code, _)| code);
        let trimmed = code.trim();
        let leading = code.len() - code.trim_start().len();

        (!trimmed.is_empty()).then(|| {
            let start = start + leading;
            let span = Span {
                start,
                end: start + trimmed.len(),
            };
            (span, trimmed)
        })
    })
}

/// The rest of the line if it starts with `keyword` followed by whitespace.
fn directive<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    line.strip_prefix(keyword)
        .filter(|rest| rest.starts_with(char::is_whitespace))
}

fn statement(c: &mut Cursor<'_>) -> Result<Statement, ParseError> {
    let id = if c.rest().starts_with('(') {
        None
    } else {
        let (raw, span) = c.take_while(|ch| ch != ':' && ch != '(');
        c.expect(
            ":",
            "`:` after a certificate label, or `(` to start a statement",
        )?;
        let id = CertId::new(raw.trim_end()).map_err(|e| ParseError::new(span, e.into()))?;
        Some(Spanned { node: id, span })
    };

    let by = principal(c)?;

    if c.eat("-[") {
        let level = level(c)?;

        c.skip_ws();
        let subject = if c.rest().starts_with('(') {
            Some(principal(c)?)
        } else {
            None
        };

        let via = if c.eat_word("via") {
            let mut ids = Vec::from([cert_id(c)?]);
            while c.eat(",") {
                ids.push(cert_id(c)?);
            }
            ids
        } else {
            Vec::new()
        };

        c.expect("]", "`]` to close the certificate")?;
        c.expect("->", "`->` after the certificate")?;
        let audience = principal(c)?;

        return Ok(Statement::Delegate(Delegation {
            id: id.map(|id| id.node),
            issuer: by,
            level,
            subject,
            via,
            audience,
            note: note(c)?,
        }));
    }

    if let Some(id) = id {
        return Err(ParseError::new(
            id.span,
            ParseErrorKind::LabelOnNonDelegation,
        ));
    }

    if c.eat("=[[") {
        let level = level(c)?;
        c.expect("]]", "`]]` to close the invocation")?;
        c.expect("=>", "`=>` after the invocation")?;
        let on = principal(c)?;

        return Ok(Statement::Invoke(Invocation {
            by,
            level,
            on,
            note: note(c)?,
        }));
    }

    if c.eat("-<revoke>-x") {
        c.expect("[", "`[` before the revoked certificate's label")?;
        let target = cert_id(c)?;
        c.expect("]", "`]` after the revoked certificate's label")?;

        return Ok(Statement::Revoke(Revocation {
            by,
            target,
            note: note(c)?,
        }));
    }

    Err(c.error_here(ParseErrorKind::Expected(
        "`-[` (delegate), `=[[` (invoke), or `-<revoke>-x` (revoke)",
    )))
}

fn principal(c: &mut Cursor<'_>) -> Result<Name, ParseError> {
    c.expect("(", "`(` to start a principal")?;
    let (raw, span) = c.take_while(|ch| ch != ')');
    c.expect(")", "`)` to close the principal")?;
    Name::new(raw.trim()).map_err(|e| ParseError::new(span, e.into()))
}

fn level(c: &mut Cursor<'_>) -> Result<Level, ParseError> {
    let (raw, span) = c.take_while(|ch| !matches!(ch, '(' | ']') && !ch.is_whitespace());
    raw.parse()
        .map_err(|e: UnknownLevel| ParseError::new(span, e.into()))
}

fn cert_id(c: &mut Cursor<'_>) -> Result<CertId, ParseError> {
    let (raw, span) = c.take_while(CertId::is_id_char);
    CertId::new(raw).map_err(|e| ParseError::new(span, e.into()))
}

fn note(c: &mut Cursor<'_>) -> Result<Option<Text>, ParseError> {
    if c.at_end() {
        return Ok(None);
    }

    c.expect(":", "end of line, or `:` before a note")?;
    let (raw, span) = c.take_while(|_| true);
    Text::new(raw.trim())
        .map(Some)
        .map_err(|e| ParseError::new(span, e.into()))
}

/// A position within one line, tracking absolute offsets for spans.
struct Cursor<'a> {
    line: &'a str,
    offset: usize,
    pos: usize,
}

impl<'a> Cursor<'a> {
    const fn new(line: &'a str, offset: usize) -> Self {
        Self {
            line,
            offset,
            pos: 0,
        }
    }

    fn rest(&self) -> &'a str {
        self.line.get(self.pos..).unwrap_or_default()
    }

    const fn here(&self) -> usize {
        self.offset + self.pos
    }

    fn skip_ws(&mut self) {
        let rest = self.rest();
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.rest().is_empty()
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_ws();
        let found = self.rest().starts_with(token);
        if found {
            self.pos += token.len();
        }
        found
    }

    /// Like [`Self::eat`], but only matches a whole word.
    fn eat_word(&mut self, word: &str) -> bool {
        self.skip_ws();
        let found = self
            .rest()
            .strip_prefix(word)
            .is_some_and(|after| after.starts_with(char::is_whitespace));
        if found {
            self.pos += word.len();
        }
        found
    }

    fn expect(&mut self, token: &str, what: &'static str) -> Result<(), ParseError> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.error_here(ParseErrorKind::Expected(what)))
        }
    }

    fn take_while<P: Fn(char) -> bool>(&mut self, pred: P) -> (&'a str, Span) {
        self.skip_ws();
        let rest = self.rest();
        let len = rest.find(|ch| !pred(ch)).unwrap_or(rest.len());
        let start = self.here();
        self.pos += len;
        (
            rest.get(..len).unwrap_or_default(),
            Span {
                start,
                end: start + len,
            },
        )
    }

    fn error_here(&self, kind: ParseErrorKind) -> ParseError {
        let start = self.here();
        let len = self.rest().chars().next().map_or(0, char::len_utf8);
        ParseError::new(
            Span {
                start,
                end: start + len,
            },
            kind,
        )
    }
}

impl Span {
    const fn at(offset: usize) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }
}

/// Malformed source text.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{kind}")]
pub struct ParseError {
    span: Span,
    kind: ParseErrorKind,
}

impl ParseError {
    /// Create an error at `span`.
    #[must_use]
    pub const fn new(span: Span, kind: ParseErrorKind) -> Self {
        Self { span, kind }
    }

    /// Where in the source the problem is.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// What the problem is.
    #[must_use]
    pub const fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }
}

/// The ways source text can be malformed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseErrorKind {
    /// The source has no non-blank lines.
    #[error("missing header; start with `rain` or `certflow`")]
    MissingHeader,

    /// The first line isn't a known diagram kind.
    #[error("unknown diagram kind {0:?}; expected `rain` or `certflow`")]
    UnknownKind(String),

    /// More than one `title` directive.
    #[error("a diagram can only have one title")]
    DuplicateTitle,

    /// Something else was expected here.
    #[error("expected {0}")]
    Expected(&'static str),

    /// A label on an invocation or revocation.
    #[error("only delegations can be labelled")]
    LabelOnNonDelegation,

    /// A bad access level.
    #[error(transparent)]
    Level(#[from] UnknownLevel),

    /// A bad principal name.
    #[error(transparent)]
    Name(#[from] InvalidName),

    /// A bad certificate label.
    #[error(transparent)]
    CertId(#[from] InvalidCertId),

    /// A bad title or note.
    #[error(transparent)]
    Text(#[from] InvalidText),
}

#[cfg(test)]
mod tests {
    use super::*;
    use testresult::TestResult;

    fn name(s: &str) -> Name {
        Name::new(s).unwrap_or_else(|e| unreachable!("test name {s:?}: {e}"))
    }

    fn statements(src: &str) -> Result<Vec<Statement>, ParseError> {
        Ok(document(src)?
            .statements
            .into_iter()
            .map(|s| s.node)
            .collect())
    }

    #[test]
    fn delegation_forms() -> TestResult {
        let parsed = statements(
            "rain
             (Doc)-[edit]->(Alice)
             cd: (Carol)-[ read ( Doc ) ]->( Dan ) : shares Read
             (Alice)-[edit(Doc) via ta, x.1]->(Bob)",
        )?;

        assert_eq!(
            parsed,
            [
                Statement::Delegate(Delegation {
                    id: None,
                    issuer: name("Doc"),
                    level: Level::Edit,
                    subject: None,
                    via: Vec::new(),
                    audience: name("Alice"),
                    note: None,
                }),
                Statement::Delegate(Delegation {
                    id: Some(CertId::new("cd")?),
                    issuer: name("Carol"),
                    level: Level::Read,
                    subject: Some(name("Doc")),
                    via: Vec::new(),
                    audience: name("Dan"),
                    note: Some(Text::new("shares Read")?),
                }),
                Statement::Delegate(Delegation {
                    id: None,
                    issuer: name("Alice"),
                    level: Level::Edit,
                    subject: Some(name("Doc")),
                    via: Vec::from([CertId::new("ta")?, CertId::new("x.1")?]),
                    audience: name("Bob"),
                    note: None,
                }),
            ]
        );
        Ok(())
    }

    #[test]
    fn invocation_and_revocation() -> TestResult {
        let parsed = statements(
            "rain %% header comment
             (Bob)=[[✏️]]=>(Doc)
             (Alice)-<revoke>-x[carol-dan] : Alice revokes Carol→Dan",
        )?;

        assert_eq!(
            parsed,
            [
                Statement::Invoke(Invocation {
                    by: name("Bob"),
                    level: Level::Edit,
                    on: name("Doc"),
                    note: None,
                }),
                Statement::Revoke(Revocation {
                    by: name("Alice"),
                    target: CertId::new("carol-dan")?,
                    note: Some(Text::new("Alice revokes Carol→Dan")?),
                }),
            ]
        );
        Ok(())
    }

    #[test]
    fn directives() -> TestResult {
        let doc = document("certflow\ntitle Sharing\nparticipant Doc,  Comment Thread 1\n")?;
        assert_eq!(doc.title.map(|t| t.node), Some(Text::new("Sharing")?));
        assert_eq!(
            doc.participants
                .into_iter()
                .map(|p| p.node)
                .collect::<Vec<_>>(),
            [name("Doc"), name("Comment Thread 1")]
        );
        Ok(())
    }

    #[test]
    fn errors_point_at_the_problem() {
        let source = "rain\n(Doc)-[write]->(Alice)";
        let err = document(source).err();
        assert_eq!(
            err.as_ref().map(ParseError::kind),
            Some(&ParseErrorKind::Level(
                "write"
                    .parse::<Level>()
                    .err()
                    .unwrap_or_else(|| unreachable!())
            ))
        );
        assert_eq!(err.map(|e| e.span().line_col(source)), Some((2, 8)));
    }

    #[test]
    fn rejects_labelled_invocation() {
        assert!(matches!(
            document("rain\nx: (Bob)=[[edit]]=>(Doc)").map_err(|e| e.kind().clone()),
            Err(ParseErrorKind::LabelOnNonDelegation)
        ));
    }

    #[test]
    fn missing_and_unknown_header() {
        assert!(matches!(
            document("  %% only a comment\n").map_err(|e| e.kind().clone()),
            Err(ParseErrorKind::MissingHeader)
        ));
        assert!(matches!(
            document("sequenceDiagram\n").map_err(|e| e.kind().clone()),
            Err(ParseErrorKind::UnknownKind(_))
        ));
    }
}
