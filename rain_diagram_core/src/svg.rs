//! SVG export, for both notations. The same document is the static export
//! and the interactive embed's: every element carries hooks for scripting,
//! so it can be made interactive without re-rendering.
//!
//! | Attribute             | On                                     | Value                                             |
//! |-----------------------|----------------------------------------|---------------------------------------------------|
//! | `data-principal`      | principals (and rain lifelines)        | the principal's key                               |
//! | `data-event`          | each event's group; handles            | its index                                         |
//! | `data-cert`           | delegation groups                      | the certificate's label                           |
//! | `data-from`/`data-to` | powerlines                             | event indices (`data-from` is absent for roots)   |
//! | `class`               | event groups                           | `event delegate`/`invoke`/`revoke` and a status   |
//!
//! Statuses are `valid`, `revoked`, `void`, and `unauthorized`, as of the
//! last event.
//!
//! Colours carry no meaning of their own: each principal gets the next from
//! a palette, and lines and boxes take the colour of whoever's authority
//! they carry.

mod certflow;
mod rain;

use crate::{
    ast::{Kind, Text},
    authority::Status,
    diagram::{Diagram, Event, PrincipalIx, Region},
};
use alloc::{format, string::String};
use core::fmt::Write as _;

impl Diagram {
    /// Draw as a standalone SVG document, in the notation the header names.
    ///
    /// ```
    /// let diagram = rain_diagram_core::parse("rain\n(Doc)-[edit]->(Alice)")?;
    /// let svg = diagram.to_svg();
    /// assert!(svg.starts_with("<svg"));
    /// assert!(svg.contains(r#"data-cert="doc-alice""#));
    /// # Ok::<(), rain_diagram_core::Error>(())
    /// ```
    #[must_use]
    pub fn to_svg(&self) -> String {
        match self.kind() {
            Kind::Rain => self.to_rain_svg(),
            Kind::Certflow => self.to_certflow_svg(),
        }
    }

    /// Draw as a rain diagram (Keyhive notebook 08), whatever the header.
    #[must_use]
    pub fn to_rain_svg(&self) -> String {
        rain::render(self)
    }

    /// Draw as a certflow snapshot (Keyhive notebook 07) of the moment after
    /// the last event, whatever the header.
    #[must_use]
    pub fn to_certflow_svg(&self) -> String {
        certflow::render(self)
    }
}

/// Colours cycle through the notebook's palette.
const PALETTE: [&str; 8] = [
    "#3d7dd8", // blue
    "#e8892b", // orange
    "#3f9b5a", // green
    "#8a5cc9", // purple
    "#c2553a", // rust
    "#2a9d96", // teal
    "#c94f8a", // pink
    "#7d8a2e", // olive
];

const REVOKE: &str = "#d33";
const FONT: f64 = 13.0;
/// Average advance of the UI font at [`FONT`], for sizing boxes.
const CHAR: f64 = 7.4;
const MARGIN: f64 = 24.0;
const PILL: f64 = 22.0;

/// Region colours: distinct from most principals', and cycled.
const REGION_PALETTE: [&str; 5] = ["#d39b12", "#2a9d96", "#8a5cc9", "#c94f8a", "#3d7dd8"];

/// A region: a tinted box with its caption in a tab at the top-left.
fn region(
    out: &mut String,
    n: usize,
    region: &Region,
    (left, top, right, bottom): (f64, f64, f64, f64),
) {
    let colour = REGION_PALETTE
        .get(n % REGION_PALETTE.len())
        .copied()
        .unwrap_or("#888");
    let caption = region.caption();
    let tab = text_width(caption) * 0.85 + 14.0;
    let _ = write!(
        out,
        concat!(
            r#"<g class="region" data-region="{id}">"#,
            r#"<rect class="region-box" x="{left:.1}" y="{top:.1}" width="{w:.1}" height="{h:.1}" rx="6" fill="{colour}" stroke="{colour}"/>"#,
            r#"<rect class="region-tab" x="{left:.1}" y="{top:.1}" width="{tab:.1}" height="18" rx="6" fill="{colour}"/>"#,
            r#"<text class="region-caption" x="{tx:.1}" y="{ty:.1}" dominant-baseline="central">{caption}</text>"#,
            "</g>"
        ),
        id = escape(region.id.as_str()),
        left = left,
        top = top,
        w = right - left,
        h = bottom - top,
        colour = colour,
        tab = tab,
        tx = left + 7.0,
        ty = top + 9.0,
        caption = escape(caption),
    );
}

fn colour(p: PrincipalIx) -> &'static str {
    PALETTE
        .get(p.index() % PALETTE.len())
        .copied()
        .unwrap_or("#888")
}

/// A certificate (or invocation, with `double`) box centred on `(x, y)`.
fn pill(x: f64, y: f64, half: f64, text: &str, colour: &str, double: bool) -> String {
    let (left, top, w) = (x - half, y - PILL / 2.0, half * 2.0);
    let inner = if double {
        format!(
            r#"<rect class="inner" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="3" stroke="{colour}"/>"#,
            left + 3.0,
            top + 3.0,
            w - 6.0,
            PILL - 6.0
        )
    } else {
        String::new()
    };
    format!(
        concat!(
            r#"<rect class="pill" x="{left:.1}" y="{top:.1}" width="{w:.1}" height="{h}" rx="4"/>"#,
            r#"<rect class="tint" x="{left:.1}" y="{top:.1}" width="{w:.1}" height="{h}" rx="4" fill="{colour}" stroke="{colour}"/>"#,
            "{inner}",
            r#"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" dominant-baseline="central">{text}</text>"#
        ),
        h = PILL,
        left = left,
        top = top,
        w = w,
        colour = colour,
        inner = inner,
        x = x,
        y = y,
        text = escape(text),
    )
}

/// Literal colours rather than CSS variables, which many non-browser SVG
/// viewers ignore. Lines take their colour from attributes, so these only
/// set what doesn't vary.
const STYLE: &str = r"<style>
.rain-diagram text { fill: #1d1c1a; }
.rain-diagram .title { font-size: 16px; font-weight: 600; }
.rain-diagram .notes text { fill: #6b6760; }
.rain-diagram .head, .rain-diagram .node { fill: #fff; stroke-width: 2; }
.rain-diagram .lifeline { stroke-width: 1.5; }
.rain-diagram .event line, .rain-diagram .event .wire { fill: none; stroke-width: 1.5; }
.rain-diagram .chevron { fill: none; stroke-width: 1.5; stroke-linejoin: round; stroke-linecap: round; }
.rain-diagram .event line.double { stroke-width: 5; }
.rain-diagram .event line.double-inner { stroke: #fff; stroke-width: 2; }
.rain-diagram .pill { fill: #fff; }
.rain-diagram .tint { fill-opacity: 0.14; stroke-width: 1.75; }
.rain-diagram .inner { fill: none; stroke-width: 1.25; }
.rain-diagram .handle, .rain-diagram .root { fill: #fff; stroke-width: 1.75; }
.rain-diagram .powerline { fill: none; stroke-width: 1.5; stroke-dasharray: 5 4; }
/* Authority flows: dashes march from source to consumer (two whole dash
   periods per loop, so it's seamless) */
.rain-diagram .powerline { animation: rain-flow 1.4s linear infinite; }
@keyframes rain-flow { to { stroke-dashoffset: -18; } }
@media (prefers-reduced-motion: reduce) { .rain-diagram .powerline { animation: none; } }
.rain-diagram .revocation, .rain-diagram .cross { fill: none; stroke: #d33; stroke-width: 1.75; }
.rain-diagram .diamond { fill: #fde8e8; stroke: #d33; stroke-width: 1.75; }
.rain-diagram .revoke text { fill: #b3261e; font-size: 12px; }
.rain-diagram .region-box { fill-opacity: 0.06; stroke-opacity: 0.6; stroke-width: 1.5; }
.rain-diagram .region-caption { fill: #fff; font-size: 11px; font-weight: 600; }
.rain-diagram .now { stroke: #9a958c; stroke-width: 1.5; stroke-dasharray: 1 5; stroke-linecap: round; }
.rain-diagram .revoked, .rain-diagram .void { opacity: 0.4; }
.rain-diagram .revoked .tint { stroke-dasharray: 3 2; }
.rain-diagram .unauthorized .tint { stroke: #d33; stroke-dasharray: 2 2; }
@media (prefers-color-scheme: dark) {
  .rain-diagram text { fill: #ecebe8; }
  .rain-diagram .notes text { fill: #9a958c; }
  .rain-diagram .head, .rain-diagram .node, .rain-diagram .pill, .rain-diagram .handle, .rain-diagram .root { fill: #1f1e1c; }
  .rain-diagram .event line.double-inner { stroke: #1f1e1c; }
  .rain-diagram .diamond { fill: #3a1f1f; }
  .rain-diagram .revoke text { fill: #f2867c; }
}
</style>";

fn note(event: &Event) -> Option<&str> {
    match event {
        Event::Delegate(c) => c.note.as_ref(),
        Event::Invoke(i) => i.note.as_ref(),
        Event::Revoke(r) => r.note.as_ref(),
    }
    .map(Text::as_str)
}

const fn status_class(status: Status) -> &'static str {
    match status {
        Status::Valid => "valid",
        Status::Unauthorized => "unauthorized",
        Status::Revoked { .. } => "revoked",
        Status::Void { .. } => "void",
    }
}

#[allow(clippy::cast_precision_loss)] // diagram sizes are tiny
const fn index(n: usize) -> f64 {
    n as f64
}

fn text_width(text: &str) -> f64 {
    index(text.chars().count()) * CHAR
}

/// Escape text for XML content and attribute values.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A level as the notebook labels it: `👑 Admin`, `✏️ Edit`, `👀 Read`,
/// `📡 Relay`.
fn level_label(level: crate::level::Level) -> String {
    format!("{} {}", level.emoji(), level.name())
}
