//! Certflow snapshots: who can do what, as of the last event.
//!
//! The picture follows Keyhive notebook 07:
//!
//! - Principals are round.
//! - Certificates are boxes, labelled with the access they grant. Solid
//!   lines run from the issuer, through the box, to the audience ("who
//!   delegated to whom"), in the subject's colour.
//! - Dashed lines are the flow of authority. They start at the subject and
//!   hop from certificate to certificate, never dropping into the
//!   principals in between.
//! - Revocations are red diamonds. A solid red line runs from the revoker,
//!   through the diamond, to the certificate it revokes, ending in an ✗;
//!   red dashes show where the revoker's authority comes from.
//!
//! Certflow has no way to show invocations happening in order, so they're
//! left out. Layout is automatic (see `layout`).

mod layout;

use super::{
    FONT, MARGIN, PILL, REVOKE, STYLE, colour, escape, level_label, pill, status_class, text_width,
};
use crate::{
    authority::Authority,
    diagram::{Diagram, Event, EventIx, PrincipalIx},
};
use alloc::{string::String, vec, vec::Vec};
use core::fmt::Write as _;
use layout::{Point, sqrt, to_f64};

/// Draw `diagram` as a certflow snapshot.
pub(super) fn render(diagram: &Diagram, happened: usize) -> String {
    let mut certflow = Certflow::new(diagram);
    certflow.happened = happened.min(diagram.events().len());
    certflow.draw()
}

/// Distance between a principal and its certificates.
const UNIT: f64 = 130.0;
const GAP: f64 = 18.0;
const DIAMOND: (f64, f64) = (22.0, 16.0);

/// A node's footprint: a circle, or a box (`half` width, `half_h` height).
#[derive(Debug, Clone, Copy)]
enum Shape {
    Circle { r: f64 },
    Box { half: f64, half_h: f64 },
}

impl Shape {
    /// How far from the centre the boundary is, heading along `(ux, uy)`.
    fn reach(self, (ux, uy): (f64, f64)) -> f64 {
        match self {
            Self::Circle { r } => r,
            Self::Box { half, half_h } => {
                let tx = if ux.abs() > 1e-9 {
                    half / ux.abs()
                } else {
                    f64::INFINITY
                };
                let ty = if uy.abs() > 1e-9 {
                    half_h / uy.abs()
                } else {
                    f64::INFINITY
                };
                tx.min(ty)
            }
        }
    }

    const fn radius(self) -> f64 {
        match self {
            Self::Circle { r } => r,
            Self::Box { half, .. } => half,
        }
    }
}

struct Certflow<'a> {
    diagram: &'a Diagram,
    shapes: Vec<Shape>,
    points: Vec<Point>,
    /// Node index for each principal and each event (if drawn).
    of_principal: Vec<usize>,
    of_event: Vec<Option<usize>>,
    title: f64,

    /// Extra margin for region boxes.
    frame: f64,

    /// How many events the snapshot includes.
    happened: usize,
    out: String,
}

impl<'a> Certflow<'a> {
    fn new(diagram: &'a Diagram) -> Self {
        let mut nodes = 0_usize;
        let mut shapes = Vec::new();
        let mut of_principal = Vec::new();
        let mut of_event = vec![None; diagram.events().len()];
        let mut edges = Vec::new();

        for principal in diagram.principals() {
            of_principal.push(nodes);
            nodes += 1;
            shapes.push(Shape::Circle {
                r: (text_width(principal.name().as_str()) / 2.0 + 14.0).max(30.0),
            });
        }

        let principal_node = |p: PrincipalIx| of_principal.get(p.index()).copied().unwrap_or(0);
        for ix in diagram.event_ixs() {
            match diagram.event(ix) {
                Some(Event::Delegate(cert)) => {
                    let node = nodes;
                    if let Some(slot) = of_event.get_mut(ix.index()) {
                        *slot = Some(node);
                    }
                    nodes += 1;
                    shapes.push(Shape::Box {
                        half: text_width(&level_label(cert.level)) / 2.0 + 12.0,
                        half_h: PILL / 2.0,
                    });
                    edges.push((principal_node(cert.issuer), node));
                    edges.push((node, principal_node(cert.audience)));
                }
                Some(Event::Revoke(revoke)) => {
                    let node = nodes;
                    if let Some(slot) = of_event.get_mut(ix.index()) {
                        *slot = Some(node);
                    }
                    nodes += 1;
                    shapes.push(Shape::Box {
                        half: DIAMOND.0,
                        half_h: DIAMOND.1,
                    });
                    edges.push((principal_node(revoke.by), node));
                    if let Some(target) = of_event.get(revoke.target.index()).copied().flatten() {
                        edges.push((node, target));
                    }
                }
                Some(Event::Invoke(_)) | None => {}
            }
        }

        let radii: Vec<f64> = shapes.iter().map(|s| s.radius()).collect();
        let mut points = layout::place(&radii, &edges, UNIT, GAP);

        // Shift into view
        let title = if diagram.title().is_some() { 32.0 } else { 0.0 };
        // Room for region boxes, which pad out around their nodes
        let frame = if diagram.regions().is_empty() {
            0.0
        } else {
            30.0 + 24.0 * to_f64(diagram.region_lanes().into_iter().max().unwrap_or(0))
        };
        let (min_x, min_y) = points
            .iter()
            .zip(&radii)
            .fold((f64::INFINITY, f64::INFINITY), |(x, y), (p, r)| {
                (x.min(p.0 - r), y.min(p.1 - r))
            });
        for p in &mut points {
            *p = (
                p.0 - min_x + MARGIN + frame,
                p.1 - min_y + MARGIN + title + frame,
            );
        }

        Self {
            diagram,
            shapes,
            points,
            of_principal,
            of_event,
            title,
            frame,
            happened: diagram.events().len(),
            out: String::new(),
        }
    }

    fn draw(mut self) -> String {
        let (width, height) = self
            .points
            .iter()
            .zip(&self.shapes)
            .fold((0.0_f64, 0.0_f64), |(w, h), (p, s)| {
                (w.max(p.0 + s.radius()), h.max(p.1 + s.radius()))
            });
        let (width, height) = (width + MARGIN + self.frame, height + MARGIN + self.frame);

        let _ = write!(
            self.out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" class="rain-diagram certflow" viewBox="0 0 {width:.0} {height:.0}" width="{width:.0}" height="{height:.0}" font-family="system-ui, sans-serif" font-size="{FONT}">"#
        );
        self.out.push_str(STYLE);
        if let Some(title) = self.diagram.title() {
            let _ = write!(
                self.out,
                r#"<text class="title" x="{MARGIN}" y="{y}">{}</text>"#,
                escape(title.as_str()),
                y = MARGIN + 4.0 + self.title - 32.0,
            );
        }

        // Regions furthest down: a box around the events inside each, outer
        // regions padded further than the ones nested in them
        let lanes = self.diagram.region_lanes();
        let deepest = lanes.iter().copied().max().unwrap_or(0);
        for (n, (region, &lane)) in self.diagram.regions().iter().zip(&lanes).enumerate() {
            let inside = (region.first.index()..=region.last.index())
                .filter(|&i| i < self.happened)
                .filter_map(|i| self.event_node(EventIx::new(i)))
                .map(|node| (self.point(node), self.shape(node).radius()));
            let (l, t, r, b) = inside.fold(
                (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(l, t, r, b), ((x, y), radius)| {
                    (
                        l.min(x - radius),
                        t.min(y - radius),
                        r.max(x + radius),
                        b.max(y + radius),
                    )
                },
            );
            if !l.is_finite() {
                continue; // only invocations inside, which certflow doesn't draw
            }
            // Enough per level for the tabs of nested regions to stack
            let pad = 12.0 + 24.0 * to_f64(deepest - lane);
            super::region(
                &mut self.out,
                n,
                region,
                (l - pad, t - pad - 18.0, r + pad, b + pad),
            );
        }

        // Powerlines underneath, then events, then principals on top
        self.out.push_str(r#"<g class="powerlines">"#);
        for ix in self.diagram.event_ixs() {
            self.powerlines(ix);
        }
        self.out.push_str("</g>");

        for ix in self.diagram.event_ixs() {
            self.event(ix);
        }

        self.out.push_str(r#"<g class="principals">"#);
        for (i, principal) in self.diagram.principals().iter().enumerate() {
            let p = PrincipalIx::new(i);
            if !self.appeared(p) {
                continue;
            }
            let node = self.principal_node(p);
            let ((x, y), r) = (self.point(node), self.shape(node).radius());
            let _ = write!(
                self.out,
                concat!(
                    r#"<g class="principal" data-principal="{key}">"#,
                    r#"<circle class="node" cx="{x:.1}" cy="{y:.1}" r="{r:.1}" stroke="{colour}"/>"#,
                    r#"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" dominant-baseline="central">{name}</text>"#,
                    "</g>"
                ),
                key = escape(principal.key()),
                x = x,
                y = y,
                r = r,
                colour = colour(p),
                name = escape(principal.name().as_str()),
            );
        }
        self.out.push_str("</g></svg>");
        self.out
    }

    fn principal_node(&self, p: PrincipalIx) -> usize {
        self.of_principal.get(p.index()).copied().unwrap_or(0)
    }

    /// The last event in the snapshot.
    fn now(&self) -> Option<EventIx> {
        self.happened
            .checked_sub(1)
            .and_then(|i| self.diagram.event_ix(i))
    }

    /// Whether `p` is in the snapshot: everyone once everything has
    /// happened, otherwise only those involved in what has.
    fn appeared(&self, p: PrincipalIx) -> bool {
        let d = self.diagram;
        self.happened >= d.events().len()
            || d.events()
                .iter()
                .take(self.happened)
                .any(|e| match &e.node {
                    Event::Delegate(c) => [c.issuer, c.audience, c.subject].contains(&p),
                    Event::Invoke(i) => i.by == p || i.on == p,
                    Event::Revoke(r) => r.by == p,
                })
    }

    fn event_node(&self, ix: EventIx) -> Option<usize> {
        self.of_event.get(ix.index()).copied().flatten()
    }

    fn shape(&self, node: usize) -> Shape {
        self.shapes
            .get(node)
            .copied()
            .unwrap_or(Shape::Circle { r: 0.0 })
    }

    fn point(&self, node: usize) -> Point {
        self.points.get(node).copied().unwrap_or((0.0, 0.0))
    }

    /// Where the straight line from node `a` towards node `b` leaves `a`'s
    /// boundary, plus the unit direction.
    fn exit(&self, a: usize, b: usize) -> (Point, (f64, f64)) {
        let (pa, pb) = (self.point(a), self.point(b));
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let length = sqrt(dx * dx + dy * dy).max(1e-9);
        let u = (dx / length, dy / length);
        let r = self.shape(a).reach(u);
        ((pa.0 + u.0 * r, pa.1 + u.1 * r), u)
    }

    #[allow(clippy::too_many_lines)] // one arm per drawn kind, sharing geometry
    fn event(&mut self, ix: EventIx) {
        let d = self.diagram;
        let (Some(event), Some(node)) = (d.event(ix), self.event_node(ix)) else {
            return;
        };
        if ix.index() >= self.happened {
            return;
        }
        let status = self
            .now()
            .and_then(|now| d.status(ix, now))
            .map_or("valid", status_class);
        let (cx, cy) = self.point(node);

        match event {
            Event::Delegate(cert) => {
                let stroke = colour(cert.subject);
                let (issuer, audience) = (
                    self.principal_node(cert.issuer),
                    self.principal_node(cert.audience),
                );
                let (from, _) = self.exit(issuer, node);
                let (into, _) = self.exit(node, issuer);
                let (out, _) = self.exit(node, audience);
                let (tip, u) = self.exit(audience, node);
                let u = (-u.0, -u.1);
                let Shape::Box { half, .. } = self.shape(node) else {
                    return;
                };

                let _ = write!(
                    self.out,
                    concat!(
                        r#"<g class="event delegate {status}" data-event="{ix}" data-cert="{id}">"#,
                        r#"<path class="wire" d="M{fx:.1},{fy:.1} L{into_x:.1},{into_y:.1} M{ox:.1},{oy:.1} L{tx:.1},{ty:.1}" stroke="{stroke}"/>"#,
                        "{head}{pill}",
                        "</g>"
                    ),
                    status = status,
                    ix = ix.index(),
                    id = escape(cert.id.as_str()),
                    fx = from.0,
                    fy = from.1,
                    into_x = into.0,
                    into_y = into.1,
                    ox = out.0,
                    oy = out.1,
                    tx = tip.0,
                    ty = tip.1,
                    stroke = stroke,
                    head = arrowhead(tip, u, stroke),
                    pill = pill(cx, cy, half, &level_label(cert.level), stroke, false),
                );
            }

            Event::Revoke(revoke) => {
                let revoker = self.principal_node(revoke.by);
                let (from, _) = self.exit(revoker, node);
                let (into, _) = self.exit(node, revoker);
                let (rx, ry) = DIAMOND;
                let target = self.event_node(revoke.target);
                let cross = target.map(|t| {
                    let (out, _) = self.exit(node, t);
                    let (end, _) = self.exit(t, node);
                    (out, end)
                });

                let _ = write!(
                    self.out,
                    concat!(
                        r#"<g class="event revoke {status}" data-event="{ix}">"#,
                        r#"<path class="revocation" d="M{fx:.1},{fy:.1} L{into_x:.1},{into_y:.1}"/>"#,
                    ),
                    status = status,
                    ix = ix.index(),
                    fx = from.0,
                    fy = from.1,
                    into_x = into.0,
                    into_y = into.1,
                );
                if let Some((out, end)) = cross {
                    let _ = write!(
                        self.out,
                        concat!(
                            r#"<path class="revocation" d="M{ox:.1},{oy:.1} L{ex:.1},{ey:.1}"/>"#,
                            r#"<path class="cross" d="M{a:.1},{b:.1} L{c:.1},{e:.1} M{a:.1},{e:.1} L{c:.1},{b:.1}"/>"#,
                        ),
                        ox = out.0,
                        oy = out.1,
                        ex = end.0,
                        ey = end.1,
                        a = end.0 - 6.0,
                        c = end.0 + 6.0,
                        b = end.1 - 6.0,
                        e = end.1 + 6.0,
                    );
                }
                let _ = write!(
                    self.out,
                    concat!(
                        r#"<path class="diamond" d="M{cx:.1},{t:.1} L{r:.1},{cy:.1} L{cx:.1},{b:.1} L{l:.1},{cy:.1} Z"/>"#,
                        "</g>"
                    ),
                    cx = cx,
                    cy = cy,
                    t = cy - ry,
                    b = cy + ry,
                    l = cx - rx,
                    r = cx + rx,
                );
            }

            Event::Invoke(_) => {}
        }
    }

    fn powerlines(&mut self, ix: EventIx) {
        let d = self.diagram;
        let (Some(event), Some(node)) = (d.event(ix), self.event_node(ix)) else {
            return;
        };
        if ix.index() >= self.happened {
            return;
        }
        let stroke = match event {
            Event::Delegate(cert) => colour(cert.subject),
            Event::Revoke(_) => REVOKE,
            Event::Invoke(_) => return,
        };

        match event.authority() {
            // Hop from certificate to certificate, never through principals
            Authority::Route(route) => {
                let mut from = route.source();
                for &handle in route.through() {
                    if let (Some(a), Some(b)) = (self.event_node(from), self.event_node(handle)) {
                        self.hop(Some(from), handle, a, b, stroke, None);
                    }
                    from = handle;
                }
                if let Some(a) = self.event_node(from) {
                    self.hop(Some(from), ix, a, node, stroke, None);
                }
            }

            // Authority over yourself starts at you
            Authority::Root => {
                if let Event::Delegate(cert) = event {
                    // It shares both ends with the issuer's wire, so bow it
                    // well clear, away from the audience
                    let subject = self.principal_node(cert.subject);
                    let away = self.point(self.principal_node(cert.audience));
                    self.hop(None, ix, subject, node, stroke, Some(away));
                }
            }

            Authority::Unauthorized => {}
        }
    }

    /// A dashed curve between two nodes' boundaries, bowed to one side (away
    /// from `away`, if given) so it doesn't sit on top of the solid lines.
    fn hop(
        &mut self,
        from: Option<EventIx>,
        to: EventIx,
        a: usize,
        b: usize,
        stroke: &str,
        away: Option<Point>,
    ) {
        let (start, _) = self.exit(a, b);
        let (end, _) = self.exit(b, a);
        let (dx, dy) = (end.0 - start.0, end.1 - start.1);
        let length = sqrt(dx * dx + dy * dy).max(1e-9);
        // At least ~20px of daylight at the middle of the curve
        let bow = 0.3_f64.max(40.0 / length);
        let mid = (f64::midpoint(start.0, end.0), f64::midpoint(start.1, end.1));
        let mut normal = (-dy * bow, dx * bow);
        if let Some(away) = away {
            let towards = (away.0 - mid.0) * normal.0 + (away.1 - mid.1) * normal.1;
            if towards > 0.0 {
                normal = (-normal.0, -normal.1);
            }
        }
        let control = (mid.0 + normal.0, mid.1 + normal.1);
        let from = from.map_or_else(String::new, |f| {
            alloc::format!(r#" data-from="{}""#, f.index())
        });
        let _ = write!(
            self.out,
            r#"<path class="powerline"{from} data-to="{to}" d="M{:.1},{:.1} Q{:.1},{:.1} {:.1},{:.1}" stroke="{stroke}"/>"#,
            start.0,
            start.1,
            control.0,
            control.1,
            end.0,
            end.1,
            to = to.index()
        );
    }
}

/// An open arrowhead at `tip`, pointing along unit vector `u`.
fn arrowhead(tip: Point, u: (f64, f64), stroke: &str) -> String {
    let (back, side) = (9.0, 5.0);
    let base = (tip.0 - u.0 * back, tip.1 - u.1 * back);
    let normal = (-u.1 * side, u.0 * side);
    alloc::format!(
        r#"<path class="chevron" d="M{:.1},{:.1} L{:.1},{:.1} L{:.1},{:.1}" stroke="{stroke}"/>"#,
        base.0 + normal.0,
        base.1 + normal.1,
        tip.0,
        tip.1,
        base.0 - normal.0,
        base.1 - normal.1,
    )
}
