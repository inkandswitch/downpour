//! Rearranging source text by moving statements, as when dragging a row of
//! a rain diagram.
//!
//! Keyhive notebook 08: "you can drag a certificate anywhere below what it
//! relies on and above what relies on it". [`Diagram::movable`] gives that
//! range for any event, and [`move_statement`] rewrites the source to match,
//! moving the whole line (note, trailing comment, and indentation included).
//!
//! ```
//! use rain_diagram_core::edit::move_statement;
//!
//! let source = "rain
//! (Doc)-[edit]->(Alice)
//! (Doc)-[read]->(Bob)     %% independent of Alice's
//! (Alice)-[edit]->(Carol)";
//!
//! // Bob's certificate can go last; Carol's can't go above Alice's
//! assert_eq!(
//!     move_statement(source, 1, 2)?,
//!     "rain
//! (Doc)-[edit]->(Alice)
//! (Alice)-[edit]->(Carol)
//! (Doc)-[read]->(Bob)     %% independent of Alice's"
//! );
//! assert!(move_statement(source, 2, 0).is_err());
//! # Ok::<(), rain_diagram_core::edit::MoveError>(())
//! ```

use crate::{
    Error as InvalidDiagram,
    diagram::{Diagram, Event, EventIx},
};
use alloc::{string::String, vec::Vec};
use core::{fmt::Write as _, ops::RangeInclusive};
use thiserror::Error;

impl Diagram {
    /// The rows `ix` can move to: below everything it relies on directly
    /// (its route's certificates, a revocation's target) and above everything
    /// that relies on it directly. Always includes `ix` itself.
    #[must_use]
    pub fn movable(&self, ix: EventIx) -> RangeInclusive<usize> {
        let i = ix.index();
        let lowest = self
            .uses(ix)
            .map(|dep| dep.index() + 1)
            .max()
            .unwrap_or(0)
            .min(i);
        let highest = self
            .event_ixs()
            .filter(|&later| later > ix && self.uses(later).any(|dep| dep == ix))
            .map(|later| later.index() - 1)
            .min()
            .unwrap_or_else(|| self.events().len().saturating_sub(1))
            .max(i);
        lowest..=highest
    }

    /// The events `ix` refers to directly.
    fn uses(&self, ix: EventIx) -> impl Iterator<Item = EventIx> + '_ {
        let event = self.event(ix);
        let route = event
            .and_then(|e| e.authority().route())
            .into_iter()
            .flat_map(crate::authority::Route::certs);
        let target = match event {
            Some(Event::Revoke(r)) => Some(r.target),
            Some(Event::Delegate(_) | Event::Invoke(_)) | None => None,
        };
        route.chain(target)
    }
}

/// Move the `from`th statement so it becomes the `to`th, and return the new
/// source.
///
/// The line lands next to its new neighbour: just after it when moving down,
/// just before it when moving up, so it joins whatever region that neighbour
/// is in.
///
/// # Errors
///
/// - [`MoveError::Source`] if `source` doesn't parse to begin with.
/// - [`MoveError::OutOfRange`] if `to` is outside [`Diagram::movable`] (or
///   either index is past the end).
/// - [`MoveError::Result`] if the moved source doesn't parse, for example
///   because it left a region empty.
pub fn move_statement(source: &str, from: usize, to: usize) -> Result<String, MoveError> {
    let diagram = crate::parse(source).map_err(MoveError::Source)?;
    let allowed = diagram
        .event_ix(from)
        .map(|ix| diagram.movable(ix))
        .ok_or(MoveError::NoSuchStatement(from))?;
    if !allowed.contains(&to) {
        return Err(MoveError::OutOfRange { from, to, allowed });
    }
    if from == to {
        return Ok(source.into());
    }

    // Work on text ending in a newline, so every line does
    let ends_with_newline = source.ends_with('\n');
    let mut text = String::from(source);
    if !ends_with_newline {
        text.push('\n');
    }

    let line = |n: usize| {
        let span = diagram.events().get(n).map(|e| e.span)?;
        let start = text.get(..span.start)?.rfind('\n').map_or(0, |i| i + 1);
        let end = span.end + text.get(span.end..)?.find('\n')? + 1;
        Some((start, end))
    };
    let ((start, end), (to_start, to_end)) = line(from)
        .zip(line(to))
        .ok_or(MoveError::OutOfRange { from, to, allowed })?;
    let moved = text.get(start..end).unwrap_or_default();

    let mut out = String::with_capacity(text.len());
    if to > from {
        // Down: after the neighbour's line
        out.push_str(text.get(..start).unwrap_or_default());
        out.push_str(text.get(end..to_end).unwrap_or_default());
        out.push_str(moved);
        out.push_str(text.get(to_end..).unwrap_or_default());
    } else {
        // Up: before the neighbour's line
        out.push_str(text.get(..to_start).unwrap_or_default());
        out.push_str(moved);
        out.push_str(text.get(to_start..start).unwrap_or_default());
        out.push_str(text.get(end..).unwrap_or_default());
    }
    if !ends_with_newline {
        out.pop();
    }

    crate::parse(&out).map_err(MoveError::Result)?;
    Ok(out)
}

/// Change a region's note (its caption), as when editing it in place.
/// The region's name, colour, indentation, and any trailing comment are
/// kept; an empty `note` removes it, so the caption shows the name.
///
/// ```
/// use rain_diagram_core::edit::set_region_note;
///
/// let source = "rain\n  >>> A (Bob): Bob does stuff  %% why\n(Bob)-[read]->(Carol)\n<<< A";
/// assert_eq!(
///     set_region_note(source, "A", "Bob and Carol")?,
///     "rain\n  >>> A (Bob): Bob and Carol  %% why\n(Bob)-[read]->(Carol)\n<<< A"
/// );
/// # Ok::<(), rain_diagram_core::edit::NoteError>(())
/// ```
///
/// # Errors
///
/// [`NoteError::Source`] if `source` doesn't parse, [`NoteError::NoSuchRegion`]
/// if no region has that name, and [`NoteError::Note`] if `note` isn't a
/// valid one-line note.
pub fn set_region_note(source: &str, region: &str, note: &str) -> Result<String, NoteError> {
    let doc = crate::parser::document(source).map_err(|e| NoteError::Source(e.into()))?;
    let found = doc
        .regions
        .iter()
        .find(|r| r.node.id.as_str() == region)
        .ok_or_else(|| NoteError::NoSuchRegion(region.into()))?;

    let note = note.trim();
    let note = if note.is_empty() {
        None
    } else {
        Some(crate::ast::Text::new(note)?)
    };

    let mut line = alloc::format!(">>> {}", found.node.id);
    if let Some(colour) = &found.node.colour {
        let _ = write!(line, " ({colour})");
    }
    if let Some(note) = &note {
        let _ = write!(line, ": {note}");
    }

    let mut out = String::with_capacity(source.len() + line.len());
    out.push_str(source.get(..found.span.start).unwrap_or_default());
    out.push_str(&line);
    out.push_str(source.get(found.span.end..).unwrap_or_default());
    crate::parse(&out).map_err(NoteError::Source)?;
    Ok(out)
}

/// Set (or with `None`, remove) where a statement's label sits along its
/// arrow, as when dragging it sideways. Only the ` @60%` changes; when the
/// note was lined up in a column, it stays in that column if there's room.
///
/// ```
/// use rain_diagram_core::edit::set_position;
///
/// let source = "rain\n(Doc)-[edit]->(Alice)           : granted\n";
/// let placed = set_position(source, 0, Some(60))?;
/// assert_eq!(placed, "rain\n(Doc)-[edit]->(Alice) @60%      : granted\n");
/// assert_eq!(set_position(&placed, 0, None)?, source);
/// # Ok::<(), rain_diagram_core::edit::PositionError>(())
/// ```
///
/// # Errors
///
/// [`PositionError::Source`] if `source` doesn't parse,
/// [`PositionError::NoSuchStatement`] if `statement` is past the end, and
/// [`PositionError::Percent`] if `percent` is over 100.
pub fn set_position(
    source: &str,
    statement: usize,
    percent: Option<u16>,
) -> Result<String, PositionError> {
    let at = percent.map(crate::ast::Percent::new).transpose()?;
    let doc = crate::parser::document(source).map_err(|e| PositionError::Source(e.into()))?;
    let span = doc
        .statements
        .get(statement)
        .map(|s| s.span)
        .ok_or(PositionError::NoSuchStatement(statement))?;
    let text = source.get(span.start..span.end).unwrap_or_default();
    let body = crate::parser::tail_start(text).ok_or(PositionError::NoSuchStatement(statement))?;

    // What follows the body: maybe an old `@N%`, then maybe `: note`
    let rest = text.get(body..).unwrap_or_default();
    let after_token = match rest.trim_start().strip_prefix('@') {
        Some(token) => token.split_once('%').map_or("", |(_, after)| after),
        None => rest,
    };
    let note = after_token.trim_start();
    let width = rest.len() - note.len();

    let token = at.map_or_else(String::new, |at| alloc::format!(" @{at}"));
    let mut tail = token.clone();
    if !note.is_empty() {
        for _ in 0..width.saturating_sub(token.len()).max(1) {
            tail.push(' ');
        }
        tail.push_str(note);
    }

    let mut out = String::with_capacity(source.len() + token.len());
    out.push_str(source.get(..span.start + body).unwrap_or_default());
    out.push_str(&tail);
    out.push_str(source.get(span.end..).unwrap_or_default());
    crate::parse(&out).map_err(PositionError::Source)?;
    Ok(out)
}

/// Why a statement's position couldn't change.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PositionError {
    /// The source doesn't parse.
    #[error("the source doesn't parse: {0}")]
    Source(InvalidDiagram),

    /// There's no statement with that index.
    #[error("there's no statement {0}")]
    NoSuchStatement(usize),

    /// Past the end of the arrow.
    #[error(transparent)]
    Percent(#[from] crate::ast::InvalidPercent),
}

/// Make region `region` cover statements `first..=last`, as when dragging
/// its edges. Its `>>>` and `<<<` lines move whole (colour, note,
/// indentation, and comments come along): the opening line goes just before
/// statement `first`, the closing line just after statement `last`.
///
/// ```
/// use rain_diagram_core::edit::set_region_rows;
///
/// let source = "rain\n>>> A: here\n(A)-[read]->(B)\n<<< A\n(B)-[read]->(C)\n";
/// assert_eq!(
///     set_region_rows(source, "A", 0, 1)?,
///     "rain\n>>> A: here\n(A)-[read]->(B)\n(B)-[read]->(C)\n<<< A\n"
/// );
/// # Ok::<(), rain_diagram_core::edit::RegionError>(())
/// ```
///
/// # Errors
///
/// [`RegionError::Source`] if `source` doesn't parse, [`RegionError::NoSuchRegion`]
/// for an unknown name, [`RegionError::OutOfRange`] unless
/// `first <= last < statements`, and [`RegionError::Result`] if the result
/// doesn't parse.
pub fn set_region_rows(
    source: &str,
    region: &str,
    first: usize,
    last: usize,
) -> Result<String, RegionError> {
    let doc = crate::parser::document(source).map_err(|e| RegionError::Source(e.into()))?;
    let found = doc
        .regions
        .iter()
        .find(|r| r.node.id.as_str() == region)
        .ok_or_else(|| RegionError::NoSuchRegion(region.into()))?;
    if first > last || last >= doc.statements.len() {
        return Err(RegionError::OutOfRange { first, last });
    }

    // Work line by line, on text ending in a newline
    let ends_with_newline = source.ends_with('\n');
    let mut text = String::from(source);
    if !ends_with_newline {
        text.push('\n');
    }
    let mut starts = Vec::new();
    let mut at = 0;
    let lines: Vec<&str> = text
        .split_inclusive('\n')
        .inspect(|line| {
            starts.push(at);
            at += line.len();
        })
        .collect();
    let line_of = |offset: usize| starts.iter().rposition(|&s| s <= offset).unwrap_or(0);

    let open = line_of(found.span.start);
    let close = lines
        .iter()
        .enumerate()
        .skip(open)
        .find(|(_, line)| {
            let code = crate::ast::comment_start(line)
                .and_then(|i| line.get(..i))
                .unwrap_or(line)
                .trim();
            code.strip_prefix("<<<")
                .is_some_and(|id| id.trim() == region)
        })
        .map(|(i, _)| i)
        .ok_or_else(|| RegionError::NoSuchRegion(region.into()))?;
    let statement_line = |i: usize| doc.statements.get(i).map(|s| line_of(s.span.start));
    let (before, after) = statement_line(first)
        .zip(statement_line(last))
        .ok_or(RegionError::OutOfRange { first, last })?;

    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        if i == open || i == close {
            continue;
        }
        if i == before {
            out.push_str(lines.get(open).copied().unwrap_or_default());
        }
        out.push_str(line);
        if i == after {
            out.push_str(lines.get(close).copied().unwrap_or_default());
        }
    }
    if !ends_with_newline {
        out.pop();
    }

    crate::parse(&out).map_err(RegionError::Result)?;
    Ok(out)
}

/// Move the principal in column `from` to column `to`, as when dragging its
/// lifeline, by writing the full column order into the `principals:` line
/// (replacing one that's there, or adding one after the header and title).
///
/// ```
/// use rain_diagram_core::edit::move_principal;
///
/// let source = "rain\n(Doc)-[edit]->(Alice)\n(Alice)-[edit]->(Bob)\n";
/// assert_eq!(
///     move_principal(source, 2, 0)?,
///     "rain\nprincipals: Bob, Doc, Alice\n(Doc)-[edit]->(Alice)\n(Alice)-[edit]->(Bob)\n"
/// );
/// # Ok::<(), rain_diagram_core::edit::PrincipalError>(())
/// ```
///
/// # Errors
///
/// [`PrincipalError::Source`] if `source` doesn't parse, and
/// [`PrincipalError::NoSuchColumn`] if `from` or `to` is past the last column.
pub fn move_principal(source: &str, from: usize, to: usize) -> Result<String, PrincipalError> {
    let diagram = crate::parse(source).map_err(PrincipalError::Source)?;
    let mut order: Vec<&str> = diagram
        .principals()
        .iter()
        .map(|p| p.name().as_str())
        .collect();
    if from >= order.len() || to >= order.len() {
        return Err(PrincipalError::NoSuchColumn(from.max(to)));
    }
    let moved = order.remove(from);
    order.insert(to, moved);
    let line = alloc::format!("principals: {}", order.join(", "));

    let doc = crate::parser::document(source).map_err(|e| PrincipalError::Source(e.into()))?;
    let line_start = |offset: usize| {
        source
            .get(..offset)
            .and_then(|s| s.rfind('\n'))
            .map_or(0, |i| i + 1)
    };
    let mut out = String::with_capacity(source.len() + line.len() + 1);

    if let (Some(first), Some(last)) = (doc.participants.first(), doc.participants.last()) {
        // Replace the directive's code, keeping indentation and any comment
        let start = line_start(first.span.start);
        let start = start
            + source
                .get(start..)
                .map_or(0, |s| s.len() - s.trim_start().len());
        let end = last.span.end;
        out.push_str(source.get(..start).unwrap_or_default());
        out.push_str(&line);
        out.push_str(source.get(end..).unwrap_or_default());
    } else {
        // After the title if there is one, else after the header
        let after = doc
            .title
            .as_ref()
            .map(|t| t.span.end)
            .or_else(|| {
                let header = crate::parser::header_end(source)?;
                Some(header)
            })
            .unwrap_or(0);
        let at = source
            .get(after..)
            .and_then(|s| s.find('\n'))
            .map_or(source.len(), |i| after + i + 1);
        out.push_str(source.get(..at).unwrap_or_default());
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
        out.push('\n');
        out.push_str(source.get(at..).unwrap_or_default());
    }

    crate::parse(&out).map_err(PrincipalError::Source)?;
    Ok(out)
}

/// Why a principal couldn't move.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrincipalError {
    /// The source doesn't parse (before or after).
    #[error("the source doesn't parse: {0}")]
    Source(InvalidDiagram),

    /// There's no such column.
    #[error("there's no column {0}")]
    NoSuchColumn(usize),
}

/// Why a region couldn't change which rows it covers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegionError {
    /// The source doesn't parse.
    #[error("the source doesn't parse: {0}")]
    Source(InvalidDiagram),

    /// No region has that name.
    #[error("there's no region called `{0}`")]
    NoSuchRegion(String),

    /// Not a range of statements.
    #[error("statements {first}..={last} aren't a range in this diagram")]
    OutOfRange {
        /// The first statement asked for.
        first: usize,

        /// The last statement asked for.
        last: usize,
    },

    /// The result doesn't parse.
    #[error("that would break the diagram: {0}")]
    Result(InvalidDiagram),
}

/// Why a region's note couldn't change.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NoteError {
    /// The source doesn't parse.
    #[error("the source doesn't parse: {0}")]
    Source(InvalidDiagram),

    /// No region has that name.
    #[error("there's no region called `{0}`")]
    NoSuchRegion(String),

    /// The new note isn't valid (it can't contain `%%` or line breaks).
    #[error(transparent)]
    Note(#[from] crate::ast::InvalidText),
}

/// Why a statement couldn't move.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MoveError {
    /// The original source doesn't parse.
    #[error("the source doesn't parse: {0}")]
    Source(InvalidDiagram),

    /// There's no statement with that index.
    #[error("there's no statement {0}")]
    NoSuchStatement(usize),

    /// The destination is above something it relies on, or below something
    /// that relies on it.
    #[error("statement {from} can only move to rows {}..={}, not {to}", .allowed.start(), .allowed.end())]
    OutOfRange {
        /// The statement being moved.
        from: usize,

        /// Where it was asked to go.
        to: usize,

        /// Where it may go.
        allowed: RangeInclusive<usize>,
    },

    /// The moved source doesn't parse (for example, a region became empty).
    #[error("moving it there breaks the diagram: {0}")]
    Result(InvalidDiagram),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use testresult::TestResult;

    const SOURCE: &str = "rain
(Doc)-[edit]->(Alice)            : 1
(Alice)-[edit(Doc)]->(Bob)       : 2
>>> A: Bob does stuff
(Bob)=[[edit]]=>(Doc)            : 3
(Doc)-[read]->(Carol)            : 4
<<< A
(Alice)-<revoke>-x[alice-bob]    : 5
";

    fn range(d: &Diagram, i: usize) -> RangeInclusive<usize> {
        d.movable(d.event_ix(i).expect("in range"))
    }

    #[test]
    fn movable_between_dependencies_and_dependents() -> TestResult {
        let d = parse(SOURCE)?;
        assert_eq!(range(&d, 0), 0..=0); // Alice→Bob relies on it straight after
        assert_eq!(range(&d, 1), 1..=1); // relies on 0; Bob's edit relies on it
        assert_eq!(range(&d, 2), 2..=4); // after Alice→Bob; nothing relies on it
        assert_eq!(range(&d, 3), 0..=4); // the Doc's own, nothing relies on it
        assert_eq!(range(&d, 4), 2..=4); // after its target, Alice→Bob
        Ok(())
    }

    #[test]
    fn moving_carries_the_whole_line_into_the_neighbours_region() -> TestResult {
        // Up: before its new neighbour, outside the region
        assert_eq!(
            move_statement(SOURCE, 3, 0)?,
            "rain
(Doc)-[read]->(Carol)            : 4
(Doc)-[edit]->(Alice)            : 1
(Alice)-[edit(Doc)]->(Bob)       : 2
>>> A: Bob does stuff
(Bob)=[[edit]]=>(Doc)            : 3
<<< A
(Alice)-<revoke>-x[alice-bob]    : 5
"
        );

        // Down: after its new neighbour, into the region
        assert_eq!(
            move_statement(SOURCE, 4, 3)?,
            "rain
(Doc)-[edit]->(Alice)            : 1
(Alice)-[edit(Doc)]->(Bob)       : 2
>>> A: Bob does stuff
(Bob)=[[edit]]=>(Doc)            : 3
(Alice)-<revoke>-x[alice-bob]    : 5
(Doc)-[read]->(Carol)            : 4
<<< A
"
        );
        Ok(())
    }

    #[test]
    fn moving_principals_rewrites_the_order_in_place() -> TestResult {
        let source = "rain\ntitle T\n  participant Doc, Alice  // order\n(Doc)-[edit]->(Alice)\n(Alice)-[edit]->(Bob)";
        assert_eq!(
            move_principal(source, 0, 2)?,
            "rain\ntitle T\n  principals: Alice, Bob, Doc  // order\n(Doc)-[edit]->(Alice)\n(Alice)-[edit]->(Bob)"
        );
        assert!(matches!(
            move_principal(source, 0, 9),
            Err(PrincipalError::NoSuchColumn(9))
        ));
        Ok(())
    }

    #[test]
    fn refuses_to_break_things() {
        assert!(matches!(
            move_statement(SOURCE, 2, 0),
            Err(MoveError::OutOfRange { .. })
        ));
        assert!(matches!(
            move_statement(SOURCE, 9, 0),
            Err(MoveError::NoSuchStatement(9))
        ));
        assert!(matches!(
            move_statement("nonsense", 0, 0),
            Err(MoveError::Source(_))
        ));
        // Moving Bob's edit out would leave region A holding only Carol's
        // certificate, which is fine; moving both out would empty it
        let lonely = "rain\n(Doc)-[edit]->(Alice)\n>>> A\n(Doc)-[read]->(Bob)\n<<< A";
        assert!(matches!(
            move_statement(lonely, 1, 0),
            Err(MoveError::Result(_))
        ));
    }
}
