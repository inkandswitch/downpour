//! Rearranging source text by moving statements, as when dragging a row of
//! a rain diagram.
//!
//! [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/): "you can drag a certificate anywhere below what it
//! relies on and above what relies on it". [`Diagram::movable`] gives that
//! range for any event, and [`move_statement`] rewrites the source to match,
//! moving the whole line (note, trailing comment, and indentation included).
//!
//! ```
//! use rain_diagram_core::edit::move_statement;
//!
//! let source = "rain
//! (Doc)-[edit]->(Alice)
//! (Doc)-[read]->(Bob)     // independent of Alice's
//! (Alice)-[edit]->(Carol)";
//!
//! // Bob's certificate can go last; Carol's can't go above Alice's
//! assert_eq!(
//!     move_statement(source, 1, 2)?,
//!     "rain
//! (Doc)-[edit]->(Alice)
//! (Alice)-[edit]->(Carol)
//! (Doc)-[read]->(Bob)     // independent of Alice's"
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
/// let source = "rain\n  >>> A (Bob): Bob does stuff  // why\n(Bob)-[read]->(Carol)\n<<< A";
/// assert_eq!(
///     set_region_note(source, "A", "Bob and Carol")?,
///     "rain\n  >>> A (Bob): Bob and Carol  // why\n(Bob)-[read]->(Carol)\n<<< A"
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

/// Place a statement's label `percent` of the way along its arrow (or with
/// `None`, back to automatic), as when dragging it sideways. Only the
/// arrow's dashes change, to the shortest pattern within 5%; a note or
/// comment lined up in a column stays in that column if there's room.
///
/// ```
/// use rain_diagram_core::edit::set_position;
///
/// let source = "rain\n(Doc)-[edit]->(Alice)           : granted\n";
/// let placed = set_position(source, 0, Some(75))?;
/// assert_eq!(placed, "rain\n(Doc)---[edit]->(Alice)         : granted\n");
/// assert_eq!(set_position(&placed, 0, None)?, source);
/// # Ok::<(), rain_diagram_core::edit::PositionError>(())
/// ```
///
/// # Errors
///
/// [`PositionError::Source`] if `source` doesn't parse, and
/// [`PositionError::NoSuchStatement`] if `statement` is past the end.
/// Percentages past 100 count as 100.
pub fn set_position(
    source: &str,
    statement: usize,
    percent: Option<u16>,
) -> Result<String, PositionError> {
    let doc = crate::parser::document(source).map_err(|e| PositionError::Source(e.into()))?;
    let span = doc
        .statements
        .get(statement)
        .map(|s| s.span)
        .ok_or(PositionError::NoSuchStatement(statement))?;
    let text = source.get(span.start..).unwrap_or_default();
    // The statement's line, without its ending (`\n` or `\r\n`), which is
    // copied over untouched
    let line = text
        .find('\n')
        .map_or(text, |end| text.get(..end).unwrap_or_default());
    let line = line.strip_suffix('\r').unwrap_or(line);
    let arrow = source
        .get(span.start..span.end)
        .and_then(crate::parser::arrow)
        .ok_or(PositionError::NoSuchStatement(statement))?;
    let dash = line
        .get(arrow.before.start..)
        .and_then(|s| s.chars().next())
        .unwrap_or('-');

    let placement = percent.and_then(|p| crate::ast::Placement::nearest(f64::from(p) / 100.0));
    let (before, after) = placement.map_or((1, 1), |p| (p.before(), p.after()));
    let run = |n: u8| core::iter::repeat_n(dash, usize::from(n)).collect::<String>();
    let old = arrow.before.end - arrow.before.start + arrow.after.end - arrow.after.start;
    let grown = usize::from(before) + usize::from(after);

    // Keep whatever follows the body (a note or comment) in its column
    let body = line
        .get(arrow.after.end..arrow.body_end)
        .unwrap_or_default();
    let rest = line.get(arrow.body_end..).unwrap_or_default();
    let trailing = rest.trim_start();
    let gap = rest.len() - trailing.len();
    let gap = if trailing.is_empty() {
        gap
    } else {
        (gap + old).saturating_sub(grown).max(1)
    };

    let mut out = String::with_capacity(source.len() + grown);
    out.push_str(
        source
            .get(..span.start + arrow.before.start)
            .unwrap_or_default(),
    );
    out.push_str(&run(before));
    out.push_str(
        line.get(arrow.before.end..arrow.after.start)
            .unwrap_or_default(),
    );
    out.push_str(&run(after));
    out.push_str(body);
    out.extend(core::iter::repeat_n(' ', gap));
    out.push_str(trailing);
    out.push_str(source.get(span.start + line.len()..).unwrap_or_default());
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
    let text = with_newline(source);
    let (lines, starts) = lines(&text);
    let line_of = |offset: usize| starts.iter().rposition(|&s| s <= offset).unwrap_or(0);

    let open = line_of(found.span.start);
    let close =
        close_line(&lines, open, region).ok_or_else(|| RegionError::NoSuchRegion(region.into()))?;
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

/// Remove statement `n`, as when selecting it and pressing Delete. The
/// whole line goes (note and trailing comment included); if it was the only
/// statement in a region, the region's `>>>` and `<<<` lines go too.
///
/// What others route through just loses its effect (the diagram shows the
/// fallout), but something that names the statement can't be left dangling:
/// a revocation has to be deleted before the certificate it revokes.
///
/// ```
/// use rain_diagram_core::edit::{DeleteError, delete_statement};
///
/// let source = "rain\nda: (Doc)-[edit]->(Alice)\n>>> A: gone soon\n(Doc)-<revoke>-x[da]\n<<< A\n";
/// assert!(matches!(
///     delete_statement(source, 0),
///     Err(DeleteError::Revoked { by: 1, .. })
/// ));
/// assert_eq!(delete_statement(source, 1)?, "rain\nda: (Doc)-[edit]->(Alice)\n");
/// # Ok::<(), DeleteError>(())
/// ```
///
/// # Errors
///
/// - [`DeleteError::Source`] if `source` doesn't parse.
/// - [`DeleteError::NoSuchStatement`] if there's no statement `n`.
/// - [`DeleteError::Revoked`] if a revocation targets it.
/// - [`DeleteError::Result`] if the result doesn't parse.
pub fn delete_statement(source: &str, n: usize) -> Result<String, DeleteError> {
    let doc = crate::parser::document(source).map_err(|e| DeleteError::Source(e.into()))?;
    let d = crate::parse(source).map_err(DeleteError::Source)?;
    let ix = d.event_ix(n).ok_or(DeleteError::NoSuchStatement(n))?;
    // Labels are unambiguous, so only a revocation naming this one can dangle
    let revoker = d
        .event_ixs()
        .find(|&j| matches!(d.event(j), Some(Event::Revoke(r)) if r.target == ix));
    if let Some(by) = revoker {
        return Err(DeleteError::Revoked {
            statement: n,
            by: by.index(),
        });
    }

    let text = with_newline(source);
    let (lines, starts) = lines(&text);
    let line_of = |offset: usize| starts.iter().rposition(|&s| s <= offset).unwrap_or(0);
    let mut gone = Vec::from([d
        .events()
        .get(n)
        .map_or(usize::MAX, |e| line_of(e.span.start))]);
    // A region holding only this statement would be left empty
    let emptied = d
        .regions()
        .iter()
        .filter(|r| r.first == ix && r.last == ix)
        .filter_map(|r| doc.regions.iter().find(|s| s.node.id == r.id));
    for region in emptied {
        let open = line_of(region.span.start);
        gone.push(open);
        gone.extend(close_line(&lines, open, region.node.id.as_str()));
    }

    let mut out: String = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !gone.contains(i))
        .map(|(_, line)| *line)
        .collect();
    if !source.ends_with('\n') {
        out.pop();
    }

    crate::parse(&out).map_err(DeleteError::Result)?;
    Ok(out)
}

/// `source`, ending in a newline so every line does.
fn with_newline(source: &str) -> String {
    let mut text = String::from(source);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// `text`'s lines (newlines kept) and where each starts.
fn lines(text: &str) -> (Vec<&str>, Vec<usize>) {
    let mut at = 0;
    text.split_inclusive('\n')
        .map(|line| {
            let start = at;
            at += line.len();
            (line, start)
        })
        .unzip()
}

/// The line closing region `id`, searching from line `open`.
fn close_line(lines: &[&str], open: usize, id: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(open)
        .find(|(_, line)| {
            let code = crate::ast::comment_start(line)
                .and_then(|i| line.get(..i))
                .unwrap_or(line)
                .trim();
            code.strip_prefix("<<<")
                .is_some_and(|rest| rest.trim() == id)
        })
        .map(|(i, _)| i)
}

/// A statement to add by direct manipulation (dragging between lifelines).
/// Principals are columns; `target` is an event index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewStatement {
    /// `issuer` delegates to `audience`, over `subject` or, if that's
    /// `None`, over what it holds at that moment.
    Delegate {
        /// The issuer's column.
        issuer: usize,
        /// The audience's column.
        audience: usize,
        /// The subject's column (the issuer's own, for authority over
        /// itself), or `None` to pick what the issuer holds.
        subject: Option<usize>,
        /// The ability granted.
        ability: crate::ability::Ability,
    },

    /// `by` uses its authority on `on`.
    Invoke {
        /// The actor's column.
        by: usize,
        /// The subject's column.
        on: usize,
        /// The ability used.
        ability: crate::ability::Ability,
    },

    /// `by` revokes the certificate at event `target`.
    Revoke {
        /// The revoker's column.
        by: usize,
        /// The revoked certificate's event index.
        target: usize,
    },
}

/// Add a statement so it becomes statement `at` (a revocation goes no
/// earlier than just after its target), returning the new source and where
/// the statement landed.
///
/// A delegation is over its `subject`, or else over the subject its issuer
/// holds most strongly at that moment (or itself, if it holds nothing). It's
/// written short (`[edit]`) when
/// inference picks the same subject and in full (`[edit(Doc)]`) otherwise. A
/// revocation names its target by label, first labelling the target if it
/// has none (with its `issuer-audience` name, if that is free).
///
/// ```
/// use rain_diagram_core::{edit::{NewStatement, insert_statement}, level::Level};
///
/// let source = "rain\nprincipals: Doc, Alice, Bob\n(Doc)-[edit]->(Alice)\n";
/// // Alice drags to Bob: she passes on Read over the Doc she holds
/// let (out, at) = insert_statement(
///     source,
///     1,
///     NewStatement::Delegate { issuer: 1, audience: 2, subject: None, ability: Level::Read.into() },
/// )?;
/// assert_eq!(at, 1);
/// assert_eq!(
///     out,
///     "rain\nprincipals: Doc, Alice, Bob\n(Doc)-[edit]->(Alice)\n(Alice)-[read]->(Bob)\n"
/// );
/// # Ok::<(), rain_diagram_core::edit::InsertError>(())
/// ```
///
/// # Errors
///
/// [`InsertError::Source`] if `source` doesn't parse,
/// [`InsertError::NoSuchColumn`] / [`InsertError::NotACertificate`] for bad
/// indices, and [`InsertError::Result`] if the result doesn't parse.
pub fn insert_statement(
    source: &str,
    at: usize,
    new: NewStatement,
) -> Result<(String, usize), InsertError> {
    let d = crate::parse(source).map_err(InsertError::Source)?;
    let name = |column: usize| {
        d.principals()
            .get(column)
            .map(|p| p.name().as_str())
            .ok_or(InsertError::NoSuchColumn(column))
    };
    let at = at.min(d.events().len());

    let mut text = String::from(source);
    let (line, at) = match new {
        NewStatement::Delegate {
            issuer,
            audience,
            subject,
            ability,
        } => {
            let (from, to) = (name(issuer)?, name(audience)?);
            let chosen = subject.map(name).transpose()?;
            // What the issuer holds, preferring what covers the ability,
            // then the strongest level, just before this moment
            let held = at
                .checked_sub(1)
                .and_then(|i| d.event_ix(i))
                .map(|now| d.reach(crate::diagram::PrincipalIx::new(issuer), now))
                .unwrap_or_default();
            let subject = chosen.or_else(|| {
                held.iter()
                    .max_by_key(|r| {
                        (
                            r.ability.covers(&ability),
                            r.ability.level(),
                            core::cmp::Reverse(r.subject),
                        )
                    })
                    .and_then(|r| d.principal(r.subject))
                    .map(|p| p.name().as_str())
            });

            let short = alloc::format!("({from})-[{ability}]->({to})");
            let line = match subject {
                None => short,
                Some(subject) => {
                    // Short if inference lands on the same subject
                    let full = alloc::format!("({from})-[{ability}({subject})]->({to})");
                    let infers_same = crate::parse(&insert_line(&text, &d, at, &short))
                        .ok()
                        .and_then(|after| {
                            let ix = after.event_ix(at)?;
                            let cert = after.cert(ix)?;
                            Some(after.principal(cert.subject)?.name().as_str() == subject)
                        })
                        .unwrap_or(false);
                    if infers_same { short } else { full }
                }
            };
            (line, at)
        }

        NewStatement::Invoke { by, on, ability } => (
            alloc::format!("({})=[[{ability}]]=>({})", name(by)?, name(on)?),
            at,
        ),

        NewStatement::Revoke { by, target } => {
            let ix = d
                .event_ix(target)
                .ok_or(InsertError::NotACertificate(target))?;
            let cert = d.cert(ix).ok_or(InsertError::NotACertificate(target))?;
            let label = if let Some(label) = &cert.label {
                label.as_str().into()
            } else {
                // Only labelled certificates can be revoked, so label the
                // target first: its display name if no label has it yet, else
                // the first free `cN` (one of the first n+1 is free)
                let taken = |id: &str| {
                    d.event_ixs()
                        .filter_map(|i| d.cert(i))
                        .any(|c| c.label.as_ref().is_some_and(|l| l.as_str() == id))
                };
                let fresh = core::iter::once(cert.id.as_str().into())
                    .chain((1..=d.events().len() + 1).map(|n| alloc::format!("c{n}")))
                    .find(|id: &String| !taken(id))
                    .unwrap_or_else(|| "c".into());
                let start = d.events().get(target).map_or(0, |e| e.span.start);
                text.insert_str(start, &alloc::format!("{fresh}: "));
                fresh
            };
            (
                alloc::format!("({})-<revoke>-x[{label}]", name(by)?),
                at.max(target + 1),
            )
        }
    };

    let d = crate::parse(&text).map_err(InsertError::Source)?;
    let out = insert_line(&text, &d, at, &line);
    crate::parse(&out).map_err(InsertError::Result)?;
    Ok((out, at))
}

/// `line` as its own line, just before statement `at` (or after the last).
fn insert_line(source: &str, d: &Diagram, at: usize, line: &str) -> String {
    let line_start = |offset: usize| {
        source
            .get(..offset)
            .and_then(|s| s.rfind('\n'))
            .map_or(0, |i| i + 1)
    };
    let offset = match d.events().get(at) {
        Some(e) => line_start(e.span.start),
        None => source.len(),
    };
    let mut out = String::with_capacity(source.len() + line.len() + 2);
    out.push_str(source.get(..offset).unwrap_or_default());
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out.push_str(source.get(offset..).unwrap_or_default());
    out
}

/// Why a statement couldn't be added.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InsertError {
    /// The source doesn't parse.
    #[error("the source doesn't parse: {0}")]
    Source(InvalidDiagram),

    /// There's no such column.
    #[error("there's no column {0}")]
    NoSuchColumn(usize),

    /// The event to revoke isn't a certificate.
    #[error("event {0} isn't a certificate")]
    NotACertificate(usize),

    /// The result doesn't parse.
    #[error("that would break the diagram: {0}")]
    Result(InvalidDiagram),
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

    /// The new note isn't valid (it can't contain a comment marker or line breaks).
    #[error(transparent)]
    Note(#[from] crate::ast::InvalidText),
}

/// Why a statement couldn't be deleted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DeleteError {
    /// The source doesn't parse.
    #[error("the source doesn't parse: {0}")]
    Source(InvalidDiagram),

    /// There's no statement with that index.
    #[error("there's no statement {0}")]
    NoSuchStatement(usize),

    /// A revocation targets it.
    #[error("statement {by} revokes statement {statement}; delete that first")]
    Revoked {
        /// The statement to delete.
        statement: usize,

        /// The revocation naming it.
        by: usize,
    },

    /// The result doesn't parse.
    #[error("that would break the diagram: {0}")]
    Result(InvalidDiagram),
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
alice-bob: (Alice)-[edit(Doc)]->(Bob) : 2
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
alice-bob: (Alice)-[edit(Doc)]->(Bob) : 2
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
alice-bob: (Alice)-[edit(Doc)]->(Bob) : 2
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
    fn inserting_writes_whole_statements() -> TestResult {
        use crate::level::Level;

        // Alice holds two subjects, so the subject is written out
        let source = "rain\nprincipals: Essay, Commenters, Alice, Bob\n(Essay)-[admin]->(Alice)\n(Commenters)-[edit]->(Alice)\n";
        let (out, _) = insert_statement(
            source,
            2,
            NewStatement::Delegate {
                issuer: 2,
                audience: 3,
                subject: None,
                ability: Level::Read.into(),
            },
        )?;
        assert!(out.ends_with("(Alice)-[read(Essay)]->(Bob)\n"), "{out}");

        // A revocation dropped above its target goes just below it, and an
        // unlabelled target gets labelled first, by its display name
        let source = "rain\n(Alice)-[read]->(Bob)\n(Alice)-[edit]->(Bob)\n";
        let (out, at) = insert_statement(source, 0, NewStatement::Revoke { by: 0, target: 1 })?;
        assert_eq!(at, 2);
        assert_eq!(
            out,
            "rain\n(Alice)-[read]->(Bob)\nalice-bob: (Alice)-[edit]->(Bob)\n(Alice)-<revoke>-x[alice-bob]\n"
        );

        // …or by the first free `cN` when another certificate has that label
        let taken = "rain\nalice-bob: (Alice)-[read]->(Bob)\n(Alice)-[edit]->(Bob)\n";
        let (out, _) = insert_statement(taken, 2, NewStatement::Revoke { by: 0, target: 1 })?;
        assert!(
            out.ends_with("c1: (Alice)-[edit]->(Bob)\n(Alice)-<revoke>-x[c1]\n"),
            "{out}"
        );

        let (out, _) = insert_statement(
            source,
            1,
            NewStatement::Invoke {
                by: 1,
                on: 0,
                ability: Level::Read.into(),
            },
        )?;
        assert_eq!(
            out,
            "rain\n(Alice)-[read]->(Bob)\n(Bob)=[[read]]=>(Alice)\n(Alice)-[edit]->(Bob)\n"
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

    #[test]
    fn deleting_takes_lonely_regions_along() -> TestResult {
        // Statement 3 is not alone in A; statement 2 would be once 3 is gone
        assert_eq!(
            delete_statement(SOURCE, 3)?,
            SOURCE.replace("(Doc)-[read]->(Carol)            : 4\n", "")
        );
        let lonely = "rain\n(Doc)-[edit]->(Alice)\n  >>> A (Alice): hi  // note\n(Alice)-[read]->(Bob)\n<<< A\n(Doc)-[read]->(Bob)";
        assert_eq!(
            delete_statement(lonely, 1)?,
            "rain\n(Doc)-[edit]->(Alice)\n(Doc)-[read]->(Bob)"
        );
        Ok(())
    }

    #[test]
    fn deleting_refuses_to_leave_revocations_dangling() {
        assert!(matches!(
            delete_statement(SOURCE, 1),
            Err(DeleteError::Revoked {
                statement: 1,
                by: 4
            })
        ));
        assert!(matches!(
            delete_statement(SOURCE, 9),
            Err(DeleteError::NoSuchStatement(9))
        ));
        // Two Doc→Alice certificates; only the second is labelled, to be
        // revoked by. The first can go, the second can't
        let shared = "rain\n(Doc)-[read]->(Alice)\nc1: (Doc)-[edit]->(Alice)\n(Doc)-<revoke>-x[c1]";
        assert_eq!(
            delete_statement(shared, 0).as_deref(),
            Ok("rain\nc1: (Doc)-[edit]->(Alice)\n(Doc)-<revoke>-x[c1]")
        );
        assert!(matches!(
            delete_statement(shared, 1),
            Err(DeleteError::Revoked { by: 2, .. })
        ));
    }

    #[test]
    fn every_percentage_gets_a_short_arrow_within_five_percent() -> TestResult {
        let source = "rain\n(Doc)-[edit]->(Alice)    // why\n(Alice)=[[edit]]=>(Doc)";
        for percent in 0..=100_u16 {
            for statement in 0..2 {
                let placed = set_position(source, statement, Some(percent))?;
                let d = parse(&placed)?;
                let at = d
                    .event_ix(statement)
                    .and_then(|ix| match d.event(ix) {
                        Some(Event::Delegate(c)) => c.at,
                        Some(Event::Invoke(i)) => i.at,
                        Some(Event::Revoke(_)) | None => None,
                    })
                    .ok_or("placed")?;
                let wanted = f64::from(percent.clamp(10, 90)) / 100.0;
                assert!(
                    (at.fraction() - wanted).abs() <= 0.05 + f64::EPSILON,
                    "{percent}% → {placed}"
                );
                assert!(at.before() + at.after() <= 10, "{placed}");
                // The comment keeps its column while the arrow fits, so
                // putting it back is exact
                if at.before() + at.after() <= 5 {
                    assert_eq!(placed.find("//"), source.find("//"), "{placed}");
                    assert_eq!(set_position(&placed, statement, None)?, source);
                }
            }
        }
        Ok(())
    }
}
