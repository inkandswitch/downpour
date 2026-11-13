//! Unicode text art for rain diagrams, for code comments and READMEs.
//!
//! The layout follows the plain-text example in [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/). Every
//! event takes three lines: a header line where powerlines cross over, the
//! event itself, and a spacer.
//!
//! ```text
//!  (Doc)       (Alice)       (Bob)
//!    │╰┄┄┄╮ ╭┄┄┄┄┄┼┄┄┄┄╮       │
//!    ├──[edit]───▶│    ┆       │   The Doc grants Alice Edit
//!    │            │    ┆       │
//!    │            │    ┆       │
//!    │            ├──[edit]───▶│   Alice passes it to Bob
//!    │            │            │
//! ```
//!
//! Powerlines (`┆`) carry authority into a certificate's label from above.
//! The source certificate sends it over the top when the next holder's
//! lane is beyond its arrow, and out of the bottom otherwise, for example
//! to a revoker's `o` handle. Invocations sit under the certificate that
//! powers them. Labels placed by hand (`(A)---[edit]->(B)`) slide along
//! their arrow instead, clear of the lifelines they cross. A revocation's
//! line ends under its target, and a `✗` marks the target.

use crate::{
    ast::Placement,
    authority::Authority,
    diagram::{Diagram, Event, EventIx},
};
use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
use core::cmp::Ordering;

impl Diagram {
    /// Draw as Unicode text art.
    ///
    /// ```
    /// let diagram = rain_diagram_core::parse("rain\n(Doc)-[edit]->(Alice)")?;
    /// assert_eq!(
    ///     diagram.to_text(),
    ///     " (Doc)       (Alice)
    ///    │╰┄┄┄╮       │
    ///    ├──[edit]───▶│
    ///    │            │
    /// "
    /// );
    /// # Ok::<(), rain_diagram_core::Error>(())
    /// ```
    #[must_use]
    pub fn to_text(&self) -> String {
        Layout::new(self).draw()
    }
}

/// Minimum distance between lifelines; fits `├──[admin]──▶│`.
const MIN_SPACING: isize = 13;

/// Lines per event: header, event, spacer.
const ROW: isize = 3;

struct Layout<'a> {
    diagram: &'a Diagram,
    lifelines: Vec<isize>,
    labels: Vec<Option<Label>>,
    canvas: Canvas,

    /// Powerlines' vertical runs so far, so later ones find their own lane.
    lanes: Vec<Lane>,
}

/// A powerline's vertical run: column `x` from line `top` to `bottom`.
#[derive(Debug, Clone, Copy)]
struct Lane {
    x: isize,
    top: isize,
    bottom: isize,

    /// Where the powerline set off. Runs from one source may share a
    /// column, since a shared trunk that splits reads correctly.
    source: (isize, isize),
}

/// Where an event's label (or box) landed.
#[derive(Debug, Clone, Copy)]
struct Label {
    start: isize,
    end: isize,
    line: isize,

    /// Where a powerline enters from above.
    input: isize,

    /// Out of the bottom, for powerlines heading back across the arrow.
    bottom: isize,

    /// The arrow's horizontal extent.
    span: (isize, isize),

    /// Whether the arrow points left, which mirrors the label's ports.
    leftward: bool,
}

impl Label {
    /// Over the top: just right or left of the input, within the label.
    fn top(&self, towards: isize) -> isize {
        if towards > self.input {
            (self.input + 2).min(self.end)
        } else {
            (self.input - 2).max(self.start)
        }
    }

    const fn covers(&self, x: isize) -> bool {
        self.start <= x && x <= self.end
    }

    const fn width(&self) -> isize {
        self.end - self.start + 1
    }
}

/// What an invocation box is placed around: the label powering it (it
/// sits underneath, by default), lines crossing its row (it keeps clear),
/// and a placement by hand, which wins over sitting underneath.
#[derive(Debug, Clone, Copy)]
struct Around<'a> {
    powered_by: Option<Label>,
    crossing: &'a [isize],
    at: Option<Placement>,
}

/// Where a powerline is headed.
#[derive(Debug, Clone, Copy)]
enum Port {
    /// Into a label from above.
    Above { x: isize, line: isize },

    /// Into a lifeline handle from the side.
    Side { x: isize, line: isize },

    /// Straight down into an invocation box.
    Drop { x: isize, line: isize },
}

impl<'a> Layout<'a> {
    fn new(diagram: &'a Diagram) -> Self {
        let widths: Vec<isize> = diagram
            .principals()
            .iter()
            .map(|p| width(p.name().as_str()) + 2)
            .collect();
        // Wide enough for every name, and for every arrow's label (with
        // room for the arrow either side) shared across the columns it spans
        let labels = diagram.events().iter().filter_map(|e| match &e.node {
            Event::Delegate(c) => {
                Some((c.issuer, c.audience, width(&format!("{}", c.ability)) + 2))
            }
            Event::Invoke(i) => Some((i.by, i.on, width(&format!("{}", i.ability)) + 4)),
            Event::Revoke(_) => None,
        });
        let spacing = labels
            .map(|(a, b, w)| {
                let span = to_isize(a.index().abs_diff(b.index()).max(1));
                (w + 5 + span - 1) / span
            })
            .chain(widths.iter().map(|w| w + 3))
            .fold(MIN_SPACING, isize::max);
        let first = widths.first().map_or(1, |w| w / 2 + 1);

        Self {
            diagram,
            lifelines: (0..widths.len())
                .map(|i| first + spacing * to_isize(i))
                .collect(),
            labels: Vec::with_capacity(diagram.events().len()),
            canvas: Canvas::default(),
            lanes: Vec::new(),
        }
    }

    /// A column for a vertical run over lines `top..=bottom` from `source`,
    /// as near `preferred` as possible: not on a lifeline, not through
    /// another event's label, and not along another source's run (where the
    /// two would merge into one line). Falls back to `preferred`.
    fn lane(&self, preferred: isize, top: isize, bottom: isize, source: (isize, isize)) -> isize {
        // A trunk this source already runs into a label may carry on through
        // it: that reads as one line feeding both
        let feeds = |label: &Label, x: isize| {
            self.lanes
                .iter()
                .any(|l| l.source == source && l.x == x && l.bottom == label.line - 1)
        };
        let free = |x: isize| {
            // Off lifelines, and off the arrowheads and junctions beside them
            !self.lifelines.iter().any(|&l| (l - x).abs() <= 1)
                && !self
                    .labels
                    .iter()
                    .flatten()
                    .any(|l| top < l.line && l.line < bottom && l.covers(x) && !feeds(l, x))
                && !self.lanes.iter().any(|lane| {
                    lane.x == x && lane.source != source && lane.top <= bottom && top <= lane.bottom
                })
        };
        (0..=4)
            .flat_map(|d| [preferred + 2 * d, preferred - 2 * d])
            .find(|&x| free(x))
            .unwrap_or(preferred)
    }

    /// Draw a powerline from `start` that drops at `jog` to its lane, runs
    /// down to `bottom`, then crosses to `x` and continues to `end`. Without
    /// a conflict the lane is `preferred` and this is the plain path.
    fn run(
        &mut self,
        start: (isize, isize),
        jog: isize,
        preferred: isize,
        bottom: isize,
        (x, end): (isize, isize),
    ) {
        let lane = self.lane(preferred, jog, bottom, start);
        self.lanes.push(Lane {
            x: lane,
            top: jog,
            bottom,
            source: start,
        });
        let path = [
            start,
            (start.0, jog),
            (lane, jog),
            (lane, bottom),
            (x, bottom),
            (x, end),
        ];
        self.canvas.path(&path, Stroke::Dashed);
    }

    fn draw(mut self) -> String {
        let last = ROW * to_isize(self.diagram.events().len());

        for (principal, &x) in self.diagram.principals().iter().zip(&self.lifelines) {
            let name = format!("({})", principal.name());
            self.canvas.text(x - width(&name) / 2, 0, &name);
            self.canvas.path(&[(x, 0), (x, last)], Stroke::Solid);
        }

        for ix in self.diagram.event_ixs() {
            let label = self.event(ix);
            self.labels.push(label);
        }

        for ix in self.diagram.event_ixs() {
            self.powerlines(ix);
        }

        let notes = self.canvas.rightmost(1) + 4;
        self.regions(notes);
        for (row, event) in self.diagram.events().iter().enumerate() {
            let (note, caveat) = match &event.node {
                Event::Delegate(c) => (c.note.as_ref(), c.caveat.as_ref()),
                Event::Invoke(i) => (i.note.as_ref(), None),
                Event::Revoke(r) => (r.note.as_ref(), None),
            };
            // A caveat is part of what the certificate says, so it leads
            let text = match (note, caveat) {
                (Some(note), Some(caveat)) => format!("where {caveat}; {note}"),
                (None, Some(caveat)) => format!("where {caveat}"),
                (Some(note), None) => note.as_str().into(),
                (None, None) => continue,
            };
            let x = notes + 2 * self.depth(row);
            self.canvas.text(x, event_line(to_isize(row)), &text);
        }

        let art = self.canvas.render();
        match self.diagram.title() {
            Some(title) => format!("{title}\n\n{art}"),
            None => art,
        }
    }

    /// Regions live in the notes column, like an outline: the caption on
    /// the first row's header line, then a double bar down every line the
    /// region covers, with its rows' notes indented past it. The grid
    /// itself is untouched.
    ///
    /// ```text
    ///   │            │     Bob does stuff
    ///   ├──[edit]───▶│     ║ Bob gives Carol Edit
    ///   │            │     ║
    ///   │◀═[[edit]]══╡     ║ Carol edits the Doc
    ///   │            │     ║
    /// ```
    fn regions(&mut self, notes: isize) {
        let lanes = self.diagram.region_lanes();
        // Captions sharing a header line queue up along it
        let mut free: BTreeMap<isize, isize> = BTreeMap::new();

        for (region, &lane) in self.diagram.regions().iter().zip(&lanes) {
            let x = notes + 2 * to_isize(lane);
            let first = event_line(to_isize(region.first.index()));
            let last = event_line(to_isize(region.last.index()));
            self.canvas.path(&[(x, first), (x, last)], Stroke::Double);

            let header = first - 1;
            let at = free.entry(header).or_insert(x);
            let start = (*at).max(x);
            self.canvas.text(start, header, region.caption());
            *at = start + width(region.caption()) + 2;
        }
    }

    /// How many regions a row sits inside, counting overlapping lanes.
    fn depth(&self, row: usize) -> isize {
        self.diagram
            .regions()
            .iter()
            .zip(self.diagram.region_lanes())
            .filter(|(r, _)| r.first.index() <= row && row <= r.last.index())
            .map(|(_, lane)| to_isize(lane) + 1)
            .max()
            .unwrap_or(0)
    }

    fn x(&self, principal: crate::diagram::PrincipalIx) -> isize {
        self.lifelines
            .get(principal.index())
            .copied()
            .unwrap_or_default()
    }

    fn label(&self, ix: EventIx) -> Option<Label> {
        self.labels.get(ix.index()).copied().flatten()
    }

    /// Draw the event's own row, returning where its label went.
    fn event(&mut self, ix: EventIx) -> Option<Label> {
        let line = event_line(to_isize(ix.index()));
        match self.diagram.event(ix)? {
            Event::Delegate(cert) => {
                let text = format!("[{}]", cert.ability);
                Some(self.arrow(
                    self.x(cert.issuer),
                    self.x(cert.audience),
                    line,
                    &text,
                    cert.at,
                ))
            }

            Event::Invoke(invoke) => {
                let text = format!("[[{}]]", invoke.ability);
                let powered_by = invoke
                    .authority
                    .route()
                    .and_then(|route| self.label(route.held()));
                // Later revocations of earlier certificates draw their line
                // up through this row; the box mustn't hide it
                let crossing: Vec<isize> = self
                    .diagram
                    .event_ixs()
                    .filter(|&later| later > ix)
                    .filter_map(|later| match self.diagram.event(later) {
                        Some(Event::Revoke(r)) if r.target < ix => self.label(r.target),
                        _ => None,
                    })
                    .map(|target| target.input)
                    .collect();
                Some(self.invocation(
                    self.x(invoke.by),
                    self.x(invoke.on),
                    line,
                    &text,
                    Around {
                        powered_by,
                        crossing: &crossing,
                        at: invoke.at,
                    },
                ))
            }

            Event::Revoke(revoke) => {
                let target = self.label(revoke.target)?;
                let x = self.x(revoke.by);
                let w = width("<revoke>");
                // Under the target, unless slid along the line by hand; the
                // line then carries on to the target's column
                let start = revoke.at.map_or(target.input - 3, |p| {
                    let (lo, hi) = if x < target.input {
                        (x + 2, target.input - 1)
                    } else {
                        (target.input + 1 - w + 1, x - 1 - w)
                    };
                    self.placed(x, target.input, w, p.fraction(), (lo, hi))
                });
                let label = self.boxed(start, line, "<revoke>", (x.min(start), x.max(start)));

                // From the revoker to the label, then on past it to the
                // target's column if the label was slid short of it
                let t = target.input;
                if x < label.start {
                    self.canvas
                        .path(&[(x, line), (label.start - 1, line)], Stroke::Solid);
                    if t > label.end {
                        self.canvas
                            .path(&[(label.end + 1, line), (t, line)], Stroke::Solid);
                    }
                } else if x > label.end {
                    self.canvas
                        .path(&[(label.end + 1, line), (x, line)], Stroke::Solid);
                    if t < label.start {
                        self.canvas
                            .path(&[(t, line), (label.start - 1, line)], Stroke::Solid);
                    }
                }
                // A revocation that has no effect (the revoker can't revoke
                // this) mustn't look like it cut anything: no power arrives
                // at the revoker, and nothing reaches up to the target
                if revoke.authority.is_authorized() {
                    self.canvas.text(x, line, "o");
                    let under = target.line + 1;
                    self.canvas.path(
                        &[(target.input, under), (target.input, line)],
                        Stroke::Solid,
                    );
                    self.canvas.text(target.input, under, "✗");
                }
                Some(label)
            }
        }
    }

    /// Where a `w`-wide label starts when placed `fraction` of the way from
    /// column `from` to `to`: within `lo..=hi`, and off any lifeline it
    /// would hide, nearest first.
    fn placed(
        &self,
        from: isize,
        to: isize,
        w: isize,
        fraction: f64,
        (lo, hi): (isize, isize),
    ) -> isize {
        let hi = hi.max(lo);
        let centre = from + round(to_f64(to - from) * fraction);
        let preferred = (centre - w / 2).clamp(lo, hi);
        let (a, b) = (from.min(to), from.max(to));
        let clear = |s: isize| {
            !self
                .lifelines
                .iter()
                .any(|&x| a < x && x < b && s <= x && x < s + w)
        };
        (0..=hi - lo)
            .flat_map(|d| [preferred - d, preferred + d])
            .find(|&s| (lo..=hi).contains(&s) && clear(s))
            .unwrap_or(preferred)
    }

    /// A delegation arrow: `├──[edit]───▶│` or `│◀───[edit]──┤`, with the
    /// label slid along it if placed by hand (`(A)---[edit]->(B)`).
    fn arrow(
        &mut self,
        from: isize,
        to: isize,
        line: isize,
        text: &str,
        at: Option<Placement>,
    ) -> Label {
        let w = width(text);
        if to < from {
            // At least one dash after `◀` and before the issuer's lifeline
            let end = at.map_or(from - 3, |p| {
                self.placed(from, to, w, p.fraction(), (to + 3, from - 1 - w)) + w - 1
            });
            self.canvas
                .path(&[(to + 1, line), (from, line)], Stroke::Solid);
            self.canvas.text(to + 1, line, "◀");
            let label = self.boxed(end - w + 1, line, text, (to, from));
            Label {
                // Within even the shortest label, `[/]`
                input: (end - 2).max(end - w + 2),
                bottom: (end - 3).max(end - w + 1),
                leftward: true,
                ..label
            }
        } else {
            let start = match at {
                Some(p) if to != from => {
                    self.placed(from, to, w, p.fraction(), (from + 2, to - 2 - w))
                }
                _ => from + 3,
            };
            let tip = if to == from { start - 1 } else { to - 1 };
            self.canvas
                .path(&[(from, line), (tip, line)], Stroke::Solid);
            if to != from {
                self.canvas.text(tip, line, "▶");
            }
            self.boxed(start, line, text, (from, to))
        }
    }

    /// An invocation: `│◀════[[edit]]═══╡`, centred under what powers it.
    fn invocation(
        &mut self,
        by: isize,
        on: isize,
        line: isize,
        text: &str,
        Around {
            powered_by,
            crossing,
            at,
        }: Around<'_>,
    ) -> Label {
        let w = width(text);
        let (low, high, default) = match on.cmp(&by) {
            Ordering::Less => (on + 3, by - 1 - w, by - 3 - w),
            Ordering::Greater => (by + 2, on - 2 - w, by + 3),
            Ordering::Equal => (by + 2, by + 2, by + 2),
        };
        // Placed by hand, or else under what powers it
        let preferred = match at {
            Some(p) => by + round(to_f64(on - by) * p.fraction()) - w / 2,
            None => powered_by.map_or(default, |p| p.start + (p.width() - w).div_euclid(2)),
        };
        let preferred = preferred.min(high).max(low);

        // Slide clear of lines crossing this row (revocations, and the
        // lifelines the double line passes over), nearest first
        let lifelines: Vec<isize> = self
            .lifelines
            .iter()
            .copied()
            .filter(|&x| on.min(by) < x && x < on.max(by))
            .collect();
        let clear = |start: isize| {
            !crossing
                .iter()
                .chain(&lifelines)
                .any(|&x| start <= x && x < start + w)
        };
        let start = (0..=high.saturating_sub(low).max(0))
            .flat_map(|d| [preferred - d, preferred + d])
            .find(|&s| (low..=high).contains(&s) && clear(s))
            .unwrap_or(preferred);

        match on.cmp(&by) {
            Ordering::Less => {
                self.canvas
                    .path(&[(on + 1, line), (by, line)], Stroke::Double);
                self.canvas.text(on + 1, line, "◀");
            }
            Ordering::Greater => {
                self.canvas
                    .path(&[(by, line), (on - 1, line)], Stroke::Double);
                self.canvas.text(on - 1, line, "▶");
            }
            Ordering::Equal => {
                self.canvas
                    .path(&[(by, line), (start - 1, line)], Stroke::Double);
            }
        }

        let mut label = self.boxed(start, line, text, (on.min(by), on.max(by)));
        // Powerlines drop in where they left the powering label, if that's
        // above the box
        label.input = powered_by
            .map(|p| p.input)
            .filter(|&x| label.covers(x))
            .unwrap_or(label.start + w / 2);
        label
    }

    /// Write `text` and describe it as a left-to-right label.
    fn boxed(&mut self, start: isize, line: isize, text: &str, span: (isize, isize)) -> Label {
        let end = start + width(text) - 1;
        self.canvas.text(start, line, text);
        Label {
            start,
            end,
            line,
            // Within even the shortest label, `[/]`
            input: (start + 2).min(end - 1),
            bottom: (start + 3).min(end),
            span,
            leftward: false,
        }
    }

    fn powerlines(&mut self, ix: EventIx) {
        let Some(event) = self.diagram.event(ix) else {
            return;
        };
        let Some(label) = self.label(ix) else {
            return;
        };

        let port = match event {
            Event::Delegate(_) => Port::Above {
                x: label.input,
                line: label.line,
            },
            Event::Invoke(_) => Port::Drop {
                x: label.input,
                line: label.line,
            },
            Event::Revoke(revoke) => Port::Side {
                x: self.x(revoke.by),
                line: label.line,
            },
        };

        match event.authority() {
            Authority::Route(route) => {
                let mut from = route.source();
                // The last handle the authority passed through, if any: what
                // comes next is supplied from it, not from that certificate
                let mut handle_at = None;
                for &handle in route.through() {
                    // Authority that arrives after the membership was signed
                    // joins on the member's lifeline, just below where it
                    // comes from, so power never runs back up the page
                    let member = self.diagram.cert(handle).map(|c| c.audience);
                    if let (true, Some(member), Some(source)) =
                        (from > handle, member, self.label(from))
                    {
                        let (x, line) = (self.x(member), source.line + 1);
                        let port = Port::Side { x, line };
                        match handle_at {
                            Some(point) => self.connect_point(point, port),
                            None => self.connect(from, port),
                        }
                        self.canvas.text(x, line, "o");
                        handle_at = Some((x, line));
                    } else if let Some(through) = self.label(handle) {
                        let x = if through.leftward {
                            through.start - 1
                        } else {
                            through.end + 1
                        };
                        match handle_at {
                            Some((hx, hline)) => self.connect_point(
                                (hx, hline),
                                Port::Above {
                                    x,
                                    line: through.line,
                                },
                            ),
                            None => self.connect(
                                from,
                                Port::Above {
                                    x,
                                    line: through.line,
                                },
                            ),
                        }
                        self.canvas.text(x, through.line, "o");
                        handle_at = Some((x, through.line));
                    }
                    from = handle;
                }
                match handle_at {
                    Some(point) => self.connect_point(point, port),
                    None => self.connect(from, port),
                }
            }

            // Authority over yourself starts on your own lifeline: off your
            // head on the first row, from an `o` on the lifeline after that
            // (like the SVG's circle)
            Authority::Root => {
                if let (Event::Delegate(cert), Port::Above { x, line }) = (event, port) {
                    let lifeline = self.x(cert.issuer);
                    let header = line - 1;
                    if ix.index() == 0 {
                        let (peel, glyph) = if x < lifeline {
                            (lifeline - 1, "╯")
                        } else {
                            (lifeline + 1, "╰")
                        };
                        self.canvas
                            .path(&[(peel, header), (x, header), (x, line)], Stroke::Dashed);
                        self.canvas.text(peel, header, glyph);
                    } else {
                        self.canvas.path(
                            &[(lifeline, header), (x, header), (x, line)],
                            Stroke::Dashed,
                        );
                        self.canvas.text(lifeline, header, "o");
                    }
                }
            }

            Authority::Unauthorized => {}
        }
    }

    /// A powerline out of a transitive handle's `o` at `(hx, hline)`: down
    /// from it, then across and into wherever it's going.
    fn connect_point(&mut self, (hx, hline): (isize, isize), to: Port) {
        let start = (hx, hline);
        match to {
            Port::Above { x, line } | Port::Drop { x, line } => {
                self.run(start, hline + 1, hx, line - 1, (x, line));
            }
            Port::Side { x, line } => self.run(start, hline + 1, hx, line, (x, line)),
        }
    }

    fn connect(&mut self, from: EventIx, to: Port) {
        let Some(source) = self.label(from) else {
            return;
        };
        let line = source.line;
        let below = line + 1;

        match to {
            // Over the top, then down the target's own column
            Port::Above { x, line: target } if x < source.span.0 || x > source.span.1 => {
                let exit = source.top(x);
                self.run((exit, line), line - 1, x, target - 1, (x, target));
            }
            Port::Above { x, line: target } => {
                let exit = source.bottom;
                self.run((exit, line), below, exit, target - 1, (x, target));
            }
            Port::Drop { x, line: target } => {
                let exit = source.input;
                self.run((exit, line), below, exit, target - 1, (x, target));
            }
            // Straight into the handle; its `o` covers the last cell
            Port::Side { x, line: target } => {
                let exit = source.bottom;
                self.run((exit, line), below, exit, target, (x, target));
            }
        }
    }
}

const fn event_line(row: isize) -> isize {
    ROW * row + 2
}

#[allow(clippy::cast_precision_loss, reason = "columns are small")]
const fn to_f64(n: isize) -> f64 {
    n as f64
}

/// Nearest whole column.
#[allow(clippy::cast_possible_truncation, reason = "columns are small")]
fn round(x: f64) -> isize {
    // `f64::round` needs std; this is `no_std`
    if x >= 0.0 {
        (x + 0.5) as isize
    } else {
        (x - 0.5) as isize
    }
}

fn to_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// How a cell side is drawn. Later variants win when strokes overlap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Stroke {
    #[default]
    None,
    Dashed,
    Double,
    Solid,
}

#[derive(Debug, Clone, Default)]
struct Cell {
    north: Stroke,
    east: Stroke,
    south: Stroke,
    west: Stroke,
    ink: Ink,
}

#[derive(Debug, Clone, Default)]
enum Ink {
    /// Draw whatever lines meet here.
    #[default]
    Lines,

    /// Text, with any zero-width characters that follow it.
    Text(String),

    /// The right half of a wide character.
    Covered,
}

/// A sparse grid of cells, addressed `(x, line)`. Columns may go negative;
/// rendering shifts everything right if they do.
#[derive(Debug, Default)]
struct Canvas {
    cells: BTreeMap<(isize, isize), Cell>,
}

impl Canvas {
    fn cell(&mut self, x: isize, line: isize) -> &mut Cell {
        self.cells.entry((line, x)).or_default()
    }

    /// Connect each pair of consecutive points with a straight run. Points
    /// must share a row or column.
    fn path(&mut self, points: &[(isize, isize)], stroke: Stroke) {
        for pair in points.windows(2) {
            let [(x0, y0), (x1, y1)] = *pair else {
                continue;
            };
            let (dx, dy) = ((x1 - x0).signum(), (y1 - y0).signum());
            let (mut x, mut y) = (x0, y0);
            while (x, y) != (x1, y1) {
                let (nx, ny) = (x + dx, y + dy);
                let here = self.cell(x, y);
                let there_side = match (dx, dy) {
                    (1, _) => {
                        here.east = here.east.max(stroke);
                        &mut self.cell(nx, ny).west
                    }
                    (-1, _) => {
                        here.west = here.west.max(stroke);
                        &mut self.cell(nx, ny).east
                    }
                    (_, 1) => {
                        here.south = here.south.max(stroke);
                        &mut self.cell(nx, ny).north
                    }
                    _ => {
                        here.north = here.north.max(stroke);
                        &mut self.cell(nx, ny).south
                    }
                };
                *there_side = (*there_side).max(stroke);
                (x, y) = (nx, ny);
            }
        }
    }

    fn text(&mut self, x: isize, line: isize, text: &str) {
        let mut at = x;
        for c in text.chars() {
            match char_width(c) {
                0 => {
                    if let Some(Cell {
                        ink: Ink::Text(s), ..
                    }) = self.cells.get_mut(&(line, at - 1))
                    {
                        s.push(c);
                    }
                }
                w => {
                    self.cell(at, line).ink = Ink::Text(String::from(c));
                    if w == 2 {
                        self.cell(at + 1, line).ink = Ink::Covered;
                    }
                    at += w;
                }
            }
        }
    }

    /// The rightmost column with anything visible, from `line` down.
    fn rightmost(&self, line: isize) -> isize {
        self.cells
            .iter()
            .filter(|((y, _), cell)| *y >= line && cell.glyph() != Some(' '))
            .map(|((_, x), _)| *x)
            .max()
            .unwrap_or_default()
    }

    fn render(&self) -> String {
        let shift = self
            .cells
            .keys()
            .map(|&(_, x)| x)
            .min()
            .unwrap_or_default()
            .min(0);

        let mut out = String::new();
        let mut lines = self.cells.iter().peekable();
        while let Some(&(&(y, _), _)) = lines.peek() {
            let mut row = String::new();
            let mut column = shift;
            while let Some(((_, x), cell)) = lines.next_if(|((line, _), _)| *line == y) {
                for _ in column..*x {
                    row.push(' ');
                }
                match &cell.ink {
                    Ink::Text(s) => row.push_str(s),
                    Ink::Covered => {}
                    Ink::Lines => row.push(cell.glyph().unwrap_or(' ')),
                }
                column = x + 1;
            }
            out.push_str(row.trim_end());
            out.push('\n');
        }
        out
    }
}

impl Cell {
    /// The box-drawing character for the lines meeting here, if this cell
    /// is lines rather than text.
    fn glyph(&self) -> Option<char> {
        use Stroke::{Dashed, Double, None, Solid};

        if !matches!(self.ink, Ink::Lines) {
            return Option::None;
        }

        let vertical = self.north.max(self.south);
        let horizontal = self.west.max(self.east);
        let sides = (
            self.north != None,
            self.east != None,
            self.south != None,
            self.west != None,
        );

        Some(match sides {
            (false, false, false, false) => ' ',
            // Vertical double lines are region bars; horizontal ones invocations
            (_, false, _, false) => match vertical {
                Solid => '│',
                Double => '║',
                Dashed | None => '┆',
            },
            (false, _, false, _) => match horizontal {
                Solid => '─',
                Double => '═',
                Dashed | None => '┄',
            },
            (false, true, true, false) => '╭',
            (false, false, true, true) => '╮',
            (true, true, false, false) => '╰',
            (true, false, false, true) => '╯',
            (true, true, true, true) => {
                if horizontal == Double {
                    '╪'
                } else {
                    '┼'
                }
            }
            (true, true, true, false) => {
                if horizontal == Double {
                    '╞'
                } else {
                    '├'
                }
            }
            (true, false, true, true) => {
                if horizontal == Double {
                    '╡'
                } else {
                    '┤'
                }
            }
            (false, true, true, true) => '┬',
            (true, true, false, true) => '┴',
        })
    }
}

/// Display width in a monospace terminal: a small approximation of East
/// Asian Width that covers combining marks, CJK, and emoji.
const fn char_width(c: char) -> isize {
    match c as u32 {
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF => 0,
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

fn width(s: &str) -> isize {
    s.chars().map(char_width).sum()
}
