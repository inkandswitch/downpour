//! Rain diagrams: lifelines falling from each principal, one row per event.
//!
//! The picture follows [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/):
//!
//! - Principals head lifelines.
//! - Certificates are arrows in the issuer's colour, labelled with a box
//!   tinted in the subject's colour, placed just off the issuer's lifeline.
//! - Powerlines (dashed, in the subject's colour) leave the top of the
//!   certificate that carries authority, arch up and over, and drop into the
//!   top of the next certificate's label. Authority over yourself starts at
//!   a small circle on your own lifeline (or from your head, on the first
//!   row).
//! - Transitive handles are circles on a membership arrow: the subject's
//!   powerline runs into the circle and out the other side.
//! - Invocations are double lines whose box sits right under the certificate
//!   that powers them, with the powerline dropping straight in.
//! - Revocations are diamonds right under the revoked certificate, with a
//!   red line from the revoker and another pointing back up to cross the
//!   certificate out.
//! - The dotted line at the bottom is "now".

use super::{
    FONT, MARGIN, PILL, REVOKE, STYLE, caveat_text, colour, describe, escape, index, note, pill,
    status_class, text_width, title_width,
};
use crate::{
    authority::Authority,
    diagram::{Cert, Diagram, Event, EventIx, Invoke, PrincipalIx, Revoke},
};
use alloc::{format, string::String, vec::Vec};
use core::fmt::Write as _;

/// Draw `diagram` as a rain diagram, with the first `happened` events in the
/// past (the "now" line sits below them) and the rest still to come.
pub(super) fn render(diagram: &Diagram, happened: usize) -> String {
    let mut svg = Svg::new(diagram);
    svg.happened = happened.min(diagram.events().len());
    svg.draw()
}

const HEADER: f64 = 64.0;
const HEAD_RY: f64 = 16.0;
const ROW: f64 = 64.0;
/// How far a powerline arches above the label it leaves.
const ARCH: f64 = 34.0;
const HANDLE: f64 = 5.0;
const DIAMOND: (f64, f64) = (46.0, 14.0);

struct Svg<'a> {
    diagram: &'a Diagram,
    spacing: f64,
    title: f64,
    out: String,
    labels: Vec<Option<Pill>>,

    /// Transitive handles, drawn last so certificate boxes don't cover them.
    handles: String,

    /// Handles drawn so far, by spot and colour: routes of one colour share
    /// a handle, and each new colour gets the next bead along.
    beads: Vec<(Spot, &'static str)>,

    /// How many events are above the "now" line.
    happened: usize,
}

/// Where an event's label (or diamond) landed.
#[derive(Debug, Clone, Copy)]
struct Pill {
    x: f64,
    y: f64,
    half: f64,

    /// The arrow's direction: `1.0` rightwards, `-1.0` leftwards.
    dir: f64,
}

impl Pill {
    const fn top(&self) -> (f64, f64) {
        (self.x, self.y - PILL / 2.0)
    }

    const fn bottom(&self) -> (f64, f64) {
        (self.x, self.y + PILL / 2.0)
    }

    /// Where a transitive handle sits: on the certificate box itself, on the
    /// edge facing the audience (as in [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/)'s "Admin○→").
    /// Later beads (`n` > 0) string out along the arrow from there.
    fn handle(&self, n: usize) -> (f64, f64) {
        #[allow(clippy::cast_precision_loss, reason = "a handful of beads")]
        let along = n as f64 * (2.0 * HANDLE + 2.0);
        (self.x + self.dir * (self.half + along), self.y)
    }
}

/// Where a transitive handle sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spot {
    /// On the membership certificate's box.
    Membership(EventIx),

    /// On the member's lifeline just below event `after`, for authority
    /// that reaches the membership only after it was signed.
    Lifeline { membership: EventIx, after: EventIx },
}

impl<'a> Svg<'a> {
    fn new(diagram: &'a Diagram) -> Self {
        let widest = diagram
            .principals()
            .iter()
            .map(|p| text_width(p.name().as_str()))
            .fold(0.0, f64::max);

        Self {
            diagram,
            spacing: (widest + 48.0).max(140.0),
            title: if diagram.title().is_some() { 32.0 } else { 0.0 },
            out: String::new(),
            labels: Vec::new(),
            handles: String::new(),
            beads: Vec::new(),
            happened: diagram.events().len(),
        }
    }

    /// Whether `ix` is below the "now" line.
    const fn future(&self, ix: EventIx) -> bool {
        ix.index() >= self.happened
    }

    /// The powerline class for a line into `to`.
    /// `ix`'s status class as of the "now" line (`future` below it).
    fn status(&self, ix: EventIx) -> &'static str {
        if self.future(ix) {
            return "future";
        }
        self.happened
            .checked_sub(1)
            .and_then(|i| self.diagram.event_ix(i))
            .and_then(|now| self.diagram.status(ix, now))
            .map_or("valid", status_class)
    }

    /// A powerline takes the status of the event it powers, so authority
    /// stops visibly flowing into anything void or revoked.
    fn powerline_class(&self, to: EventIx) -> String {
        format!("powerline {}", self.status(to))
    }

    fn lifeline(&self, p: PrincipalIx) -> f64 {
        MARGIN + self.spacing / 2.0 + self.spacing * index(p.index())
    }

    fn head(&self) -> f64 {
        self.title + HEADER / 2.0 - 4.0
    }

    fn row(&self, ix: EventIx) -> f64 {
        self.title + HEADER + ROW * (index(ix.index()) + 0.5)
    }

    fn draw(mut self) -> String {
        let d = self.diagram;
        let bottom = self.title + HEADER + ROW * index(d.events().len()) + MARGIN / 2.0;
        let notes_x = MARGIN + self.spacing * index(d.principals().len()) + 8.0;
        let notes_width = d
            .events()
            .iter()
            .filter_map(|e| note(&e.node))
            .map(text_width)
            .fold(0.0, f64::max);
        let width = (notes_x + notes_width + MARGIN).max(title_width(d));
        let height = bottom + MARGIN / 2.0;

        let _ = write!(
            self.out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" class="rain-diagram" data-rows-top="{rows_top:.1}" data-row-height="{ROW}" data-events="{events}" data-moment="{moment}" viewBox="0 0 {width:.0} {height:.0}" width="{width:.0}" height="{height:.0}" font-family="system-ui, sans-serif" font-size="{FONT}">"#,
            rows_top = self.title + HEADER,
            events = self.diagram.events().len(),
            moment = self.happened,
        );
        self.out.push_str(STYLE);
        if let Some(title) = d.title() {
            let _ = write!(
                self.out,
                r#"<text class="title" x="{MARGIN}" y="{y}">{}</text>"#,
                escape(title.as_str()),
                y = MARGIN + 4.0
            );
        }

        // Lifelines first, then regions over them, so captions aren't crossed
        // out (the box tint is translucent, so lifelines still show through)
        self.principals(bottom);

        self.regions(width);

        // "Now": everything above it has happened. Its handle opens a
        // certflow snapshot of that moment (in the interactive embed)
        let _ = write!(
            self.out,
            concat!(
                r#"<g class="now" data-moment="{moment}">"#,
                r#"<rect class="hit" x="{MARGIN}" y="{hy:.1}" width="{hw:.1}" height="20"/>"#,
                r#"<line class="now-line" x1="{MARGIN}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}"/>"#,
                r#"<g class="now-handle"><circle class="hit" cx="{hx:.1}" cy="{y:.1}" r="11"/>"#,
                r#"<text x="{hx:.1}" y="{y:.1}" text-anchor="middle" dominant-baseline="central">{clock}</text></g>"#,
                "<title>Now: everything above this line has happened</title>",
                "</g>"
            ),
            moment = self.happened,
            MARGIN = MARGIN,
            y = self.title + HEADER + ROW * index(self.happened) - 2.0,
            x2 = notes_x - 32.0,
            hx = notes_x - 18.0,
            hy = self.title + HEADER + ROW * index(self.happened) - 12.0,
            hw = notes_x - 32.0 - MARGIN,
            clock = clock(self.happened),
        );

        // Labels first (later events need to know where earlier ones went),
        // then powerlines underneath, then the events on top
        let mut events = String::new();
        for ix in d.event_ixs() {
            let mut group = String::new();
            let label = self.event(ix, &mut group);
            self.labels.push(label);
            events.push_str(&group);
        }

        self.out.push_str(r#"<g class="powerlines">"#);
        for ix in d.event_ixs() {
            self.powerlines(ix);
        }
        self.out.push_str("</g>");
        self.out.push_str(&events);
        // Handles on top: they sit on certificate boxes
        self.out.push_str(r#"<g class="handles">"#);
        let handles = core::mem::take(&mut self.handles);
        self.out.push_str(&handles);
        self.out.push_str("</g>");

        self.out.push_str(r#"<g class="notes">"#);
        for ix in d.event_ixs() {
            if let Some(text) = d.event(ix).and_then(note) {
                let _ = write!(
                    self.out,
                    r#"<text x="{notes_x:.1}" y="{:.1}" dominant-baseline="middle">{}</text>"#,
                    self.row(ix),
                    escape(text)
                );
            }
        }
        self.out.push_str("</g></svg>");
        self.out
    }

    /// Region boxes over the lifelines, each with invisible grips on its top
    /// and bottom edges for dragging them to other rows.
    fn regions(&mut self, width: f64) {
        let d = self.diagram;
        let lanes = d.region_lanes();
        for (n, (region, &lane)) in d.regions().iter().zip(&lanes).enumerate() {
            let inset = 5.0 * index(lane);
            let top = self.title + HEADER + ROW * index(region.first.index());
            let bottom = self.title + HEADER + ROW * index(region.last.index() + 1);
            super::region(
                &mut self.out,
                n,
                region,
                (
                    MARGIN / 2.0 + inset,
                    top + 3.0 + inset,
                    width - MARGIN / 2.0 - inset,
                    bottom - 3.0 - inset,
                ),
            );
            // Invisible grips on the top and bottom edges, for dragging
            // them to other rows in the interactive embed
            for (edge, y) in [("top", top + 3.0 + inset), ("bottom", bottom - 3.0 - inset)] {
                let _ = write!(
                    self.out,
                    r#"<rect class="hit region-edge" data-region="{id}" data-edge="{edge}" data-first="{first}" data-last="{last}" x="{x:.1}" y="{y:.1}" width="{w:.1}" height="10"/>"#,
                    id = escape(region.id.as_str()),
                    first = region.first.index(),
                    last = region.last.index(),
                    x = MARGIN / 2.0 + inset,
                    y = y - 5.0,
                    w = width - MARGIN - 2.0 * inset,
                );
            }
        }
    }
    fn principals(&mut self, bottom: f64) {
        let y = self.head();
        self.out.push_str(r#"<g class="principals">"#);
        for (i, principal) in self.diagram.principals().iter().enumerate() {
            let p = PrincipalIx::new(i);
            let x = self.lifeline(p);
            let colour = colour(p);
            let _ = write!(
                self.out,
                concat!(
                    r#"<g class="principal" data-principal="{key}" data-x="{x:.1}">"#,
                    r#"<line class="lifeline" x1="{x:.1}" y1="{top:.1}" x2="{x:.1}" y2="{bottom:.1}" stroke="{colour}"/>"#,
                    r#"<rect class="hit lifeline-hit" data-column="{i}" x="{hx:.1}" y="{top:.1}" width="18" height="{h:.1}"/>"#,
                    r#"<ellipse class="head" cx="{x:.1}" cy="{y:.1}" rx="{rx:.1}" ry="{HEAD_RY}" stroke="{colour}"/>"#,
                    r#"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" dominant-baseline="central">{name}</text>"#,
                    "</g>"
                ),
                key = escape(principal.key()),
                x = x,
                y = y,
                top = y + HEAD_RY,
                bottom = bottom,
                colour = colour,
                rx = text_width(principal.name().as_str()) / 2.0 + 16.0,
                HEAD_RY = HEAD_RY,
                i = i,
                hx = x - 9.0,
                h = bottom - y - HEAD_RY,
                name = escape(principal.name().as_str()),
            );
        }
        self.out.push_str("</g>");
    }

    fn event(&self, ix: EventIx, out: &mut String) -> Option<Pill> {
        // As of the "now" line; below it, nothing has happened yet
        let status = self.status(ix);

        match self.diagram.event(ix)? {
            Event::Delegate(cert) => Some(self.delegation(ix, cert, status, out)),
            Event::Invoke(invoke) => Some(self.invocation(ix, invoke, status, out)),
            Event::Revoke(revoke) => self.revocation(ix, revoke, status, out),
        }
    }

    fn delegation(&self, ix: EventIx, cert: &Cert, status: &str, out: &mut String) -> Pill {
        let (issuer, audience, subject) = (cert.issuer, cert.audience, cert.subject);
        let (from, to) = (self.lifeline(issuer), self.lifeline(audience));
        let y = self.row(ix);
        let text = &cert.ability.label();
        let half = text_width(text) / 2.0 + 10.0;
        let dir = if to < from { -1.0 } else { 1.0 };
        // Just off the issuer's lifeline, unless the arrow is too short
        // Just off the issuer's lifeline (unless the arrow is too short), or
        // where it was placed by hand, kept within the arrow
        let offset = (half + 16.0).min((to - from).abs() / 2.0);
        let (low, high) = (from.min(to) + half + 4.0, from.max(to) - half - 4.0);
        let x = match cert.at {
            Some(p) if low <= high => (from + (to - from) * p.fraction()).clamp(low, high),
            _ => from + dir * offset,
        };
        let rect = Pill { x, y, half, dir };

        let _ = write!(
            out,
            concat!(
                r#"<g class="event delegate {status}" data-event="{ix}" data-cert="{id}" data-span="{from:.1} {to:.1}">{hit}"#,
                r#"<line x1="{from:.1}" y1="{y:.1}" x2="{to:.1}" y2="{y:.1}" stroke="{colour}"/>"#,
                "{head}{pill}{caveat}",
                "<title>{title}</title>",
                "</g>"
            ),
            status = status,
            ix = ix.index(),
            caveat = caveat_text(rect.x, y, cert.caveat.as_ref()),
            hit = hit(from, to, y),
            id = escape(cert.id.as_str()),
            from = from,
            to = to,
            y = y,
            colour = colour(issuer),
            head = chevron(to, y, dir, colour(issuer)),
            pill = pill(rect.x, rect.y, rect.half, text, colour(subject), false),
            title = escape(&describe(self.diagram, ix)),
        );
        rect
    }

    fn invocation(&self, ix: EventIx, invoke: &Invoke, status: &str, out: &mut String) -> Pill {
        let (by, on) = (self.lifeline(invoke.by), self.lifeline(invoke.on));
        let y = self.row(ix);
        let text = &invoke.ability.label();
        let half = text_width(text) / 2.0 + 13.0;
        let dir = if on < by { -1.0 } else { 1.0 };

        // Right under the certificate that powers it, when that's between
        // the two lifelines
        let (low, high) = (by.min(on) + half + 10.0, by.max(on) - half - 10.0);
        let powered = invoke
            .authority
            .route()
            .and_then(|r| self.label(r.held()))
            .map_or(by + dir * (half + 16.0), |p| p.x);
        // A hand-placed position wins over sitting under its power
        let preferred = invoke.at.map_or(powered, |p| by + (on - by) * p.fraction());
        let x = if low <= high {
            preferred.clamp(low, high)
        } else {
            f64::midpoint(by, on)
        };
        let rect = Pill { x, y, half, dir };
        let stroke = colour(invoke.by);

        let _ = write!(
            out,
            concat!(
                r#"<g class="event invoke {status}" data-event="{ix}" data-span="{by:.1} {on:.1}">{hit}"#,
                r#"<line class="double" x1="{by:.1}" y1="{y:.1}" x2="{on:.1}" y2="{y:.1}" stroke="{colour}"/>"#,
                r#"<line class="double-inner" x1="{by:.1}" y1="{y:.1}" x2="{on:.1}" y2="{y:.1}"/>"#,
                "{head}{pill}",
                "<title>{title}</title>",
                "</g>"
            ),
            status = status,
            ix = ix.index(),
            title = escape(&describe(self.diagram, ix)),
            hit = hit(by, on, y),
            by = by,
            on = on,
            y = y,
            colour = stroke,
            head = chevron(on, y, dir, stroke),
            pill = pill(rect.x, rect.y, rect.half, text, colour(invoke.on), true),
        );
        rect
    }

    fn revocation(
        &self,
        ix: EventIx,
        revoke: &Revoke,
        status: &str,
        out: &mut String,
    ) -> Option<Pill> {
        let target = self.label(revoke.target)?;
        let x = self.lifeline(revoke.by);
        let y = self.row(ix);
        let (rx, ry) = DIAMOND;
        let tx = target.x;
        // Right under the target, unless placed along the red line by hand
        // kept clear of the revoker's lifeline, as delegation labels are
        let cx = revoke.at.map_or(tx, |p| {
            let along = x + (tx - x) * p.fraction();
            let (low, high) = (x.min(tx) + rx + 4.0, x.max(tx) - rx - 4.0);
            if low <= high {
                along.clamp(low, high)
            } else {
                along
            }
        });
        let near = if x < cx { cx - rx } else { cx + rx };
        let under = target.y + PILL / 2.0;
        let up = under + 2.0;
        let rest = if (cx - tx).abs() < 0.5 {
            format!("M{cx:.1},{:.1} V{up:.1}", y - ry)
        } else {
            let far = if tx > cx { cx + rx } else { cx - rx };
            format!("M{far:.1},{y:.1} H{tx:.1} V{up:.1}")
        };

        // A revocation with no effect mustn't look like it cut anything: no
        // cross on the target, and a tooltip saying why
        let (cross, title) = if revoke.authority.is_authorized() {
            (
                format!(
                    r#"<path class="cross" d="M{a:.1},{b:.1} L{c:.1},{e:.1} M{a:.1},{e:.1} L{c:.1},{b:.1}"/>"#,
                    a = tx - 6.0,
                    c = tx + 6.0,
                    b = under + 2.0,
                    e = under + 14.0,
                ),
                String::new(),
            )
        } else {
            let d = self.diagram;
            let name = d.principal(revoke.by).map_or("", |p| p.name().as_str());
            let cert = d.cert(revoke.target).map_or("", |c| c.id.as_str());
            (
                String::new(),
                format!(
                    "<title>No effect: {} can't revoke {}</title>",
                    escape(name),
                    escape(cert)
                ),
            )
        };

        let _ = write!(
            out,
            concat!(
                r#"<g class="event revoke {status}" data-event="{ix}" data-span="{x:.1} {tx:.1}">{hit}"#,
                r#"<path class="revocation" d="M{x:.1},{y:.1} H{near:.1} {rest}"/>"#,
                "{cross}",
                r#"<path class="diamond" d="M{cx:.1},{dt:.1} L{dr:.1},{y:.1} L{cx:.1},{db:.1} L{dl:.1},{y:.1} Z"/>"#,
                r#"<text x="{cx:.1}" y="{y:.1}" text-anchor="middle" dominant-baseline="central">✂️ Revoke</text>"#,
                "{title}</g>"
            ),
            status = status,
            ix = ix.index(),
            hit = hit(x, tx, y),
            cross = cross,
            title = title,
            x = x,
            y = y,
            near = near,
            rest = rest,
            tx = tx,
            cx = cx,
            dt = y - ry,
            db = y + ry,
            dl = cx - rx,
            dr = cx + rx,
        );
        Some(Pill {
            x: cx,
            y,
            half: rx,
            dir: if x < cx { 1.0 } else { -1.0 },
        })
    }

    /// Place (and, the first time, draw) the handle where a `colour` route
    /// from `from` passes through membership `handle` (labelled `h`).
    ///
    /// Authority that arrives after the membership was signed joins on the
    /// member's lifeline, just below where it comes from, so power never
    /// runs back up the page. Its tooltip names the membership.
    fn handle(
        &mut self,
        from: EventIx,
        handle: EventIx,
        h: Pill,
        colour: &'static str,
    ) -> (f64, f64) {
        let d = self.diagram;
        let late = d
            .cert(handle)
            .map(|c| c.audience)
            .zip(self.label(from))
            .filter(|_| from > handle);
        let (spot, close) = if late.is_some() {
            let name = d.cert(handle).map_or("", |c| c.id.as_str());
            (
                Spot::Lifeline {
                    membership: handle,
                    after: from,
                },
                alloc::format!("><title>through {}</title></circle>", escape(name)),
            )
        } else {
            (Spot::Membership(handle), String::from("/>"))
        };

        let (n, new) = self.bead(spot, colour);
        let (hx, hy) = if let Some((member, source)) = late {
            #[allow(clippy::cast_precision_loss, reason = "a handful of beads")]
            let beside = n as f64 * (2.0 * HANDLE + 2.0);
            (self.lifeline(member) + beside, source.y + ROW / 2.0)
        } else {
            h.handle(n)
        };
        if new {
            let _ = write!(
                self.handles,
                r#"<circle class="handle" cx="{hx:.1}" cy="{hy:.1}" r="{HANDLE}" stroke="{colour}" data-event="{}"{close}"#,
                handle.index()
            );
        }
        (hx, hy)
    }

    /// Which bead at `spot` a `colour` route uses, and whether it is new
    /// (so not yet drawn).
    fn bead(&mut self, spot: Spot, colour: &'static str) -> (usize, bool) {
        let mine: Vec<&str> = self
            .beads
            .iter()
            .filter(|(at, _)| *at == spot)
            .map(|&(_, c)| c)
            .collect();
        if let Some(n) = mine.iter().position(|&c| c == colour) {
            return (n, false);
        }
        self.beads.push((spot, colour));
        (mine.len(), true)
    }

    fn label(&self, ix: EventIx) -> Option<Pill> {
        self.labels.get(ix.index()).copied().flatten()
    }

    fn powerlines(&mut self, ix: EventIx) {
        let d = self.diagram;
        let (Some(event), Some(to)) = (d.event(ix), self.label(ix)) else {
            return;
        };
        let (subject, colour) = match event {
            Event::Delegate(c) => (c.subject, colour(c.subject)),
            Event::Invoke(i) => (i.on, colour(i.on)),
            Event::Revoke(r) => match d.cert(r.target) {
                Some(c) => (c.subject, REVOKE),
                None => return,
            },
        };

        match event.authority() {
            Authority::Route(route) => {
                let mut from = route.source();
                let mut exit = self.label(from).map(|p| p.top());
                for &handle in route.through() {
                    let (Some(start), Some(h)) = (exit, self.label(handle)) else {
                        continue;
                    };
                    let (hx, hy) = self.handle(from, handle, h, colour);
                    self.arch(Some(from), handle, start, (hx, hy - HANDLE), colour);
                    exit = Some((hx, hy - HANDLE));
                    from = handle;
                }

                let Some(start) = exit else { return };
                match event {
                    // Straight down into an invocation that sits under its
                    // power; arched otherwise
                    Event::Invoke(_) if (start.0 - to.x).abs() < 0.5 => {
                        let (x, y1) = self.label(from).map_or(start, |p| p.bottom());
                        self.line(Some(from), ix, (x, y1), to.top(), colour);
                    }
                    // Into the diamond's upper edge, on the near side
                    Event::Revoke(_) => {
                        let (rx, ry) = DIAMOND;
                        let side = if start.0 < to.x { -1.0 } else { 1.0 };
                        let end = (to.x + side * rx / 2.0, to.y - ry / 2.0);
                        self.arch(Some(from), ix, start, end, colour);
                    }
                    Event::Delegate(_) | Event::Invoke(_) => {
                        self.arch(Some(from), ix, start, to.top(), colour);
                    }
                }
            }

            // Authority over yourself starts on your own lifeline: from your
            // head on the first row, otherwise from a small circle
            Authority::Root => {
                let Event::Delegate(_) = event else { return };
                let x = self.lifeline(subject);
                let side = if to.x < x { -1.0 } else { 1.0 };
                let (top_x, top_y) = to.top();
                let start = if ix.index() == 0 {
                    (x + side * 10.0, self.head() + HEAD_RY - 2.0)
                } else {
                    let cy = to.y - ROW / 2.0 + 2.0;
                    let _ = write!(
                        self.out,
                        r#"<circle class="root" cx="{x:.1}" cy="{cy:.1}" r="{HANDLE}" stroke="{colour}" data-event="{}"/>"#,
                        ix.index()
                    );
                    (x + side * HANDLE, cy)
                };
                // A quarter-ellipse (Bézier circle constant), out sideways and
                // down into the label, as round as the other hops
                let k = 0.552_284_75;
                let _ = write!(
                    self.out,
                    r#"<path class="{class}" data-to="{to}" d="M{sx:.1},{sy:.1} C{c1x:.1},{sy:.1} {top_x:.1},{c2y:.1} {top_x:.1},{top_y:.1}" stroke="{colour}"/>"#,
                    to = ix.index(),
                    class = self.powerline_class(ix),
                    sx = start.0,
                    sy = start.1,
                    c1x = start.0 + k * (top_x - start.0),
                    c2y = top_y - k * (top_y - start.1),
                );
            }

            Authority::Unauthorized => {}
        }
    }

    /// Up out of one label, over, and down into the next.
    fn arch(
        &mut self,
        from: Option<EventIx>,
        to: EventIx,
        (x1, y1): (f64, f64),
        (x2, y2): (f64, f64),
        colour: &str,
    ) {
        let peak = y1.min(y2) - ARCH;
        let from = from.map_or_else(String::new, |f| format!(r#" data-from="{}""#, f.index()));
        let _ = write!(
            self.out,
            r#"<path class="{class}"{from} data-to="{to}" d="M{x1:.1},{y1:.1} C{x1:.1},{peak:.1} {x2:.1},{peak:.1} {x2:.1},{y2:.1}" stroke="{colour}"/>"#,
            to = to.index(),
            class = self.powerline_class(to),
        );
    }

    fn line(
        &mut self,
        from: Option<EventIx>,
        to: EventIx,
        (x1, y1): (f64, f64),
        (x2, y2): (f64, f64),
        colour: &str,
    ) {
        let from = from.map_or_else(String::new, |f| format!(r#" data-from="{}""#, f.index()));
        let _ = write!(
            self.out,
            r#"<path class="{class}"{from} data-to="{to}" d="M{x1:.1},{y1:.1} L{x2:.1},{y2:.1}" stroke="{colour}"/>"#,
            to = to.index(),
            class = self.powerline_class(to),
        );
    }
}

/// An open arrowhead ending at `(x, y)`.
fn chevron(x: f64, y: f64, dir: f64, colour: &str) -> String {
    let back = x - dir * 8.0;
    format!(
        r#"<path class="chevron" d="M{back:.1},{:.1} L{x:.1},{y:.1} L{back:.1},{:.1}" stroke="{colour}"/>"#,
        y - 5.0,
        y + 5.0
    )
}

/// A clock face for the "now" handle: the hands advance an hour per event,
/// so dragging the line down winds the clock forward and up winds it back.
fn clock(happened: usize) -> &'static str {
    const FACES: [&str; 12] = [
        "🕛", "🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚",
    ];
    FACES.get(happened % FACES.len()).copied().unwrap_or("🕒")
}

/// An invisible band over a row's arrow, so it's easy to grab with a
/// trackpad rather than only on its strokes.
fn hit(a: f64, b: f64, y: f64) -> String {
    let (left, right) = (a.min(b) - 6.0, a.max(b) + 6.0);
    format!(
        r#"<rect class="hit" x="{left:.1}" y="{:.1}" width="{:.1}" height="{:.1}"/>"#,
        y - ROW * 0.35,
        right - left,
        ROW * 0.7
    )
}
