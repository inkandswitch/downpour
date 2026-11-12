//! Syntax tree for the textual notation, exactly as written.
//!
//! The AST keeps what the author wrote: omitted subjects stay omitted and
//! certificate references stay names. That lets it print back out as the
//! same source (see the [`Display`](fmt::Display) impls). Resolving names,
//! inferring subjects, and checking references happens in
//! [`crate::diagram`].

use crate::level::Level;
use alloc::{string::String, vec::Vec};
use core::fmt;
use thiserror::Error;

/// Which notation a document is written in.
///
/// Both kinds share one statement language and one authority model. They
/// differ only in rendering: rain diagrams lay events out in time (lifelines
/// and rows), certflow diagrams show a single snapshot as a graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Kind {
    /// A snapshot graph of principals and certificates.
    Certflow,

    /// Certflow merged with a sequence diagram: time runs down the page.
    Rain,
}

impl Kind {
    /// The header keyword that selects this kind.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Certflow => "certflow",
            Self::Rain => "rain",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.keyword())
    }
}

/// A whole document: header, directives, and statements in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The notation, from the header line.
    pub kind: Kind,

    /// The optional `title` directive.
    pub title: Option<Spanned<Text>>,

    /// Principals from `participant` directives, which fix column order.
    pub participants: Vec<Spanned<Name>>,

    /// Delegations, invocations, and revocations, in causal order.
    pub statements: Vec<Spanned<Statement>>,
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.kind)?;

        if let Some(title) = &self.title {
            writeln!(f, "title {}", title.node)?;
        }

        if !self.participants.is_empty() {
            f.write_str("participant ")?;
            write_list(f, self.participants.iter().map(|p| &p.node))?;
            writeln!(f)?;
        }

        self.statements
            .iter()
            .try_for_each(|statement| writeln!(f, "{}", statement.node))
    }
}

/// One line of the diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Statement {
    /// `(Alice)-[edit(Doc)]->(Bob)`
    Delegate(Delegation),

    /// `(Bob)=[[edit]]=>(Doc)`
    Invoke(Invocation),

    /// `(Alice)-<revoke>-x[cd]`
    Revoke(Revocation),
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Delegate(d) => d.fmt(f),
            Self::Invoke(i) => i.fmt(f),
            Self::Revoke(r) => r.fmt(f),
        }
    }
}

/// A certificate: the issuer grants the audience a level over a subject.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Delegation {
    /// An explicit label (`cd: …`) that revocations and `via` can refer to.
    pub id: Option<CertId>,

    /// Who signs the certificate.
    pub issuer: Name,

    /// The access granted.
    pub level: Level,

    /// What the access is over. Omitted means "obvious from context".
    pub subject: Option<Name>,

    /// Certificates that carry the subject's authority to the issuer
    /// (transitive handles).
    pub via: Vec<CertId>,

    /// Who receives the certificate.
    pub audience: Name,

    /// A trailing `: note`.
    pub note: Option<Text>,
}

impl fmt::Display for Delegation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(id) = &self.id {
            write!(f, "{id}: ")?;
        }

        write!(f, "({})-[{}", self.issuer, self.level)?;

        if let Some(subject) = &self.subject {
            write!(f, "({subject})")?;
        }

        if !self.via.is_empty() {
            f.write_str(" via ")?;
            write_list(f, &self.via)?;
        }

        write!(f, "]->({})", self.audience)?;
        write_note(f, self.note.as_ref())
    }
}

/// A principal using its authority on a subject, such as editing a document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Invocation {
    /// Who acts.
    pub by: Name,

    /// The access being exercised.
    pub level: Level,

    /// What is acted on.
    pub on: Name,

    /// A trailing `: note`.
    pub note: Option<Text>,
}

impl fmt::Display for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({})=[[{}]]=>({})", self.by, self.level, self.on)?;
        write_note(f, self.note.as_ref())
    }
}

/// A signed statement that invalidates an earlier certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Revocation {
    /// Who revokes.
    pub by: Name,

    /// The certificate being revoked.
    pub target: CertId,

    /// A trailing `: note`.
    pub note: Option<Text>,
}

impl fmt::Display for Revocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({})-<revoke>-x[{}]", self.by, self.target)?;
        write_note(f, self.note.as_ref())
    }
}

/// A principal's name, as written between round brackets.
///
/// Names may contain spaces and any Unicode, but not the notation's
/// delimiters `()[],`, control characters, or a `%%` comment marker, and
/// have no surrounding whitespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// Check and wrap a principal name.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidName`] if the name is empty, has surrounding
    /// whitespace, or contains a reserved character or `%%`.
    pub fn new(name: &str) -> Result<Self, InvalidName> {
        if name.is_empty() {
            return Err(InvalidName::Empty);
        }

        if name.trim() != name {
            return Err(InvalidName::Untrimmed);
        }

        if let Some(c) = name
            .chars()
            .find(|c| matches!(c, '(' | ')' | '[' | ']' | ',') || c.is_control())
        {
            return Err(InvalidName::Reserved(c));
        }

        if name.contains("%%") {
            return Err(InvalidName::Comment);
        }

        Ok(Self(name.into()))
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A certificate label, such as `cd` in `cd: (Carol)-[read]->(Dan)`.
///
/// Labels are letters, digits, `-`, `_`, and `.`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CertId(String);

impl CertId {
    /// Check and wrap a certificate label.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCertId`] if the label is empty or contains a
    /// character other than a letter, digit, `-`, `_`, or `.`.
    pub fn new(id: &str) -> Result<Self, InvalidCertId> {
        if id.is_empty() {
            return Err(InvalidCertId::Empty);
        }

        match id.chars().find(|&c| !Self::is_id_char(c)) {
            Some(c) => Err(InvalidCertId::InvalidChar(c)),
            None => Ok(Self(id.into())),
        }
    }

    /// Whether `c` may appear in a label.
    #[must_use]
    pub fn is_id_char(c: char) -> bool {
        c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
    }

    /// The label as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CertId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Free text on one line: a title or a note.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Text(String);

impl Text {
    /// Check and wrap one line of text.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidText`] if the text is empty, has surrounding
    /// whitespace, contains a control character, or contains `%%`.
    pub fn new(text: &str) -> Result<Self, InvalidText> {
        if text.is_empty() {
            return Err(InvalidText::Empty);
        }

        if text.trim() != text {
            return Err(InvalidText::Untrimmed);
        }

        if let Some(c) = text.chars().find(|c| c.is_control()) {
            return Err(InvalidText::Control(c));
        }

        if text.contains("%%") {
            return Err(InvalidText::Comment);
        }

        Ok(Self(text.into()))
    }

    /// The text as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A byte range in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// Inclusive start offset.
    pub start: usize,

    /// Exclusive end offset.
    pub end: usize,
}

impl Span {
    /// The 1-based line and column (in characters) where this span starts.
    ///
    /// ```
    /// use rain_diagram_core::ast::Span;
    ///
    /// let source = "rain\n(Doc)-[nope]->(Alice)";
    /// assert_eq!(Span { start: 12, end: 16 }.line_col(source), (2, 8));
    /// ```
    #[must_use]
    pub fn line_col(self, source: &str) -> (usize, usize) {
        let before = source.get(..self.start).unwrap_or(source);
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before.get(line_start..).unwrap_or_default().chars().count();
        (before.matches('\n').count() + 1, column + 1)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// A node and where it came from in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    /// The node.
    pub node: T,

    /// Its location in the source.
    pub span: Span,
}

/// Why a string is not a valid [`Name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InvalidName {
    /// Nothing between the brackets.
    #[error("principal name is empty")]
    Empty,

    /// Leading or trailing whitespace.
    #[error("principal name has surrounding whitespace")]
    Untrimmed,

    /// A delimiter or control character.
    #[error("principal name contains reserved character {0:?}")]
    Reserved(char),

    /// A `%%` comment marker.
    #[error("principal name contains `%%`")]
    Comment,
}

/// Why a string is not a valid [`CertId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InvalidCertId {
    /// No label.
    #[error("certificate label is empty")]
    Empty,

    /// A character outside letters, digits, `-`, `_`, and `.`.
    #[error("certificate label contains {0:?}; use letters, digits, `-`, `_`, or `.`")]
    InvalidChar(char),
}

/// Why a string is not valid [`Text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InvalidText {
    /// No text.
    #[error("text is empty")]
    Empty,

    /// Leading or trailing whitespace.
    #[error("text has surrounding whitespace")]
    Untrimmed,

    /// A control character, such as a newline.
    #[error("text contains control character {0:?}")]
    Control(char),

    /// A `%%` comment marker.
    #[error("text contains `%%`")]
    Comment,
}

fn write_list<T: fmt::Display, I: IntoIterator<Item = T>>(
    f: &mut fmt::Formatter<'_>,
    items: I,
) -> fmt::Result {
    items.into_iter().enumerate().try_for_each(|(i, item)| {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{item}")
    })
}

fn write_note(f: &mut fmt::Formatter<'_>, note: Option<&Text>) -> fmt::Result {
    note.map_or(Ok(()), |note| write!(f, " : {note}"))
}

#[cfg(feature = "arbitrary")]
mod arbitrary_impls {
    use super::{CertId, Document, Kind, Name, Span, Spanned, Statement, Text};
    use alloc::{string::String, vec::Vec};
    use arbitrary::{Arbitrary, Result, Unstructured};

    fn string_from(u: &mut Unstructured<'_>, alphabet: &[char], max: usize) -> Result<String> {
        let len = u.int_in_range(1..=max)?;
        (0..len).map(|_| u.choose(alphabet).copied()).collect()
    }

    impl<'a> Arbitrary<'a> for Name {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            const ALPHABET: &[char] = &[
                'a', 'B', 'z', '0', '9', ' ', '-', '_', '.', ':', 'é', '→', '📱', '👑',
            ];
            let raw = string_from(u, ALPHABET, 12)?;
            Ok(Self::new(raw.trim()).unwrap_or_else(|_| Self("x".into())))
        }
    }

    impl<'a> Arbitrary<'a> for CertId {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            const ALPHABET: &[char] = &['a', 'Z', '0', '9', '-', '_', '.', 'é'];
            Ok(Self(string_from(u, ALPHABET, 8)?))
        }
    }

    impl<'a> Arbitrary<'a> for Text {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            const ALPHABET: &[char] = &[
                'a', 'B', '1', ' ', '(', ')', '[', ']', ',', ':', '-', '>', '%', '→', '✏',
                '\u{fe0f}',
            ];
            let raw = string_from(u, ALPHABET, 24)?;
            let raw = raw.replace("%%", "%");
            Ok(Self::new(raw.trim()).unwrap_or_else(|_| Self("note".into())))
        }
    }

    const fn spanned<T>(node: T) -> Spanned<T> {
        Spanned {
            node,
            span: Span { start: 0, end: 0 },
        }
    }

    impl<'a> Arbitrary<'a> for Document {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            Ok(Self {
                kind: Kind::arbitrary(u)?,
                title: Option::<Text>::arbitrary(u)?.map(spanned),
                participants: Vec::<Name>::arbitrary(u)?
                    .into_iter()
                    .map(spanned)
                    .collect(),
                statements: Vec::<Statement>::arbitrary(u)?
                    .into_iter()
                    .map(spanned)
                    .collect(),
            })
        }
    }
}
