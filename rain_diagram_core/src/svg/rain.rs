//! Rain diagrams: lifelines falling from each principal, one row per event.
//!
//! The picture follows Keyhive notebook 08:
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
    FONT, MARGIN, PILL, REVOKE, STYLE, colour, escape, index, level_label, note, pill,
    status_class, text_width,
};
use crate::{
    authority::Authority,
    diagram::{Cert, Diagram, Event, EventIx, Invoke, PrincipalIx, Revoke},
};
use alloc::{format, string::String, vec::Vec};
use core::fmt::Write as _;

/// Draw `diagram` as a rain diagram.
pub(super) fn render(diagram: &Diagram) -> String {
    Svg::new(diagram).draw()
}

const HEADER: f64 = 64.0;
const HEAD_RY: f64 = 16.0;
const ROW: f64 = 64.0;
/// How far a powerline arches above the label it leaves.
const ARCH: f64 = 34.0;
const HANDLE: f64 = 5.0;
const DIAMOND: (f64, f64) = (36.0, 13.0);

struct Svg<'a> {
    diagram: &'a Diagram,
    spacing: f64,
    title: f64,
    out: String,
    labels: Vec<Option<Pill>>,
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

    /// Where a transitive handle sits on this certificate's arrow.
    fn handle(&self) -> (f64, f64) {
        (self.x + self.dir * (self.half + 12.0), self.y)
    }
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
        }
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
        let width = notes_x + notes_width + MARGIN;
        let height = bottom + MARGIN / 2.0;

        let _ = write!(
            self.out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" class="rain-diagram" viewBox="0 0 {width:.0} {height:.0}" width="{width:.0}" height="{height:.0}" font-family="system-ui, sans-serif" font-size="{FONT}">"#
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
        }

        self.principals(bottom);
        let _ = write!(
            self.out,
            r#"<line class="now" x1="{MARGIN}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}"/>"#,
            y = bottom - 2.0,
            x2 = notes_x - 8.0,
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
                    r#"<g class="principal" data-principal="{key}">"#,
                    r#"<line class="lifeline" x1="{x:.1}" y1="{top:.1}" x2="{x:.1}" y2="{bottom:.1}" stroke="{colour}"/>"#,
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
                name = escape(principal.name().as_str()),
            );
        }
        self.out.push_str("</g>");
    }

    fn event(&self, ix: EventIx, out: &mut String) -> Option<Pill> {
        let status = self
            .diagram
            .now()
            .and_then(|now| self.diagram.status(ix, now))
            .map_or("valid", status_class);

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
        let text = &level_label(cert.level);
        let half = text_width(text) / 2.0 + 10.0;
        let dir = if to < from { -1.0 } else { 1.0 };
        // Just off the issuer's lifeline, unless the arrow is too short
        let offset = (half + 16.0).min((to - from).abs() / 2.0);
        let rect = Pill {
            x: from + dir * offset,
            y,
            half,
            dir,
        };

        let _ = write!(
            out,
            concat!(
                r#"<g class="event delegate {status}" data-event="{ix}" data-cert="{id}">"#,
                r#"<line x1="{from:.1}" y1="{y:.1}" x2="{to:.1}" y2="{y:.1}" stroke="{colour}"/>"#,
                "{head}{pill}",
                "<title>{title}</title>",
                "</g>"
            ),
            status = status,
            ix = ix.index(),
            id = escape(cert.id.as_str()),
            from = from,
            to = to,
            y = y,
            colour = colour(issuer),
            head = chevron(to, y, dir, colour(issuer)),
            pill = pill(rect.x, rect.y, rect.half, text, colour(subject), false),
            title = escape(&self.describe_cert(ix)),
        );
        rect
    }

    fn invocation(&self, ix: EventIx, invoke: &Invoke, status: &str, out: &mut String) -> Pill {
        let (by, on) = (self.lifeline(invoke.by), self.lifeline(invoke.on));
        let y = self.row(ix);
        let text = &level_label(invoke.level);
        let half = text_width(text) / 2.0 + 13.0;
        let dir = if on < by { -1.0 } else { 1.0 };

        // Right under the certificate that powers it, when that's between
        // the two lifelines
        let (low, high) = (by.min(on) + half + 10.0, by.max(on) - half - 10.0);
        let preferred = invoke
            .authority
            .route()
            .and_then(|r| self.label(r.held()))
            .map_or(by + dir * (half + 16.0), |p| p.x);
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
                r#"<g class="event invoke {status}" data-event="{ix}">"#,
                r#"<line class="double" x1="{by:.1}" y1="{y:.1}" x2="{on:.1}" y2="{y:.1}" stroke="{colour}"/>"#,
                r#"<line class="double-inner" x1="{by:.1}" y1="{y:.1}" x2="{on:.1}" y2="{y:.1}"/>"#,
                "{head}{pill}",
                "</g>"
            ),
            status = status,
            ix = ix.index(),
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
        let (cx, near) = (
            target.x,
            if x < target.x {
                target.x - rx
            } else {
                target.x + rx
            },
        );
        let under = target.y + PILL / 2.0;

        let _ = write!(
            out,
            concat!(
                r#"<g class="event revoke {status}" data-event="{ix}">"#,
                r#"<path class="revocation" d="M{x:.1},{y:.1} H{near:.1} M{cx:.1},{dt:.1} V{up:.1}"/>"#,
                r#"<path class="cross" d="M{a:.1},{b:.1} L{c:.1},{e:.1} M{a:.1},{e:.1} L{c:.1},{b:.1}"/>"#,
                r#"<path class="diamond" d="M{cx:.1},{dt:.1} L{dr:.1},{y:.1} L{cx:.1},{db:.1} L{dl:.1},{y:.1} Z"/>"#,
                r#"<text x="{cx:.1}" y="{y:.1}" text-anchor="middle" dominant-baseline="central">revoke</text>"#,
                "</g>"
            ),
            status = status,
            ix = ix.index(),
            x = x,
            y = y,
            near = near,
            cx = cx,
            up = under + 2.0,
            a = cx - 6.0,
            c = cx + 6.0,
            b = under + 2.0,
            e = under + 14.0,
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

    fn describe_cert(&self, ix: EventIx) -> String {
        let d = self.diagram;
        let Some(cert) = d.cert(ix) else {
            return String::new();
        };
        let name = |p| d.principal(p).map_or("", |p| p.name().as_str());
        let mut text = format!(
            "{}: {} grants {} {} over {}",
            cert.id,
            name(cert.issuer),
            name(cert.audience),
            cert.level,
            name(cert.subject)
        );
        if let Some(granted) = d.granted(ix).filter(|&g| g < cert.level) {
            let _ = write!(text, " (attenuated to {granted})");
        }
        text
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
                    let (hx, hy) = h.handle();
                    self.arch(Some(from), handle, start, (hx, hy - HANDLE), colour);
                    let _ = write!(
                        self.out,
                        r#"<circle class="handle" cx="{hx:.1}" cy="{hy:.1}" r="{HANDLE}" stroke="{colour}" data-event="{}"/>"#,
                        handle.index()
                    );
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
                    r#"<path class="powerline" data-to="{to}" d="M{sx:.1},{sy:.1} C{c1x:.1},{sy:.1} {top_x:.1},{c2y:.1} {top_x:.1},{top_y:.1}" stroke="{colour}"/>"#,
                    to = ix.index(),
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
            r#"<path class="powerline"{from} data-to="{to}" d="M{x1:.1},{y1:.1} C{x1:.1},{peak:.1} {x2:.1},{peak:.1} {x2:.1},{y2:.1}" stroke="{colour}"/>"#,
            to = to.index()
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
            r#"<path class="powerline"{from} data-to="{to}" d="M{x1:.1},{y1:.1} L{x2:.1},{y2:.1}" stroke="{colour}"/>"#,
            to = to.index()
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
