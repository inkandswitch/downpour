//! Syntax tree for the textual notation, exactly as written.
//!
//! The AST keeps what the author wrote: certificate references stay names.
//! That lets it print back out as the same source (see the
//! [`Display`](fmt::Display) impls). Resolving names and checking references
//! happens in [`crate::diagram`].

use crate::ability::Ability;
use alloc::{string::String, vec::Vec};
use core::{
    fmt::{self, Write as _},
    num::NonZeroU8,
};
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

    /// Labelled regions (`>>> A: note` … `<<< A`), in the order they open.
    pub regions: Vec<Spanned<Region>>,
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.kind)?;

        if let Some(title) = &self.title {
            writeln!(f, "title {}", title.node)?;
        }

        if !self.participants.is_empty() {
            f.write_str("principals: ")?;
            write_list(f, self.participants.iter().map(|p| &p.node))?;
            writeln!(f)?;
        }

        // Regions are interleaved with the statements they cover: at each
        // position, close what ends here (innermost first), then open what
        // starts here (in order)
        let len = self.statements.len();
        for at in 0..=len {
            let mut closing: Vec<&Region> = self
                .regions
                .iter()
                .map(|r| &r.node)
                .filter(|r| r.statements.end == at)
                .collect();
            closing.reverse();
            closing.sort_by_key(|r| core::cmp::Reverse(r.statements.start));
            for region in closing {
                writeln!(f, "<<< {}", region.id)?;
            }

            for region in self.regions.iter().map(|r| &r.node) {
                if region.statements.start == at && at < len {
                    write!(f, ">>> {}", region.id)?;
                    if let Some(colour) = &region.colour {
                        write!(f, " ({colour})")?;
                    }
                    if let Some(note) = &region.note {
                        write!(f, ": {note}")?;
                    }
                    writeln!(f)?;
                }
            }

            if let Some(statement) = self.statements.get(at) {
                writeln!(f, "{}", statement.node)?;
            }
        }
        Ok(())
    }
}

/// A labelled stretch of statements, drawn as a coloured box:
///
/// ```text
/// >>> A: Bob does stuff
/// (Bob)=[[edit]]=>(Doc)
/// <<< A
/// ```
///
/// Regions may nest or overlap, since each is closed by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// The name that opens and closes it. Same rules as certificate labels.
    pub id: CertId,

    /// A principal whose colour it takes: `>>> A (Bob): note`.
    pub colour: Option<Name>,

    /// What it's about, if anything.
    pub note: Option<Text>,

    /// The statements it covers, by index. Never empty.
    pub statements: core::ops::Range<usize>,
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

/// A certificate: the issuer grants the audience an ability over a subject,
/// perhaps with a caveat.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Delegation {
    /// An explicit label (`cd: …`) that revocations and `via` can refer to.
    pub id: Option<CertId>,

    /// Who signs the certificate.
    pub issuer: Name,

    /// The access granted.
    pub ability: Ability,

    /// What the access is over. Always written: `[edit(Doc)]`, or
    /// `[admin(Team)]` for the issuer's own authority.
    pub subject: Name,

    /// Certificates that carry the subject's authority to the issuer
    /// (transitive handles).
    pub via: Vec<CertId>,

    /// A `where` condition the access is limited by.
    pub caveat: Option<Caveat>,

    /// Who receives the certificate.
    pub audience: Name,

    /// Where its label sits along the arrow, if placed by hand.
    pub at: Option<Placement>,

    /// A trailing `: note`.
    pub note: Option<Text>,
}

impl fmt::Display for Delegation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(id) = &self.id {
            write!(f, "{id}: ")?;
        }

        let (before, after) = runs(self.at);
        write!(f, "({})", self.issuer)?;
        write_run(f, '-', before)?;
        write!(f, "[{}", self.ability)?;

        write!(f, "({})", self.subject)?;

        if !self.via.is_empty() {
            f.write_str(" via ")?;
            write_list(f, &self.via)?;
        }

        if let Some(caveat) = &self.caveat {
            write!(f, " where {caveat}")?;
        }

        f.write_char(']')?;
        write_run(f, '-', after)?;
        write!(f, ">({})", self.audience)?;
        write_tail(f, self.note.as_ref())
    }
}

/// A principal using its authority on a subject, such as editing a document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Invocation {
    /// Who acts.
    pub by: Name,

    /// The access being exercised.
    pub ability: Ability,

    /// What is acted on.
    pub on: Name,

    /// Where its label sits along the arrow, if placed by hand.
    pub at: Option<Placement>,

    /// A trailing `: note`.
    pub note: Option<Text>,
}

impl fmt::Display for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (before, after) = runs(self.at);
        write!(f, "({})", self.by)?;
        write_run(f, '=', before)?;
        write!(f, "[[{}]]", self.ability)?;
        write_run(f, '=', after)?;
        write!(f, ">({})", self.on)?;
        write_tail(f, self.note.as_ref())
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

    /// Where its label sits along the arrow, if placed by hand.
    pub at: Option<Placement>,

    /// A trailing `: note`.
    pub note: Option<Text>,
}

impl fmt::Display for Revocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (before, after) = runs(self.at);
        write!(f, "({})", self.by)?;
        write_run(f, '-', before)?;
        f.write_str("<revoke>")?;
        write_run(f, '-', after)?;
        write!(f, "x[{}]", self.target)?;
        write_tail(f, self.note.as_ref())
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

        if comment_start(name).is_some() {
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

/// A certificate label, such as `cd` in `cd: (Carol)-[read(Doc)]->(Dan)`.
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

        if comment_start(text).is_some() {
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

/// A delegation's `where` condition: [`Text`] without square brackets.
///
/// Caveats are opaque: diagrams carry them along and show them, but never
/// interpret them. That's enough to see how conditions pile up as authority
/// is passed on, whatever language (UCAN policies, Biscuit checks) they're
/// written in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Caveat(Text);

impl Caveat {
    /// Check and wrap a caveat.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCaveat`] if it isn't valid [`Text`], or contains
    /// `[` or `]`.
    pub fn new(text: &str) -> Result<Self, InvalidCaveat> {
        if let Some(c) = text.chars().find(|c| matches!(c, '[' | ']')) {
            return Err(InvalidCaveat::Bracket(c));
        }

        Ok(Self(Text::new(text)?))
    }

    /// The caveat as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for Caveat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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

    /// A comment marker: `%%`, or `//` at the start or after a space.
    #[error("principal name contains a comment marker (`%%`, or `//` after a space)")]
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

    /// A comment marker: `%%`, or `//` at the start or after a space.
    #[error("text contains a comment marker (`%%`, or `//` after a space)")]
    Comment,
}

/// Why a string is not a valid [`Caveat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InvalidCaveat {
    /// Not valid text.
    #[error("caveat: {0}")]
    Text(#[from] InvalidText),

    /// A square bracket, which would end the certificate early.
    #[error("caveat contains {0:?}; caveats can't contain square brackets")]
    Bracket(char),
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

/// Where a comment starts in `line`, if it has one: `%%` anywhere (as in
/// Mermaid), or `//` at the start of the line or after whitespace. The
/// whitespace rule keeps URLs like `https://…` in notes intact.
#[must_use]
pub fn comment_start(line: &str) -> Option<usize> {
    let percent = line.find("%%");
    let slashes = line.match_indices("//").map(|(i, _)| i).find(|&i| {
        line.get(..i)
            .is_some_and(|before| before.is_empty() || before.ends_with(char::is_whitespace))
    });
    match (percent, slashes) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// A statement's optional ` : note`.
fn write_tail(f: &mut fmt::Formatter<'_>, note: Option<&Text>) -> fmt::Result {
    note.map_or(Ok(()), |note| write!(f, " : {note}"))
}

/// Where a row's label sits along its arrow, written as the runs of dashes
/// (`=` for an invocation) either side of it: `(Alice)---[edit(Doc)]->(Bob)`
/// puts it three quarters of the way to Bob. One each side is automatic
/// placement, so a `Placement` is never 1:1.
///
/// ```
/// use rain_diagram_core::ast::Placement;
///
/// assert_eq!(Placement::nearest(0.75).map(|p| (p.before(), p.after())), Some((3, 1)));
/// assert_eq!(Placement::nearest(0.5).map(|p| (p.before(), p.after())), Some((2, 2)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Placement {
    before: NonZeroU8,
    after: NonZeroU8,
}

impl Placement {
    /// The longest run `nearest` writes on one side.
    const LONGEST: u8 = 10;

    /// From the dash counts either side; `None` for one each (automatic).
    #[must_use]
    pub fn new(before: NonZeroU8, after: NonZeroU8) -> Option<Self> {
        (before.get() > 1 || after.get() > 1).then_some(Self { before, after })
    }

    /// The shortest arrow putting the label within 5% of `fraction` (0.0 at
    /// the actor, 1.0 at the target), as when dragging it. `None` only for
    /// a NaN.
    #[must_use]
    pub fn nearest(fraction: f64) -> Option<Self> {
        // One dash is the least on a side, so the ends are a tenth in
        let end = 1.0 / f64::from(Self::LONGEST);
        let fraction = fraction.clamp(end, 1.0 - end);
        (2..=Self::LONGEST)
            .flat_map(|total| (1..total).map(move |before| (before, total - before)))
            .filter_map(|(before, after)| {
                Self::new(NonZeroU8::new(before)?, NonZeroU8::new(after)?)
            })
            .find(|p| (p.fraction() - fraction).abs() <= 0.05 + f64::EPSILON)
    }

    /// Dashes before the label.
    #[must_use]
    pub const fn before(self) -> u8 {
        self.before.get()
    }

    /// Dashes after the label (counting the one in the arrowhead).
    #[must_use]
    pub const fn after(self) -> u8 {
        self.after.get()
    }

    /// How far along, from 0.0 (the actor) to 1.0 (the target).
    #[must_use]
    pub fn fraction(self) -> f64 {
        f64::from(self.before()) / (f64::from(self.before()) + f64::from(self.after()))
    }
}

/// `n` copies of `dash`.
fn write_run(f: &mut fmt::Formatter<'_>, dash: char, n: u8) -> fmt::Result {
    (0..n).try_for_each(|_| f.write_char(dash))
}

/// The dash counts either side of a label: `at`'s, or one each.
const fn runs(at: Option<Placement>) -> (u8, u8) {
    match at {
        Some(p) => (p.before(), p.after()),
        None => (1, 1),
    }
}

#[cfg(feature = "arbitrary")]
mod arbitrary_impls {
    use super::{
        Caveat, CertId, Document, Kind, Name, NonZeroU8, Placement, Region, Span, Spanned,
        Statement, Text,
    };
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

    impl<'a> Arbitrary<'a> for Placement {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            let before = NonZeroU8::new(u.int_in_range(1..=6)?).unwrap_or(NonZeroU8::MIN);
            let after = NonZeroU8::new(u.int_in_range(1..=6)?).unwrap_or(NonZeroU8::MIN);
            // One each is automatic, written as `None`; nudge it to 2:2
            Ok(Self::new(before, after).unwrap_or(Self {
                before: NonZeroU8::MIN.saturating_add(1),
                after: NonZeroU8::MIN.saturating_add(1),
            }))
        }
    }

    impl<'a> Arbitrary<'a> for Text {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            const ALPHABET: &[char] = &[
                'a', 'B', '1', ' ', '(', ')', '[', ']', ',', ':', '-', '>', '%', '/', '→', '✏',
                '\u{fe0f}',
            ];
            let raw = string_from(u, ALPHABET, 24)?;
            let raw = raw.replace("%%", "%");
            Ok(Self::new(raw.trim()).unwrap_or_else(|_| Self("note".into())))
        }
    }

    impl<'a> Arbitrary<'a> for Caveat {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            const ALPHABET: &[char] = &[
                'a', 'Z', '1', ' ', '<', '=', '(', ')', '.', ',', '"', '/', '→',
            ];
            let raw = string_from(u, ALPHABET, 16)?.replace("//", "/");
            Ok(Self::new(raw.trim()).unwrap_or_else(|_| Self(Text("x > 1".into()))))
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
            Self {
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
                regions: Vec::new(),
            }
            .with_regions(u)
        }
    }

    impl Document {
        /// Add up to three non-empty regions with distinct ids.
        fn with_regions(mut self, u: &mut Unstructured<'_>) -> Result<Self> {
            let len = self.statements.len();
            if len == 0 {
                return Ok(self);
            }
            for n in 0..u.int_in_range(0..=3_usize)? {
                let start = u.int_in_range(0..=len - 1)?;
                let end = u.int_in_range(start + 1..=len)?;
                let id = CertId::new(&alloc::format!("r{n}"))
                    .unwrap_or_else(|_| unreachable!("r0, r1, … are valid ids"));
                self.regions.push(spanned(Region {
                    id,
                    colour: Option::<Name>::arbitrary(u)?,
                    note: Option::<Text>::arbitrary(u)?,
                    statements: start..end,
                }));
            }
            // `regions` is in opening order, which is what parsing produces
            self.regions.sort_by_key(|r| r.node.statements.start);
            Ok(self)
        }
    }
}
