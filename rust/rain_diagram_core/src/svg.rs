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
//! | `class`               | powerlines                             | `powerline` and the status of the event it feeds  |
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
    diagram::{Diagram, Event, EventIx, PrincipalIx, Region},
};
use alloc::{format, string::String};
use core::{
    cmp::Ordering,
    fmt::{self, Write as _},
};

impl Diagram {
    /// Draw as a standalone SVG document, in the notation the header names.
    ///
    /// ```
    /// let diagram = rain_diagram_core::parse("rain\n(Doc)-[edit(Doc)]->(Alice)")?;
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

    /// Draw as a rain diagram ([Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/)), whatever the header.
    #[must_use]
    pub fn to_rain_svg(&self) -> String {
        self.to_rain_svg_at(self.events().len())
    }

    /// Draw as a rain diagram with the "now" line below the first `happened`
    /// events: statuses are as of then, and later rows are faded.
    ///
    /// ```
    /// let d = rain_diagram_core::parse(
    ///     "rain
    ///      doc-alice: (Doc)-[edit(Doc)]->(Alice)
    ///      (Doc)-<revoke>-x[doc-alice]",
    /// )?;
    /// // Before the revocation, the certificate is still valid
    /// assert!(d.to_rain_svg_at(1).contains(r#"class="event delegate valid""#));
    /// assert!(d.to_rain_svg_at(2).contains(r#"class="event delegate revoked""#));
    /// # Ok::<(), rain_diagram_core::Error>(())
    /// ```
    #[must_use]
    pub fn to_rain_svg_at(&self, happened: usize) -> String {
        rain::render(self, happened)
    }

    /// Draw as a certflow snapshot ([Keyhive Lab Note #7](https://www.inkandswitch.com/keyhive/notebook/07/)) of the moment after
    /// the last event, whatever the header.
    #[must_use]
    pub fn to_certflow_svg(&self) -> String {
        self.to_certflow_svg_at(self.events().len())
    }

    /// Draw as a certflow snapshot after the first `happened` events. The
    /// layout comes from the whole diagram, so nodes stay put from one
    /// moment to the next.
    #[must_use]
    pub fn to_certflow_svg_at(&self, happened: usize) -> String {
        certflow::render(self, happened)
    }
}

/// Colours cycle through [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/)'s palette.
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
    let colour = region.colour.map_or_else(
        || {
            REGION_PALETTE
                .get(n % REGION_PALETTE.len())
                .copied()
                .unwrap_or("#888")
        },
        colour,
    );
    let caption = region.caption();
    let tab = text_width(caption) * 0.85 + 14.0;
    let _ = write!(
        out,
        concat!(
            r#"<g class="region" data-region="{id}"{note}>"#,
            r#"<rect class="region-box" x="{left:.1}" y="{top:.1}" width="{w:.1}" height="{h:.1}" rx="6" fill="{colour}" stroke="{colour}"/>"#,
            r#"<rect class="region-tab" x="{left:.1}" y="{top:.1}" width="{tab:.1}" height="18" rx="6" fill="{colour}"/>"#,
            r#"<text class="region-caption" x="{tx:.1}" y="{ty:.1}" dominant-baseline="central">{caption}</text>"#,
            "</g>"
        ),
        id = escape(region.id.as_str()),
        note = region.note.as_ref().map_or_else(String::new, |n| format!(
            r#" data-note="{}""#,
            escape(n.as_str())
        )),
        left = Px(left),
        top = Px(top),
        w = Px(right - left),
        h = Px(bottom - top),
        colour = colour,
        tab = Px(tab),
        tx = Px(left + 7.0),
        ty = Px(top + 9.0),
        caption = escape(caption)
    );
}

fn colour(p: PrincipalIx) -> &'static str {
    PALETTE
        .get(p.index() % PALETTE.len())
        .copied()
        .unwrap_or("#888")
}

/// A certificate (or invocation, with `double`) box centred on `(x, y)`.
/// A coordinate or size, written with integer arithmetic: `{:.1}` gives one
/// decimal, `{:.0}` none, and no precision gives one decimal only when
/// it's needed (`22`, `22.5`).
///
/// SVG output is all such numbers, and formatting `f64` with `core::fmt`
/// pulls in its float formatter: about 20 KB of Wasm. Rounding is half to
/// even on the scaled value, like `core::fmt`'s for exact ties.
#[derive(Debug, Clone, Copy)]
struct Px(f64);

impl fmt::Display for Px {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let decimals = f.precision();
        let scale = if decimals == Some(0) { 1 } else { 10 };
        // As `core::fmt` does, negatives keep their sign even when they
        // round to zero (`-0.0`)
        let sign = if self.0.is_sign_negative() { "-" } else { "" };
        let n = round_scaled(self.0.abs(), scale);
        match decimals {
            Some(0) => write!(f, "{sign}{n}"),
            None if n.is_multiple_of(10) => write!(f, "{sign}{}", n / 10),
            _ => write!(f, "{sign}{}.{}", n / 10, n % 10),
        }
    }
}

/// `x × scale` (`x` ≥ 0) to the nearest whole number, exactly as
/// `core::fmt` rounds the decimal value of `x`: ties (only possible when
/// `x` is exactly halfway) go to even. Near a tie, `x × scale` in floating
/// point can land on the wrong side, so the tie is checked exactly.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "canvas coordinates are small and non-negative here"
)]
fn round_scaled(x: f64, scale: u64) -> u64 {
    if !x.is_finite() {
        return 0;
    }
    // From 2^52 up, every f64 is a whole number: nothing to round
    if x >= 4_503_599_627_370_496.0 {
        return (x as u64).saturating_mul(scale);
    }
    let k = (x * scale as f64) as u64;
    // Against the tie between k and k + 1: (2k + 1) / (2 × scale)
    match compare_exact(x, 2 * scale, 2 * k + 1) {
        Ordering::Less => k,
        Ordering::Equal if k.is_multiple_of(2) => k,
        Ordering::Greater | Ordering::Equal => k + 1,
    }
}

/// How `x × factor` compares with `n`, exactly, for 0 ≤ `x` < 2^52:
/// `x` is `mantissa × 2^exponent` with a negative exponent, so both sides
/// are integers once `n` is shifted up by it.
fn compare_exact(x: f64, factor: u64, n: u64) -> Ordering {
    let bits = x.to_bits();
    let raw_exponent = (bits >> 52) & 0x7ff;
    // Zero or subnormal: below 2^-1022, far under any `n` ≥ 1
    if raw_exponent == 0 {
        return Ordering::Less;
    }
    let mantissa = (bits & ((1 << 52) - 1)) + (1 << 52);
    let exponent = i64::try_from(raw_exponent).unwrap_or(0) - 1075;
    let left = u128::from(mantissa) * u128::from(factor);
    let shift = u32::try_from(-exponent).unwrap_or(u32::MAX);
    // So tiny that `n` shifted up overflows: `x × factor` is far below it
    if shift >= u128::from(n).leading_zeros() {
        return Ordering::Less;
    }
    left.cmp(&(u128::from(n) << shift))
}

/// How wide the canvas must be to fit the title (16px, semibold) and its
/// margins.
fn title_width(d: &Diagram) -> f64 {
    d.title().map_or(0.0, |t| {
        ceil(text_width(t.as_str()) * 16.0 / FONT * 1.08 + 2.0 * MARGIN)
    })
}

/// The next whole number up, for a positive `x` (`f64::ceil` needs std).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "canvas sizes are small"
)]
fn ceil(x: f64) -> f64 {
    let whole = x as i64 as f64;
    if whole < x { whole + 1.0 } else { whole }
}

/// A tooltip for a delegation or invocation: what it does and every caveat
/// it must satisfy (its own, and those on everything it relies on).
fn describe(d: &Diagram, ix: EventIx) -> String {
    let name = |p| d.principal(p).map_or("", |p| p.name().as_str());
    let mut text = match d.event(ix) {
        Some(Event::Delegate(cert)) => {
            let mut text = format!(
                "{}: {} grants {} {} over {}",
                cert.id,
                name(cert.issuer),
                name(cert.audience),
                cert.ability,
                name(cert.subject)
            );
            if let Some(granted) = d.granted(ix).filter(|&g| *g != cert.ability) {
                let _ = write!(text, " (attenuated to {granted})");
            }
            text
        }
        Some(Event::Invoke(invoke)) => format!(
            "{} uses {} on {}",
            name(invoke.by),
            invoke.ability,
            name(invoke.on)
        ),
        Some(Event::Revoke(_)) | None => return String::new(),
    };
    let mut caveats = d.caveats(ix).peekable();
    if caveats.peek().is_some() {
        text.push_str("\nbound by:");
        for (_, caveat) in caveats {
            let _ = write!(text, "\n  where {caveat}");
        }
    }
    text
}

/// A certificate's own caveat, in small type under its label at `(x, y)`.
fn caveat_text(x: f64, y: f64, caveat: Option<&crate::ast::Caveat>) -> String {
    caveat.map_or_else(String::new, |caveat| {
        format!(
            r#"<text class="caveat" x="{x:.1}" y="{:.1}" text-anchor="middle" dominant-baseline="central">where {}</text>"#,
            Px(y + PILL / 2.0 + 10.0),
            escape(caveat.as_str()), x = Px(x))
    })
}

fn pill(x: f64, y: f64, half: f64, text: &str, colour: &str, double: bool) -> String {
    let (left, top, w) = (x - half, y - PILL / 2.0, half * 2.0);
    let inner = if double {
        format!(
            r#"<rect class="inner" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="3" stroke="{colour}"/>"#,
            Px(left + 3.0),
            Px(top + 3.0),
            Px(w - 6.0),
            Px(PILL - 6.0)
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
        h = Px(PILL),
        left = Px(left),
        top = Px(top),
        w = Px(w),
        colour = colour,
        inner = inner,
        x = Px(x),
        y = Px(y),
        text = escape(text)
    )
}

/// Literal colours rather than CSS variables, which many non-browser SVG
/// viewers ignore. Lines take their colour from attributes, so these only
/// set what doesn't vary.
const STYLE: &str = r"<style>
.rain-diagram text { fill: #1d1c1a; }
.rain-diagram .title { font-size: 16px; font-weight: 600; }
.rain-diagram .notes text { fill: #6b6760; }
/* Caveats sit under their label, haloed so lines behind stay readable */
.rain-diagram text.caveat { fill: #6b6760; font-size: 11px; font-style: italic; paint-order: stroke; stroke: #fff; stroke-width: 3px; stroke-linejoin: round; }
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
.rain-diagram .powerline.void, .rain-diagram .powerline.revoked { animation: none; }
@media (prefers-reduced-motion: reduce) { .rain-diagram .powerline { animation: none; } }
.rain-diagram .revocation, .rain-diagram .cross { fill: none; stroke: #d33; stroke-width: 1.75; }
.rain-diagram .diamond { fill: #fde8e8; stroke: #d33; stroke-width: 1.75; }
.rain-diagram .revoke text { fill: #b3261e; font-size: 12px; }
.rain-diagram .region-box { fill-opacity: 0.06; stroke-opacity: 0.6; stroke-width: 1.5; pointer-events: none; }
.rain-diagram .region text.region-caption { fill: #fff; font-size: 11px; font-weight: 600; }
.rain-diagram .now-line { stroke: #9a958c; stroke-width: 1.5; stroke-dasharray: 1 5; stroke-linecap: round; }
/* Invisible hit areas, so rows and the now line are easy to grab */
.rain-diagram .hit { fill: transparent; }
.rain-diagram .now-handle text { font-size: 16px; }
.rain-diagram .future { opacity: 0.18; }
.rain-diagram .revoked, .rain-diagram .void { opacity: 0.4; }
.rain-diagram .revoked .tint { stroke-dasharray: 3 2; }
.rain-diagram .unauthorized .tint { stroke: #d33; stroke-dasharray: 2 2; }
.rain-diagram .revoke.unauthorized .revocation, .rain-diagram .revoke.unauthorized .diamond { stroke-dasharray: 4 3; opacity: 0.55; }
/* Dark: follows the system unless a host page adds a `light` or `dark` class
   to the root element (the preview's theme toggle does). No angle brackets
   in here: inline SVG styles are parsed as markup, not raw text */
@media (prefers-color-scheme: dark) {
  .rain-diagram:not(.light) text { fill: #ecebe8; }
  .rain-diagram:not(.light) .notes text { fill: #9a958c; }
  .rain-diagram:not(.light) text.caveat { fill: #9a958c; stroke: #1f1e1c; }
  .rain-diagram:not(.light) :is(.head, .node, .pill, .handle, .root) { fill: #1f1e1c; }
  .rain-diagram:not(.light) .event line.double-inner { stroke: #1f1e1c; }
  .rain-diagram:not(.light) .diamond { fill: #3a1f1f; }
  .rain-diagram:not(.light) .revoke text { fill: #f2867c; }
}
.rain-diagram.dark text { fill: #ecebe8; }
.rain-diagram.dark .notes text { fill: #9a958c; }
.rain-diagram.dark text.caveat { fill: #9a958c; stroke: #1f1e1c; }
.rain-diagram.dark :is(.head, .node, .pill, .handle, .root) { fill: #1f1e1c; }
.rain-diagram.dark .event line.double-inner { stroke: #1f1e1c; }
.rain-diagram.dark .diamond { fill: #3a1f1f; }
.rain-diagram.dark .revoke text { fill: #f2867c; }
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

/// Roughly how wide `text` sets in the UI font at [`FONT`], for sizing
/// boxes. There are no font metrics here (the SVG is drawn by whatever
/// font the viewer has), so this sorts characters into width classes that
/// hold across common sans-serifs, erring wide so text stays inside its box.
fn text_width(text: &str) -> f64 {
    text.chars().map(advance).sum::<f64>() * FONT
}

/// One character's advance, in ems.
const fn advance(c: char) -> f64 {
    match c {
        // Combining marks, variation selectors (as in `✏️`), and joiners
        '\u{0300}'..='\u{036f}' | '\u{200b}'..='\u{200f}' | '\u{fe00}'..='\u{fe0f}' => 0.0,
        ' ' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' | 'i' | 'j' | 'l' | 'I' => 0.3,
        '(' | ')' | '[' | ']' | 'f' | 't' | 'r' | '-' => 0.38,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.85,
        'A'..='Z' => 0.66,
        c if c.is_ascii() => 0.56,
        c if wide(c) => 1.05,
        // Other scripts (accented Latin, Greek, Cyrillic, Arabic, …)
        _ => 0.62,
    }
}

/// East Asian wide and fullwidth characters, and emoji: about an em each.
const fn wide(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x115f          // Hangul Jamo
        | 0x2600..=0x27bf        // Symbols and dingbats (☀, ✏, ✂)
        | 0x2e80..=0xa4cf        // CJK radicals through Yi
        | 0xac00..=0xd7a3        // Hangul syllables
        | 0xf900..=0xfaff        // CJK compatibility ideographs
        | 0xfe30..=0xfe4f        // CJK compatibility forms
        | 0xff00..=0xff60        // Fullwidth forms
        | 0xffe0..=0xffe6
        | 0x1f000..=0x1faff      // Emoji and pictographs
        | 0x20000..=0x3fffd // CJK extensions
    )
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

#[cfg(test)]
mod tests {
    use super::{FONT, MARGIN, Px, text_width, title_width};

    /// The title sits a margin in from the left, and the canvas leaves the
    /// same margin after it (rounded up to a whole pixel).
    #[test]
    fn titles_get_a_margin_on_both_sides() -> Result<(), crate::Error> {
        let title = "A title of some length, with Wide letters";
        let d = crate::parse(&alloc::format!("rain\ntitle {title}\n(A)-[read(A)]->(B)"))?;
        let text = text_width(title) * 16.0 / FONT * 1.08;
        let slack = title_width(&d) - MARGIN - text - MARGIN;
        assert!((0.0..1.0).contains(&slack), "{slack}");
        Ok(())
    }

    #[test]
    fn coordinates_format_like_floats() {
        use alloc::format;
        let px = |x: f64| {
            (
                format!("{:.1}", Px(x)),
                format!("{:.0}", Px(x)),
                format!("{}", Px(x)),
            )
        };
        assert_eq!(px(22.0), ("22.0".into(), "22".into(), "22".into()));
        assert_eq!(px(12.34), ("12.3".into(), "12".into(), "12.3".into()));
        assert_eq!(px(-3.96), ("-4.0".into(), "-4".into(), "-4".into()));
        assert_eq!(px(-0.04).0, "-0.0");
        // Far outside any canvas, still what `core::fmt` writes
        for x in [
            1e-30,
            5e-324,
            4_503_599_627_370_496.0,
            1e17,
            123_456_789.987_654,
        ] {
            assert_eq!(format!("{:.1}", Px(x)), format!("{x:.1}"), "{x}");
            assert_eq!(format!("{:.0}", Px(x)), format!("{x:.0}"), "{x}");
        }
    }

    /// Exactly what `core::fmt` writes, ties and near-ties included.
    #[test]
    fn rounds_exactly_like_core_fmt() {
        use alloc::format;
        bolero::check!()
            .with_type::<(u32, u8)>()
            .for_each(|&(n, frac)| {
                // Hundredths, so near-ties like 34.85 and 0.05 come up often
                let x = f64::from(n % 100_000) + f64::from(frac % 100) / 100.0;
                for x in [x, -x] {
                    assert_eq!(format!("{:.1}", Px(x)), format!("{x:.1}"), "{x}");
                    assert_eq!(format!("{:.0}", Px(x)), format!("{x:.0}"), "{x}");
                }
            });
    }

    #[test]
    fn wide_scripts_take_about_twice_the_room() {
        let latin = text_width("Alice");
        let cjk = text_width("爱丽丝的文");
        assert!(cjk > 1.6 * latin, "{cjk} vs {latin}");
        assert!(text_width("김민준") > text_width("Kim"));
    }

    #[test]
    fn accented_latin_is_narrower_than_cjk() {
        assert!(text_width("é") < text_width("文"));
        assert!(text_width("Zoë") < text_width("佐伊三"));
    }

    #[test]
    fn invisible_characters_take_none() {
        // U+FE0F only asks for the emoji presentation of ✏
        assert!((text_width("✏\u{fe0f}") - text_width("✏")).abs() < f64::EPSILON);
        assert!((text_width("e\u{301}") - text_width("e")).abs() < f64::EPSILON);
    }

    #[test]
    fn narrow_and_wide_latin_differ() {
        assert!(text_width("WWW") > 2.0 * text_width("iii"));
        // An ordinary name stays near the old 0.57em average
        let per_char = text_width("Carol") / 5.0 / FONT;
        assert!((0.45..0.65).contains(&per_char), "{per_char}");
    }
}
