//! Property tests for the authority analysis and the text renderer, over
//! generated scenarios that always resolve.
//!
//! Random source text almost never resolves (names must line up, labels
//! must exist), so these build source from a small cast instead. Every
//! certificate gets an explicit label and subject, revocations only target
//! earlier certificates, and the rest is up to the generator, including
//! delegations nobody had the authority to make.

// Generators and helpers outside `#[test]` fns are still test code
#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::panic
)]

mod common;

use arbitrary::{Arbitrary, Unstructured};
use bolero::check;
use rain_diagram_core::{
    authority::{Authority, Status},
    diagram::{Diagram, Event, EventIx, PrincipalIx},
    level::Level,
    parse,
};
use std::fmt::Write as _;

const CAST: [&str; 4] = ["Doc", "Alice", "Team", "Bob"];

/// What a delegation or invocation is about. Mostly aims at things the
/// actor plausibly holds, so that chains (and transitive routes) form.
#[derive(Debug, Clone, Copy)]
enum Target {
    /// The actor itself: always authorised.
    Own,

    /// Something the actor holds, directly or through groups (ignoring
    /// revocation, so it may not be authorised after all).
    Held(u8),

    /// Anyone at all.
    Any(u8),
}

#[derive(Debug)]
enum Step {
    Delegate {
        issuer: u8,
        audience: u8,
        subject: Target,
        level: Level,
    },
    Invoke {
        by: u8,
        on: Target,
        level: Level,
    },
    Revoke {
        by: u8,
        target: u8,
    },
}

/// One byte per choice, so short fuzzer inputs still tell long stories.
/// Delegations are weighted up, since everything else builds on them.
impl<'a> Arbitrary<'a> for Target {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let (kind, i) = (u8::arbitrary(u)?, u8::arbitrary(u)?);
        Ok(match kind % 8 {
            0..=1 => Self::Own,
            2..=6 => Self::Held(i),
            _ => Self::Any(i),
        })
    }
}

impl<'a> Arbitrary<'a> for Step {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let level = |u: &mut Unstructured<'a>| -> arbitrary::Result<Level> {
            Ok(Level::ALL[usize::from(u8::arbitrary(u)?) % Level::ALL.len()])
        };
        Ok(match u8::arbitrary(u)? % 8 {
            0..=4 => Self::Delegate {
                issuer: u8::arbitrary(u)?,
                audience: u8::arbitrary(u)?,
                subject: Target::arbitrary(u)?,
                level: level(u)?,
            },
            5..=6 => Self::Invoke {
                by: u8::arbitrary(u)?,
                on: Target::arbitrary(u)?,
                level: level(u)?,
            },
            _ => Self::Revoke {
                by: u8::arbitrary(u)?,
                target: u8::arbitrary(u)?,
            },
        })
    }
}
#[derive(Debug, Arbitrary)]
struct Scenario {
    title: bool,

    /// Which adjacent pair `shuffling_is_harmless` tries to swap
    swap: u8,

    /// Regions, as (start, length) seeds over the statements
    regions: Vec<(u8, u8)>,

    /// Labels placed by hand, as (statement, dashes before, dashes after)
    /// seeds, so the placement paths get exercised too
    placements: Vec<(u8, u8, u8)>,

    steps: Vec<Step>,
}

impl Scenario {
    #[allow(
        clippy::too_many_lines,
        reason = "one pass building the source from generated steps"
    )]
    fn source(&self) -> String {
        let pick = |i: u8| usize::from(i) % CAST.len();
        let mut source = String::from("rain\n");
        if self.title {
            source.push_str("title A generated scenario\n");
        }

        // Who has been handed authority over whom
        let mut grants: Vec<(usize, usize)> = Vec::new();
        let aim = |grants: &[(usize, usize)], actor: usize, target: Target| match target {
            Target::Own => actor,
            Target::Any(i) => pick(i),
            Target::Held(i) => {
                let held = holdings(grants, actor);
                if held.is_empty() {
                    actor
                } else {
                    held[usize::from(i) % held.len()]
                }
            }
        };

        let mut lines: Vec<String> = Vec::new();
        // Each certificate's subject, by label number
        let mut subjects: Vec<usize> = Vec::new();
        for (n, step) in self.steps.iter().enumerate() {
            match *step {
                Step::Delegate {
                    issuer,
                    audience,
                    subject,
                    level,
                } => {
                    // Half the time, pass on something already received, so
                    // chains (and so transitive routes and revocation
                    // fallout) form rather than mostly unauthorised one-offs
                    let received = grants.get(usize::from(issuer & 0x7f) % grants.len().max(1));
                    let issuer = match received {
                        Some(&(holder, _)) if issuer & 0x80 != 0 => holder,
                        _ => pick(issuer),
                    };
                    let audience = pick(audience);
                    let subject = aim(&grants, issuer, subject);
                    lines.push(format!(
                        "c{}: ({})-[{}({})]->({}) : step {n}",
                        subjects.len(),
                        CAST[issuer],
                        level.keyword(),
                        CAST[subject],
                        CAST[audience]
                    ));
                    grants.push((audience, subject));
                    subjects.push(subject);
                }
                Step::Invoke { by, on, level } => {
                    let by = pick(by);
                    let on = aim(&grants, by, on);
                    lines.push(format!(
                        "({})=[[{}]]=>({})",
                        CAST[by],
                        level.keyword(),
                        CAST[on]
                    ));
                }
                Step::Revoke { by, target } if !subjects.is_empty() => {
                    // Void scenarios need an effective revocation of a
                    // certificate something later relies on. So mostly
                    // aim upstream (the first half), and mostly have the
                    // subject revoke, since it always may
                    let upstream = if by & 1 == 0 {
                        subjects.len().div_ceil(2)
                    } else {
                        subjects.len()
                    };
                    let target = usize::from(target) % upstream;
                    let by = if (by >> 1) % 4 < 3 {
                        subjects[target]
                    } else {
                        pick(by)
                    };
                    lines.push(format!("({})-<revoke>-x[c{target}]", CAST[by]));
                }
                Step::Revoke { .. } => {}
            }
        }

        // Some labels placed by hand: 1–4 dashes either side, at most one
        // placement per statement
        if !lines.is_empty() {
            for &(n, before, after) in &self.placements {
                let n = usize::from(n) % lines.len();
                if !(lines[n].contains("--") || lines[n].contains("==")) {
                    lines[n] = place(
                        &lines[n],
                        usize::from(before % 4) + 1,
                        usize::from(after % 4) + 1,
                    );
                }
            }
        }

        // Regions over non-empty runs of statements, closed innermost first
        let len = lines.len();
        let regions: Vec<(usize, usize)> = if len == 0 {
            Vec::new()
        } else {
            self.regions
                .iter()
                .map(|&(start, length)| {
                    let start = usize::from(start) % len;
                    (start, start + 1 + usize::from(length) % (len - start))
                })
                .collect()
        };
        for at in 0..=len {
            for (k, _) in regions.iter().enumerate().rev().filter(|(_, r)| r.1 == at) {
                let _ = writeln!(source, "<<< r{k}");
            }
            for (k, _) in regions.iter().enumerate().filter(|(_, r)| r.0 == at) {
                let _ = writeln!(source, ">>> r{k}: region {k}");
            }
            if let Some(line) = lines.get(at) {
                source.push_str(line);
                source.push('\n');
            }
        }
        source
    }

    fn diagram(&self) -> (String, Diagram) {
        let source = self.source();
        let diagram = parse(&source);
        assert!(
            diagram.is_ok(),
            "scenarios always resolve: {diagram:?}\n{source}"
        );
        let Ok(diagram) = diagram else {
            unreachable!("just asserted")
        };
        (source, diagram)
    }
}

/// `line` with its label placed by hand: `before` and `after` dashes (or
/// `=` for an invocation) either side of it.
fn place(line: &str, before: usize, after: usize) -> String {
    let (dash, dash_after) = ("-".repeat(before), "-".repeat(after));
    let (eq, eq_after) = ("=".repeat(before), "=".repeat(after));
    if line.contains("<revoke>") {
        line.replacen("-<revoke>-x", &format!("{dash}<revoke>{dash_after}x"), 1)
    } else if line.contains("=[[") {
        line.replacen("=[[", &format!("{eq}[["), 1)
            .replacen("]]=>", &format!("]]{eq_after}>"), 1)
    } else {
        line.replacen("-[", &format!("{dash}["), 1)
            .replacen("]->", &format!("]{dash_after}>"), 1)
    }
}

fn certs(d: &Diagram) -> impl Iterator<Item = (EventIx, &rain_diagram_core::diagram::Cert)> {
    d.event_ixs().filter_map(|ix| Some((ix, d.cert(ix)?)))
}

/// Who acts, on what, for any event that needs authority.
fn actor(event: &Event) -> Option<(PrincipalIx, PrincipalIx)> {
    match event {
        Event::Delegate(c) => Some((c.issuer, c.subject)),
        Event::Invoke(i) => Some((i.by, i.on)),
        Event::Revoke(_) => None,
    }
}

/// Every route is a well-formed chain of earlier certificates from the
/// subject to the actor, and a certificate never grants more than it
/// claims or than its route carries.
#[test]
fn routes_are_causal_chains_and_attenuate() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();

        for ix in d.event_ixs() {
            let event = d.event(ix).expect("indices from event_ixs");
            let Some(route) = event.authority().route() else {
                continue;
            };

            let chain: Vec<_> = route.certs().collect();
            assert!(
                chain.iter().all(|&c| c < ix),
                "routes only use the past\n{source}"
            );
            assert!(
                chain.iter().all(|&c| d.cert(c).is_some()),
                "routes are certificates\n{source}"
            );

            // source → through… → actor, each hop's audience is the next
            // hop's subject
            let hops: Vec<_> = chain.iter().filter_map(|&c| d.cert(c)).collect();
            for pair in hops.windows(2) {
                assert_eq!(pair[0].audience, pair[1].subject, "{source}");
            }
            if let (Some((actor, subject)), Some(first), Some(last)) =
                (actor(event), hops.first(), hops.last())
            {
                assert_eq!(first.subject, subject, "{source}");
                assert_eq!(last.audience, actor, "{source}");
            }

            if let Event::Delegate(cert) = event {
                let granted = d.granted(ix).expect("routed certificates are authorised");
                assert!(granted <= cert.level, "{source}");
                for &c in &chain {
                    assert!(Some(granted) <= d.granted(c), "{source}");
                }
            }
        }

        for (ix, cert) in certs(&d) {
            assert_eq!(
                d.granted(ix).is_some(),
                cert.authority.is_authorized(),
                "{source}"
            );
            if cert.issuer == cert.subject {
                assert_eq!(cert.authority, Authority::Root, "{source}");
            }
        }
    });
}

/// Once something is revoked or void, it stays that way; unauthorised
/// events never become valid; nothing has a status before it happens.
#[test]
fn status_only_gets_worse_with_time() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();

        for ix in d.event_ixs() {
            let mut previous = None;
            for now in d.event_ixs() {
                let status = d.status(ix, now);
                assert_eq!(status.is_none(), now < ix, "{source}");
                match (previous, status) {
                    (Some(Status::Unauthorized), s) => {
                        assert_eq!(s, Some(Status::Unauthorized), "{source}");
                    }
                    (Some(p @ (Status::Revoked { .. } | Status::Void { .. })), s) => {
                        assert_eq!(s, Some(p), "first revocation wins\n{source}");
                    }
                    _ => {}
                }
                previous = status;
            }
        }
    });
}

/// Revocation is exactly "a dependency was revoked": the cause of a void
/// event is something it depends on, and dependents always come later.
#[test]
fn void_means_a_dependency_was_revoked() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let Some(now) = d.now() else { return };

        for (cert, _) in certs(&d) {
            let dependents: Vec<_> = d.dependents(cert).collect();
            assert!(dependents.iter().all(|&e| e > cert), "{source}");

            if let Some(Status::Revoked { revocation }) = d.status(cert, now) {
                // Revocations stay in force once effective, even one that
                // revokes its own power (leaving a group)
                let voidable = dependents
                    .into_iter()
                    .filter(|&e| !matches!(d.event(e), Some(Event::Revoke(_))));
                for e in voidable {
                    assert!(
                        matches!(
                            d.status(e, now),
                            Some(
                                Status::Void { .. } | Status::Revoked { .. } | Status::Unauthorized
                            )
                        ),
                        "everything relying on a revoked certificate is void\n{source}"
                    );
                }
                let Some(Event::Revoke(r)) = d.event(revocation) else {
                    panic!("revoked by a revocation\n{source}");
                };
                assert_eq!(r.target, cert, "{source}");
            }
        }

        for ix in d.event_ixs() {
            if let Some(Status::Void { revoked, .. }) = d.status(ix, now) {
                assert!(d.dependents(revoked).any(|e| e == ix), "{source}");
            }
        }
    });
}

/// A certificate that's still valid shows up in its audience's reach, at
/// least at the level it grants.
#[test]
fn valid_certificates_are_reachable() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let Some(now) = d.now() else { return };

        for (ix, cert) in certs(&d) {
            if d.status(ix, now) != Some(Status::Valid) || cert.audience == cert.subject {
                continue;
            }
            let granted = d.granted(ix).expect("valid certificates are authorised");
            let reach = d.reach(cert.audience, now);
            assert!(
                reach
                    .iter()
                    .any(|r| r.subject == cert.subject && r.level >= granted),
                "{source}"
            );
        }
    });
}

/// The text art has three lines per event plus the header, no trailing
/// whitespace, and unbroken lifelines.
#[test]
fn text_art_is_a_grid() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let art = d.to_text();
        let art = if scenario.title {
            art.strip_prefix("A generated scenario\n\n")
                .expect("title comes first")
        } else {
            &art
        };

        let lines: Vec<&str> = art.lines().collect();
        if d.principals().is_empty() {
            assert!(lines.is_empty(), "{source}");
            return;
        }
        assert_eq!(lines.len(), 3 * d.events().len() + 1, "{source}\n{art}");
        assert!(art.ends_with('\n'));

        for line in &lines {
            assert_eq!(line.trim_end(), *line, "no trailing whitespace\n{art}");
        }

        // Every principal's name is centred over a column that holds a
        // non-space character on every line below it
        let header: Vec<char> = lines[0].chars().collect();
        for principal in d.principals() {
            let name = format!("({})", principal.name());
            let start = lines[0].find(&name).expect("names are in the header");
            let column = lines[0][..start].chars().count() + name.chars().count() / 2;
            assert!(column < header.len().max(column + 1));
            for line in &lines[1..] {
                let c = line.chars().nth(column);
                assert!(
                    c.is_some_and(|c| c != ' '),
                    "lifeline of {name} broken at column {column}\n{art}"
                );
            }
        }
    });
}

/// Guards the properties above against passing vacuously: the generator
/// must actually reach transitive routes, attenuation, and fallout.
#[test]
fn generator_is_not_vacuous() {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    static TRANSITIVE: AtomicUsize = AtomicUsize::new(0);
    static ATTENUATED: AtomicUsize = AtomicUsize::new(0);
    static VOID: AtomicUsize = AtomicUsize::new(0);

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (_, d) = scenario.diagram();
        RUNS.fetch_add(1, Relaxed);
        let Some(now) = d.now() else { return };

        for (ix, cert) in certs(&d) {
            if cert
                .authority
                .route()
                .is_some_and(|r| !r.through().is_empty())
            {
                TRANSITIVE.fetch_add(1, Relaxed);
            }
            if d.granted(ix).is_some_and(|g| g < cert.level) {
                ATTENUATED.fetch_add(1, Relaxed);
            }
        }
        if d.event_ixs()
            .any(|ix| matches!(d.status(ix, now), Some(Status::Void { .. })))
        {
            VOID.fetch_add(1, Relaxed);
        }
    });

    let counts = [&RUNS, &TRANSITIVE, &ATTENUATED, &VOID].map(|c| c.load(Relaxed));
    eprintln!("runs, transitive, attenuated, void: {counts:?}");
    assert!(counts.iter().all(|&n| n > 0), "{counts:?}");
}

/// Everyone `actor` holds authority over, through any chain of grants.
fn holdings(grants: &[(usize, usize)], actor: usize) -> Vec<usize> {
    let mut held = std::collections::BTreeSet::new();
    let mut frontier = vec![actor];
    while let Some(p) = frontier.pop() {
        for &(audience, subject) in grants {
            if audience == p && subject != actor && held.insert(subject) {
                frontier.push(subject);
            }
        }
    }
    held.into_iter().collect()
}

/// The SVG is well-formed XML with one group per event, whatever the names
/// and notes contain.
#[test]
fn svg_is_well_formed() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let svg = d.to_svg();

        let mut open: Vec<&str> = Vec::new();
        let mut rest = svg.as_str();
        while let Some(start) = rest.find('<') {
            let end = rest[start..].find('>').expect("tags close") + start;
            let tag = &rest[start + 1..end];
            rest = &rest[end + 1..];
            let name = tag
                .trim_start_matches('/')
                .split([' ', '/'])
                .next()
                .unwrap_or_default();
            if tag.starts_with('/') {
                assert_eq!(open.pop(), Some(name), "{source}");
            } else if !tag.ends_with('/') {
                open.push(name);
            }
            // Text between tags never contains markup characters
            let text = &rest[..rest.find('<').unwrap_or(rest.len())];
            assert!(!text.contains('>'), "unescaped text {text:?}\n{source}");
        }
        assert!(open.is_empty(), "unclosed {open:?}\n{source}");
        assert_eq!(
            svg.matches(r#"<g class="event "#).count(),
            d.events().len(),
            "{source}"
        );
    });
}

/// Names and notes are escaped in the SVG.
#[test]
fn svg_escapes_markup() -> Result<(), rain_diagram_core::Error> {
    let d = parse("rain\n(<b>&\"x\")-[edit]->(Bob) : 1 < 2 & \"so\" <script>")?;
    let svg = d.to_svg();
    assert!(!svg.contains("<b>") && !svg.contains("<script>"));
    assert!(svg.contains("&lt;b&gt;&amp;&quot;x&quot;"));
    Ok(())
}

/// Re-emit a diagram as source with every route pinned by `via` (as a
/// Keyhive certificate names its proof), optionally swapping two events.
fn pinned_source(d: &Diagram, swap: Option<usize>) -> String {
    let name = |p: PrincipalIx| d.principal(p).map_or("", |p| p.name().as_str());
    let label = |ix: EventIx| d.cert(ix).map_or("", |c| c.id.as_str());

    let mut order: Vec<EventIx> = d.event_ixs().collect();
    if let Some(i) = swap {
        order.swap(i, i + 1);
    }

    let mut source = String::from("rain\n");
    for ix in order {
        let line = match d.event(ix).expect("indices from event_ixs") {
            Event::Delegate(c) => {
                let via = c.authority.route().map_or_else(String::new, |r| {
                    let holder_first: Vec<_> = r
                        .through()
                        .iter()
                        .rev()
                        .chain([&r.source()])
                        .map(|&v| label(v))
                        .collect();
                    format!(" via {}", holder_first.join(", "))
                });
                format!(
                    "{}: ({})-[{}({}){via}]->({})",
                    c.id,
                    name(c.issuer),
                    c.level.keyword(),
                    name(c.subject),
                    name(c.audience)
                )
            }
            Event::Invoke(i) => format!(
                "({})=[[{}]]=>({})",
                name(i.by),
                i.level.keyword(),
                name(i.on)
            ),
            Event::Revoke(r) => format!("({})-<revoke>-x[{}]", name(r.by), label(r.target)),
        };
        source.push_str(&line);
        source.push('\n');
    }
    source
}

/// The certificates an event relies on directly: its route, or a
/// revocation's target and power.
fn uses(d: &Diagram, ix: EventIx) -> Vec<EventIx> {
    let Some(event) = d.event(ix) else {
        return Vec::new();
    };
    let mut uses: Vec<_> = event
        .authority()
        .route()
        .map(|r| r.certs().collect())
        .unwrap_or_default();
    if let Event::Revoke(r) = event {
        uses.push(r.target);
    }
    uses
}

/// [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/): "you can drag a certificate anywhere below what it relies
/// on and above what relies on it, and the outcome never changes."
///
/// Swaps a delegation with an adjacent event when neither relies on the
/// other and both were authorised, with routes pinned, and checks every
/// certificate ends up equally valid.
#[test]
fn shuffling_is_harmless() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (_, d) = scenario.diagram();
        let Some(now) = d.now() else { return };
        if d.events().len() < 2 {
            return;
        }

        let i = usize::from(scenario.swap) % (d.events().len() - 1);
        let (a, b) = (
            d.event_ix(i).expect("in range"),
            d.event_ix(i + 1).expect("in range"),
        );
        let authorised = |ix| {
            d.event(ix)
                .is_some_and(|e: &Event| e.authority().is_authorized())
        };
        let delegation = |ix| matches!(d.event(ix), Some(Event::Delegate(_)));
        if !(delegation(a) || delegation(b))
            || !(authorised(a) && authorised(b))
            || uses(&d, b).contains(&a)
        {
            return;
        }

        let original = pinned_source(&d, None);
        let shuffled = pinned_source(&d, Some(i));
        let before = parse(&original).expect("pinned source resolves");
        let after = parse(&shuffled).expect("shuffled source resolves");

        let valid = |d: &Diagram| -> std::collections::BTreeMap<String, bool> {
            let Some(now) = d.now() else {
                return std::collections::BTreeMap::default();
            };
            certs(d)
                .map(|(ix, c)| (c.id.to_string(), d.status(ix, now) == Some(Status::Valid)))
                .collect()
        };
        // Pinning changes nothing by itself
        assert_eq!(
            valid(&before),
            certs(&d)
                .map(|(ix, c)| (c.id.to_string(), d.status(ix, now) == Some(Status::Valid)))
                .collect(),
            "{original}"
        );
        assert_eq!(
            valid(&before),
            valid(&after),
            "\n{original}\nvs\n{shuffled}"
        );
    });
}

/// Certflow snapshots are well-formed, deterministic, and free of NaNs
/// whatever the layout has to cope with.
#[test]
fn certflow_is_well_formed() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let svg = d.to_certflow_svg();

        // The stylesheet legitimately says `infinite`; the geometry mustn't
        let drawing = svg.split("</style>").nth(1).unwrap_or_default();
        assert!(
            !drawing.contains("NaN") && !drawing.contains("inf"),
            "{source}"
        );
        assert_eq!(svg, d.to_certflow_svg(), "deterministic\n{source}");
        let drawn = d
            .events()
            .iter()
            .filter(|e| !matches!(e.node, Event::Invoke(_)))
            .count();
        assert_eq!(
            svg.matches(r#"<g class="event "#).count(),
            drawn,
            "{source}"
        );
        assert_eq!(
            svg.matches("<g").count(),
            svg.matches("</g>").count(),
            "{source}"
        );
    });
}

/// Dragging a row anywhere `movable` allows rewrites the source so that
/// exactly that statement moves, or fails only because the result wouldn't
/// parse (an emptied region).
#[test]
fn moves_within_range_rearrange_the_source() {
    use rain_diagram_core::edit::{MoveError, move_statement};

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let lines = |d: &Diagram, source: &str| -> Vec<String> {
            d.events()
                .iter()
                .map(|e| source[e.span.start..e.span.end].to_string())
                .collect()
        };
        let before = lines(&d, &source);

        for from in d.event_ixs() {
            let range = d.movable(from);
            assert!(range.contains(&from.index()), "{source}");
            for to in [*range.start(), *range.end()] {
                match move_statement(&source, from.index(), to) {
                    Ok(moved) => {
                        let after = parse(&moved).expect("moves are checked");
                        let mut expected = before.clone();
                        let line = expected.remove(from.index());
                        expected.insert(to, line);
                        assert_eq!(lines(&after, &moved), expected, "{source}\n→\n{moved}");
                    }
                    Err(MoveError::Result(_)) => {}
                    Err(e) => panic!("{e}\n{source}"),
                }
            }
        }
    });
}

/// Deleting a statement removes exactly that statement, unless a
/// revocation names it, and never changes what the other revocations
/// target.
#[test]
fn deleting_removes_exactly_one_statement() {
    use rain_diagram_core::edit::{DeleteError, delete_statement};

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let lines = |d: &Diagram, source: &str| -> Vec<String> {
            d.events()
                .iter()
                .map(|e| source[e.span.start..e.span.end].to_string())
                .collect()
        };
        let before = lines(&d, &source);

        for ix in d.event_ixs() {
            let revoked = d
                .event_ixs()
                .any(|j| matches!(d.event(j), Some(Event::Revoke(r)) if r.target == ix));
            match delete_statement(&source, ix.index()) {
                Ok(out) => {
                    assert!(!revoked, "{source}");
                    let after = parse(&out).expect("deletions are checked");
                    let mut expected = before.clone();
                    expected.remove(ix.index());
                    assert_eq!(lines(&after, &out), expected, "{source}\n→\n{out}");
                }
                Err(DeleteError::Revoked { .. }) => assert!(revoked, "{source}"),
                Err(DeleteError::Result(_)) => {}
                Err(e) => panic!("{e}\n{source}"),
            }
        }
    });
}

/// At every moment, exactly the rows from that moment on are still to
/// come, and the certflow snapshot of that moment is well-formed.
#[test]
fn every_moment_renders() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let n = d.events().len();
        for happened in 0..=n {
            let rain = d.to_rain_svg_at(happened);
            assert_eq!(
                rain.matches(" future\" data-event=").count(),
                n - happened,
                "{source}"
            );
            assert!(
                rain.contains(&format!(r#"data-moment="{happened}""#)),
                "{source}"
            );

            let certflow = d.to_certflow_svg_at(happened);
            let drawing = certflow.split("</style>").nth(1).unwrap_or_default();
            assert!(
                !drawing.contains("NaN") && !drawing.contains("inf"),
                "{source}"
            );
            assert_eq!(
                certflow.matches("<g").count(),
                certflow.matches("</g>").count(),
                "{source}"
            );
        }
    });
}

/// Dragging a region's edges to any rows either works, leaving exactly
/// those rows inside, or is refused because the result wouldn't parse.
#[test]
fn regions_can_cover_any_rows() {
    use rain_diagram_core::edit::{RegionError, set_region_rows};

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let n = d.events().len();
        let Some(region) = d.regions().first() else {
            return;
        };
        let id = region.id.as_str().to_string();
        let seed = usize::from(scenario.swap);
        let first = seed % n;
        let last = first + (seed / 7) % (n - first);

        match set_region_rows(&source, &id, first, last) {
            Ok(moved) => {
                let after = parse(&moved).expect("checked");
                let region = after
                    .regions()
                    .iter()
                    .find(|r| r.id.as_str() == id)
                    .expect("kept");
                assert_eq!(
                    (region.first.index(), region.last.index()),
                    (first, last),
                    "{moved}"
                );
                assert_eq!(after.events().len(), n, "{moved}");
            }
            Err(RegionError::Result(_)) => {}
            Err(e) => panic!("{e}\n{source}"),
        }
    });
}

/// Extra random choices for the edit properties, drawn alongside a scenario.
#[derive(Debug, Arbitrary)]
struct Choices {
    at: u8,
    from: u8,
    to: u8,
    level: u8,
    percent: u8,
    revoke: bool,
}

fn scenario_and_choices(bytes: &[u8]) -> Option<(String, Diagram, Choices)> {
    let (scenario, choices) = common::arbitrary::<(Scenario, Choices)>(bytes)?;
    let (source, d) = scenario.diagram();
    Some((source, d, choices))
}

/// The opening tag of event `ix`'s group in `svg`.
fn event_group(svg: &str, ix: EventIx) -> &str {
    let hook = format!(r#"data-event="{}""#, ix.index());
    svg.match_indices(r#"<g class="event "#)
        .map(|(at, _)| &svg[at..at + svg[at..].find("</g>").expect("groups close")])
        .find(|group| group[..group.find('>').expect("tags close")].contains(&hook))
        .expect("every event has a group")
}

/// A revocation with no effect never draws a cross on its target, in either
/// renderer. (The SVG is checked per revocation; the text art can only be
/// checked overall, since crosses aren't tied to their revocation there.)
#[test]
fn ineffective_revocations_never_strike() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let svg = d.to_rain_svg();
        let mut any_effective = false;
        for ix in d.event_ixs() {
            let Some(Event::Revoke(r)) = d.event(ix) else {
                continue;
            };
            let group = event_group(&svg, ix);
            if r.authority.is_authorized() {
                any_effective = true;
                assert!(group.contains(r#"class="cross""#), "{source}\n{group}");
            } else {
                assert!(!group.contains(r#"class="cross""#), "{source}\n{group}");
            }
        }
        if !any_effective {
            assert!(!d.to_text().contains('✗'), "{source}");
        }
    });
}

/// Every event's SVG class is its status as of the last event.
#[test]
fn svg_classes_match_statuses() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let Some(now) = d.now() else { return };
        let svg = d.to_rain_svg();
        for ix in d.event_ixs() {
            let expected = match d.status(ix, now).expect("happened") {
                Status::Valid => "valid",
                Status::Unauthorized => "unauthorized",
                Status::Revoked { .. } => "revoked",
                Status::Void { .. } => "void",
            };
            let group = event_group(&svg, ix);
            let class = &group[..group.find('>').expect("tags close")];
            assert!(
                class
                    .split('"')
                    .nth(1)
                    .is_some_and(|c| c.split(' ').any(|w| w == expected)),
                "#{} should be {expected}: {class}\n{source}",
                ix.index()
            );
        }
    });
}

/// Inserting a delegation or invocation and then deleting it gives back
/// the source; an inserted revocation revokes the certificate it was given.
#[test]
fn inserted_statements_land_where_asked() {
    use rain_diagram_core::edit::{InsertError, NewStatement, delete_statement, insert_statement};

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some((source, d, c)) = scenario_and_choices(bytes) else {
            return;
        };
        let columns = d.principals().len();
        if columns == 0 {
            return;
        }
        let at = usize::from(c.at) % (d.events().len() + 1);
        let (from, to) = (usize::from(c.from) % columns, usize::from(c.to) % columns);
        let level = Level::ALL[usize::from(c.level) % Level::ALL.len()];

        let certs: Vec<EventIx> = certs(&d).map(|(ix, _)| ix).collect();
        let new = match (c.revoke, certs.get(usize::from(c.to) % certs.len().max(1))) {
            (true, Some(&target)) => NewStatement::Revoke {
                by: from,
                target: target.index(),
            },
            _ if c.level % 2 == 0 => NewStatement::Delegate {
                issuer: from,
                audience: to,
                level,
            },
            _ => NewStatement::Invoke {
                by: from,
                on: to,
                level,
            },
        };

        let (out, landed) = match insert_statement(&source, at, new) {
            Ok(done) => done,
            // Allowed: the new line makes some later inference ambiguous
            Err(InsertError::Result(_)) => return,
            Err(e) => panic!("{e}\n{source}"),
        };
        let after = parse(&out).expect("insertions are checked");
        assert_eq!(
            after.events().len(),
            d.events().len() + 1,
            "{source}\n→\n{out}"
        );

        match new {
            NewStatement::Revoke { target, .. } => {
                let Some(Event::Revoke(r)) = after.event_ix(landed).and_then(|ix| after.event(ix))
                else {
                    panic!("no revocation at {landed}\n{out}");
                };
                assert_eq!(r.target.index(), target, "{out}");
            }
            NewStatement::Delegate { .. } | NewStatement::Invoke { .. } => {
                assert_eq!(landed, at, "{out}");
                let back = delete_statement(&out, landed).expect("a fresh statement can go");
                assert_eq!(back, source, "{out}");
            }
        }
    });
}

/// Placing a label and putting it back to automatic gives the same
/// statements as putting the original back (the spacing before a note may
/// differ), and the placement is what was asked.
#[test]
fn placements_round_trip() {
    use rain_diagram_core::{ast::Placement, edit::set_position, parser::document};

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some((source, d, c)) = scenario_and_choices(bytes) else {
            return;
        };
        if d.events().is_empty() {
            return;
        }
        let n = usize::from(c.at) % d.events().len();
        let percent = u16::from(c.percent) % 101;

        let placed = set_position(&source, n, Some(percent)).expect("any statement can be placed");
        let doc = document(&placed).expect("placements are checked");
        let at = match &doc.statements[n].node {
            rain_diagram_core::ast::Statement::Delegate(s) => s.at,
            rain_diagram_core::ast::Statement::Invoke(s) => s.at,
            rain_diagram_core::ast::Statement::Revoke(s) => s.at,
        };
        assert_eq!(
            at,
            Placement::nearest(f64::from(percent) / 100.0),
            "{placed}"
        );

        let back = set_position(&placed, n, None).expect("and put back");
        assert_eq!(
            document(&back).expect("parses").to_string(),
            // (the generated source may already have placed this one)
            document(&set_position(&source, n, None).expect("unplaceable"))
                .expect("parses")
                .to_string(),
            "{source}\n→\n{placed}\n→\n{back}"
        );
    });
}

/// Moving a principal's column reorders the columns as asked and changes
/// nothing about who can do what.
#[test]
fn moving_principals_only_reorders() {
    use rain_diagram_core::edit::move_principal;

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some((source, d, c)) = scenario_and_choices(bytes) else {
            return;
        };
        let columns = d.principals().len();
        if columns == 0 {
            return;
        }
        let (from, to) = (usize::from(c.from) % columns, usize::from(c.to) % columns);
        let out = move_principal(&source, from, to).expect("columns in range");
        let after = parse(&out).expect("reordering is safe");

        let mut expected: Vec<&str> = d.principals().iter().map(|p| p.name().as_str()).collect();
        let moved = expected.remove(from);
        expected.insert(to, moved);
        let order: Vec<&str> = after
            .principals()
            .iter()
            .map(|p| p.name().as_str())
            .collect();
        assert_eq!(order, expected, "{out}");

        let (Some(now), Some(now_after)) = (d.now(), after.now()) else {
            return;
        };
        for (a, b) in d.event_ixs().zip(after.event_ixs()) {
            assert_eq!(
                d.status(a, now),
                after.status(b, now_after),
                "{source}\n→\n{out}"
            );
        }
    });
}

/// Changing a region's note and changing it back gives back the source.
#[test]
fn region_notes_round_trip() {
    use rain_diagram_core::edit::set_region_note;

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some((source, d, c)) = scenario_and_choices(bytes) else {
            return;
        };
        if d.regions().is_empty() {
            return;
        }
        let k = usize::from(c.at) % d.regions().len();
        let region = d.regions()[k].id.as_str().to_owned();
        let original = d.regions()[k].caption().to_owned();

        let changed = set_region_note(&source, &region, "A different caption").expect("valid note");
        assert_eq!(
            parse(&changed).expect("parses").regions()[k].caption(),
            "A different caption"
        );
        assert_eq!(
            set_region_note(&changed, &region, &original).expect("back"),
            source
        );
    });
}

/// Every powerline carries the status of the event it feeds, so authority
/// never visibly flows into something void or revoked (the stylesheet
/// stops those animating). Checked in both notations.
#[test]
fn powerlines_carry_their_consumers_status() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
            return;
        };
        let (source, d) = scenario.diagram();
        let Some(now) = d.now() else { return };
        for svg in [d.to_rain_svg(), d.to_certflow_svg()] {
            for (at, _) in svg.match_indices(r#"<path class="powerline"#) {
                let tag = &svg[at..at + svg[at..].find("/>").expect("paths close")];
                let to: usize = tag
                    .split(r#"data-to=""#)
                    .nth(1)
                    .and_then(|rest| rest.split('"').next())
                    .and_then(|n| n.parse().ok())
                    .expect("powerlines say what they feed");
                let ix = d.event_ix(to).expect("in range");
                let expected = match d.status(ix, now).expect("happened") {
                    Status::Valid => "valid",
                    Status::Unauthorized => "unauthorized",
                    Status::Revoked { .. } => "revoked",
                    Status::Void { .. } => "void",
                };
                assert!(
                    tag.starts_with(&format!(r#"<path class="powerline {expected}""#)),
                    "{tag}\n{source}"
                );
            }
        }
    });
}

/// Wherever a label is placed by hand, the text art still draws it whole on
/// its own row, and the grid keeps its shape.
#[test]
fn placed_labels_render_on_their_row() {
    use rain_diagram_core::edit::set_position;

    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some((source, d, c)) = scenario_and_choices(bytes) else {
            return;
        };
        if d.events().is_empty() {
            return;
        }
        let n = usize::from(c.at) % d.events().len();
        let placed = set_position(&source, n, Some(u16::from(c.percent) % 101)).expect("placeable");
        let after = parse(&placed).expect("placements are checked");
        let text = after.to_text();
        let lines: Vec<&str> = text.lines().collect();

        // Header, then three lines per event (after any title)
        let title = usize::from(after.title().is_some()) * 2;
        assert_eq!(
            lines.len(),
            title + 1 + 3 * after.events().len(),
            "{placed}\n{text}"
        );

        let label = match after.event(after.event_ix(n).expect("in range")) {
            Some(Event::Delegate(cert)) => format!("[{}]", cert.level),
            Some(Event::Invoke(invoke)) => format!("[[{}]]", invoke.level),
            Some(Event::Revoke(_)) => String::from("<revoke>"),
            None => unreachable!("in range"),
        };
        let row = lines[title + 2 + 3 * n];
        assert!(
            row.contains(&label),
            "{label} missing from row {n}: {row:?}\n{placed}\n{text}"
        );
    });
}

/// Rain SVG geometry: the coordinates the picture's meaning depends on.
/// (Exact positions aren't pinned; relationships between shapes are.)
#[allow(
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::type_complexity,
    reason = "small coordinates and short geometric names in a test"
)]
mod geometry {
    use super::{Event, Scenario, check, common, event_group};
    use std::collections::BTreeMap;

    /// Every element's opening tag, as (name, attributes).
    fn tags(svg: &str) -> Vec<(&str, BTreeMap<&str, &str>)> {
        let mut out = Vec::new();
        let mut rest = svg;
        while let Some(start) = rest.find('<') {
            let end = start + rest[start..].find('>').expect("tags close");
            let tag = &rest[start + 1..end];
            rest = &rest[end + 1..];
            if tag.starts_with('/') || tag.starts_with('!') {
                continue;
            }
            let name = tag.split([' ', '/']).next().unwrap_or_default();
            let mut attrs = BTreeMap::new();
            let mut body = tag[name.len()..].trim_end_matches('/');
            while let Some(eq) = body.find("=\"") {
                let key = body[..eq].trim();
                let value_end = eq + 2 + body[eq + 2..].find('"').expect("quoted");
                attrs.insert(key, &body[eq + 2..value_end]);
                body = &body[value_end + 1..];
            }
            out.push((name, attrs));
        }
        out
    }

    fn num(attrs: &BTreeMap<&str, &str>, key: &str) -> f64 {
        attrs[key]
            .parse()
            .unwrap_or_else(|_| panic!("{key}={:?}", attrs[key]))
    }

    /// The numbers in a path's `d`, as points (H/V carry one number).
    fn points(d: &str) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        let mut command = 'M';
        for token in d.split_whitespace() {
            let (head, rest) = match token.chars().next() {
                Some(c) if c.is_ascii_alphabetic() => (Some(c), &token[1..]),
                _ => (None, token),
            };
            if let Some(c) = head {
                command = c;
            }
            let last = out.last().copied().unwrap_or((0.0, 0.0));
            let mut numbers = rest.split(',').filter_map(|n| n.parse::<f64>().ok());
            match command {
                'H' => out.extend(numbers.next().map(|x| (x, last.1))),
                'V' => out.extend(numbers.next().map(|y| (last.0, y))),
                _ => {
                    if let (Some(x), Some(y)) = (numbers.next(), numbers.next()) {
                        out.push((x, y));
                    }
                }
            }
        }
        out
    }

    /// A box, as (left, top, right, bottom).
    type Bbox = (f64, f64, f64, f64);

    fn grow((l, t, r, b): Bbox, by: f64) -> Bbox {
        (l - by, t - by, r + by, b + by)
    }

    fn inside((x, y): (f64, f64), (l, t, r, b): Bbox) -> bool {
        l <= x && x <= r && t <= y && y <= b
    }

    #[allow(clippy::too_many_lines)]
    #[test]
    fn rain_geometry_holds_together() {
        check!().with_type::<Vec<u8>>().for_each(|bytes| {
            let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
                return;
            };
            let (source, d) = scenario.diagram();
            let svg = d.to_rain_svg();
            let all = tags(&svg);
            let root = &all[0].1;
            let (top, height) = (num(root, "data-rows-top"), num(root, "data-row-height"));
            let view: Vec<f64> = root["viewBox"]
                .split(' ')
                .filter_map(|n| n.parse().ok())
                .collect();
            let frame = (0.0, 0.0, view[2], view[3]);

            // Columns: left to right, evenly spaced; lifeline and head on it
            let mut columns = Vec::new();
            let mut current_group: Option<&BTreeMap<&str, &str>> = None;
            let mut events: BTreeMap<usize, Vec<(&str, &BTreeMap<&str, &str>)>> = BTreeMap::new();
            let mut event: Option<usize> = None;
            for (name, attrs) in &all {
                if *name == "g" {
                    current_group = Some(attrs);
                    event = attrs
                        .get("class")
                        .filter(|c| c.starts_with("event "))
                        .and_then(|_| attrs.get("data-event"))
                        .and_then(|n| n.parse().ok());
                    if attrs.get("class") == Some(&"principal") {
                        columns.push(num(attrs, "data-x"));
                    }
                    continue;
                }
                if let Some(i) = event {
                    events.entry(i).or_default().push((name, attrs));
                }
                let in_principal =
                    current_group.is_some_and(|g| g.get("class") == Some(&"principal"));
                if in_principal {
                    let x = *columns.last().expect("a principal");
                    match *name {
                        "line" => assert!(
                            (num(attrs, "x1") - x).abs() < 0.05
                                && (num(attrs, "x2") - x).abs() < 0.05,
                            "{source}"
                        ),
                        "ellipse" => assert!((num(attrs, "cx") - x).abs() < 0.05, "{source}"),
                        _ => {}
                    }
                }
                // Everything inside the frame
                for (kx, ky) in [("x", "y"), ("cx", "cy"), ("x1", "y1"), ("x2", "y2")] {
                    if let (Some(x), Some(y)) = (attrs.get(kx), attrs.get(ky)) {
                        let p = (x.parse().expect("x"), y.parse().expect("y"));
                        assert!(
                            inside(p, grow(frame, 0.5)),
                            "{name} at {p:?} outside {frame:?}\n{source}"
                        );
                    }
                }
            }
            assert_eq!(columns.len(), d.principals().len(), "{source}");
            for pair in columns.windows(2) {
                assert!(
                    pair[1] > pair[0],
                    "columns left to right: {columns:?}\n{source}"
                );
                assert!(
                    ((pair[1] - pair[0]) - (columns[1] - columns[0])).abs() < 0.05,
                    "even spacing: {columns:?}"
                );
            }
            let column = |p: rain_diagram_core::diagram::PrincipalIx| columns[p.index()];

            // Each event's label box: pills and diamonds
            let boxes: BTreeMap<usize, Bbox> = events
                .iter()
                .filter_map(|(&i, parts)| {
                    parts
                        .iter()
                        .find_map(|(name, a)| match (*name, a.get("class")) {
                            ("rect", Some(&"pill")) => {
                                let (x, y) = (num(a, "x"), num(a, "y"));
                                Some((i, (x, y, x + num(a, "width"), y + num(a, "height"))))
                            }
                            ("path", Some(&"diamond")) => {
                                let ps = points(a["d"]);
                                let xs = ps.iter().map(|p| p.0);
                                let ys = ps.iter().map(|p| p.1);
                                Some((
                                    i,
                                    (
                                        xs.clone().fold(f64::MAX, f64::min),
                                        ys.clone().fold(f64::MAX, f64::min),
                                        xs.fold(f64::MIN, f64::max),
                                        ys.fold(f64::MIN, f64::max),
                                    ),
                                ))
                            }
                            _ => None,
                        })
                })
                .collect();

            for ix in d.event_ixs() {
                let i = ix.index();
                let row = top + height * (i as f64 + 0.5);
                let parts = &events[&i];
                let bbox = boxes[&i];
                // On its row
                assert!(
                    (f64::midpoint(bbox.1, bbox.3) - row).abs() < 0.6,
                    "#{i} off its row\n{source}"
                );

                if let Some(Event::Delegate(cert)) = d.event(ix) {
                    let (a, b) = (column(cert.issuer), column(cert.audience));
                    let line = parts
                        .iter()
                        .find(|(n, _)| *n == "line")
                        .expect("an arrow")
                        .1;
                    assert!(
                        (num(line, "x1") - a).abs() < 0.05 && (num(line, "x2") - b).abs() < 0.05,
                        "#{i} arrow\n{source}"
                    );
                    assert!(
                        (num(line, "y1") - row).abs() < 0.05,
                        "#{i} arrow off its row\n{source}"
                    );
                    if cert.issuer != cert.audience {
                        let (lo, hi) = (a.min(b), a.max(b));
                        assert!(
                            lo < bbox.0 && bbox.2 < hi,
                            "#{i} label outside its arrow {bbox:?} vs {lo}..{hi}\n{source}"
                        );
                    }
                }

                if let Some(Event::Revoke(r)) = d.event(ix) {
                    let cross = parts
                        .iter()
                        .find(|(n, a)| *n == "path" && a.get("class") == Some(&"cross"));
                    assert_eq!(
                        cross.is_some(),
                        r.authority.is_authorized(),
                        "#{i}\n{source}"
                    );
                    if let Some((_, cross)) = cross {
                        let target = boxes[&r.target.index()];
                        let ps = points(cross["d"]);
                        let cx = ps.iter().map(|p| p.0).sum::<f64>() / ps.len() as f64;
                        let cy = ps.iter().map(|p| p.1).fold(f64::MAX, f64::min);
                        assert!(
                            target.0 <= cx && cx <= target.2,
                            "#{i} cross not under its target\n{source}"
                        );
                        assert!(
                            cy >= target.3,
                            "#{i} cross above its target's box\n{source}"
                        );
                    }
                }
            }

            // Exact positions: where each label goes, by the layout's rules
            // (coordinates are written to 0.1, so allow a little rounding)
            let near = |a: f64, b: f64| (a - b).abs() < 0.15;
            let centre = |b: Bbox| f64::midpoint(b.0, b.2);
            let half = |b: Bbox| (b.2 - b.0) / 2.0;
            for ix in d.event_ixs() {
                let i = ix.index();
                let bbox = boxes[&i];
                match d.event(ix) {
                    Some(Event::Delegate(cert)) => {
                        let (from, to) = (column(cert.issuer), column(cert.audience));
                        let h = half(bbox);
                        let (low, high) = (from.min(to) + h + 4.0, from.max(to) - h - 4.0);
                        let dir = if to < from { -1.0 } else { 1.0 };
                        let expected = match cert.at {
                            Some(p) if low <= high => {
                                (from + (to - from) * p.fraction()).clamp(low, high)
                            }
                            _ => from + dir * (h + 16.0).min((to - from).abs() / 2.0),
                        };
                        assert!(
                            near(centre(bbox), expected),
                            "#{i} label at {} not {expected}\n{source}",
                            centre(bbox)
                        );
                    }
                    Some(Event::Invoke(invoke)) => {
                        let (by, on) = (column(invoke.by), column(invoke.on));
                        let h = half(bbox);
                        let (low, high) = (by.min(on) + h + 10.0, by.max(on) - h - 10.0);
                        let dir = if on < by { -1.0 } else { 1.0 };
                        let powered = invoke
                            .authority
                            .route()
                            .and_then(|r| boxes.get(&r.held().index()))
                            .map_or(by + dir * (h + 16.0), |b| centre(*b));
                        let preferred =
                            invoke.at.map_or(powered, |p| by + (on - by) * p.fraction());
                        let expected = if low <= high {
                            preferred.clamp(low, high)
                        } else {
                            f64::midpoint(by, on)
                        };
                        assert!(
                            near(centre(bbox), expected),
                            "#{i} box at {} not {expected}\n{source}",
                            centre(bbox)
                        );
                    }
                    Some(Event::Revoke(r)) => {
                        let (x, tx) = (column(r.by), centre(boxes[&r.target.index()]));
                        let expected = r.at.map_or(tx, |p| {
                            let along = x + (tx - x) * p.fraction();
                            let (low, high) = (x.min(tx) + 50.0, x.max(tx) - 50.0);
                            if low <= high {
                                along.clamp(low, high)
                            } else {
                                along
                            }
                        });
                        assert!(
                            near(centre(bbox), expected),
                            "#{i} diamond at {} not {expected}\n{source}",
                            centre(bbox)
                        );
                    }
                    None => unreachable!("in range"),
                }
            }

            // Region boxes: over exactly their rows, inset by lane, centred
            // across the picture; regions sharing rows get different lanes
            let mut regions: Vec<(&str, Bbox)> = Vec::new();
            let mut region: Option<&str> = None;
            for (name, a) in &all {
                if *name == "g" {
                    region = (a.get("class") == Some(&"region")).then(|| a["data-region"]);
                } else if let (Some(id), "rect", Some(&"region-box")) =
                    (region, *name, a.get("class"))
                {
                    let (x, y) = (num(a, "x"), num(a, "y"));
                    regions.push((id, (x, y, x + num(a, "width"), y + num(a, "height"))));
                }
            }
            assert_eq!(regions.len(), d.regions().len(), "{source}");
            let mut insets = Vec::new();
            for ((id, b), r) in regions.iter().zip(d.regions()) {
                assert_eq!(*id, r.id.as_str(), "{source}");
                let inset = b.0 - 12.0;
                assert!(
                    inset > -0.15 && near(inset / 5.0, (inset / 5.0).round()),
                    "region {id} inset {inset}\n{source}"
                );
                assert!(
                    near(b.2, view[2] - 12.0 - inset),
                    "region {id} right edge\n{source}"
                );
                let first = top + height * r.first.index() as f64;
                let last = top + height * (r.last.index() + 1) as f64;
                assert!(
                    near(b.1, first + 3.0 + inset),
                    "region {id} top {} not {}\n{source}",
                    b.1,
                    first + 3.0 + inset
                );
                assert!(
                    near(b.3, last - 3.0 - inset),
                    "region {id} bottom\n{source}"
                );
                insets.push((r.first.index(), r.last.index(), inset));
            }
            for (k, a) in insets.iter().enumerate() {
                for b in &insets[k + 1..] {
                    if a.0 <= b.1 && b.0 <= a.1 {
                        assert!(
                            !near(a.2, b.2),
                            "regions sharing rows share a lane\n{source}"
                        );
                    }
                }
            }

            // Region grips sit on their box's top and bottom edges, as wide as it
            for (_, a) in all
                .iter()
                .filter(|(n, a)| *n == "rect" && a.get("class") == Some(&"hit region-edge"))
            {
                let b = regions
                    .iter()
                    .find(|(id, _)| *id == a["data-region"])
                    .expect("its region")
                    .1;
                let edge = if a["data-edge"] == "top" { b.1 } else { b.3 };
                assert!(
                    near(num(a, "x"), b.0) && near(num(a, "width"), b.2 - b.0),
                    "grip width\n{source}"
                );
                assert!(
                    near(num(a, "y"), edge - 5.0),
                    "{} grip off its edge\n{source}",
                    a["data-edge"]
                );
            }
            // Regions without a colour of their own cycle through the palette
            let fills: Vec<&str> = all
                .iter()
                .scan(None, |region: &mut Option<&str>, (name, a)| {
                    if *name == "g" {
                        *region = (a.get("class") == Some(&"region")).then(|| a["data-region"]);
                    }
                    Some((*region, *name, a))
                })
                .filter(|(_, name, a)| *name == "rect" && a.get("class") == Some(&"region-box"))
                .zip(d.regions())
                .filter(|(_, r)| r.colour.is_none())
                .map(|((_, _, a), _)| a["fill"])
                .take(5)
                .collect();
            let distinct: std::collections::BTreeSet<&str> = fills.iter().copied().collect();
            assert_eq!(
                distinct.len(),
                fills.len(),
                "uncoloured regions share a colour: {fills:?}\n{source}"
            );

            // Revocation lines run from the revoker's lifeline to just under
            // the target's box
            for ix in d.event_ixs() {
                let Some(Event::Revoke(r)) = d.event(ix) else {
                    continue;
                };
                let path = events[&ix.index()]
                    .iter()
                    .find(|(n, a)| *n == "path" && a.get("class") == Some(&"revocation"))
                    .expect("a revocation line")
                    .1;
                let ps = points(path["d"]);
                let row = top + height * (ix.index() as f64 + 0.5);
                let target = boxes[&r.target.index()];
                assert!(
                    near(ps[0].0, column(r.by)) && near(ps[0].1, row),
                    "#{} line starts at {:?}\n{source}",
                    ix.index(),
                    ps[0]
                );
                let end = *ps.last().expect("points");
                assert!(
                    near(end.0, centre(target)) && near(end.1, target.3 + 2.0),
                    "#{} line ends at {end:?}\n{source}",
                    ix.index()
                );
            }

            // Handles: on their membership's arrow, n beads out from the box's
            // audience-side edge; or, for authority that arrived later, on the
            // member's lifeline at a row boundary
            for (_, a) in all
                .iter()
                .filter(|(n, a)| *n == "circle" && a.get("class") == Some(&"handle"))
            {
                let (cx, cy) = (num(a, "cx"), num(a, "cy"));
                let e: usize = a["data-event"].parse().expect("data-event");
                let Some(Event::Delegate(cert)) = d.event_ix(e).and_then(|ix| d.event(ix)) else {
                    panic!("handles are on certificates\n{source}");
                };
                let b = boxes[&e];
                let dir = if column(cert.audience) < column(cert.issuer) {
                    -1.0
                } else {
                    1.0
                };
                let bead = |x: f64| {
                    let n = (x / 12.0).round();
                    (0.0..8.0).contains(&n) && near(x, n * 12.0)
                };
                let on_arrow =
                    near(cy, f64::midpoint(b.1, b.3)) && bead(dir * (cx - centre(b)) - half(b));
                let lifeline = bead(cx - column(cert.audience))
                    && near((cy - top) / height, ((cy - top) / height).round());
                assert!(
                    on_arrow || lifeline,
                    "handle on #{e} at {cx},{cy}\n{source}"
                );
            }

            // A certificate's tooltip mentions attenuation exactly when it
            // grants less than it claims
            for ix in d.event_ixs() {
                let Some(Event::Delegate(cert)) = d.event(ix) else {
                    continue;
                };
                let attenuated = d.granted(ix).is_some_and(|g| g < cert.level);
                assert_eq!(
                    event_group(&svg, ix).contains("(attenuated to"),
                    attenuated,
                    "#{}\n{source}",
                    ix.index()
                );
            }

            // Powerlines run from where authority comes from to the box it
            // feeds, or exactly to the top of a handle; straight drops into
            // invocations are vertical
            let handles: Vec<(f64, f64)> = all
                .iter()
                .filter(|(n, a)| *n == "circle" && a.get("class") == Some(&"handle"))
                .map(|(_, a)| (num(a, "cx"), num(a, "cy")))
                .collect();
            let at_handle = |p: (f64, f64)| {
                handles
                    .iter()
                    .any(|h| near(h.0, p.0) && near(h.1 - 5.0, p.1))
            };
            let roots: BTreeMap<usize, (f64, f64)> = all
                .iter()
                .filter(|(n, a)| *n == "circle" && a.get("class") == Some(&"root"))
                .map(|(_, a)| {
                    let e: usize = a["data-event"].parse().expect("data-event");
                    let (cx, cy) = (num(a, "cx"), num(a, "cy"));
                    let Some(Event::Delegate(cert)) = d.event_ix(e).and_then(|ix| d.event(ix))
                    else {
                        panic!("roots are on certificates\n{source}");
                    };
                    assert!(
                        near(cx, column(cert.issuer)),
                        "root #{e} off its issuer's lifeline\n{source}"
                    );
                    assert!(
                        near(cy, top + height * e as f64 + 2.0),
                        "root #{e} at {cy}\n{source}"
                    );
                    (e, (cx, cy))
                })
                .collect();
            for (_, a) in all.iter().filter(|(n, a)| {
                *n == "path" && a.get("class").is_some_and(|c| c.starts_with("powerline"))
            }) {
                let ps = points(a["d"]);
                let (start, end) = (ps[0], *ps.last().expect("points"));
                let to: usize = a["data-to"].parse().expect("data-to");
                // Authority arrives from above: into the box's upper half
                let b = boxes[&to];
                let upper = (
                    b.0 - 1.0,
                    b.1 - 1.0,
                    b.2 + 1.0,
                    f64::midpoint(b.1, b.3) + 0.15,
                );
                assert!(
                    inside(end, upper) || at_handle(end),
                    "powerline into #{to} ends at {end:?}, not in the top of {b:?}\n{source}"
                );
                if let Some(from) = a.get("data-from") {
                    let from: usize = from.parse().expect("data-from");
                    assert!(
                        inside(start, grow(boxes[&from], 1.0)) || at_handle(start),
                        "powerline from #{from} starts at {start:?}\n{source}"
                    );
                }
                // Roots after the first row leave from the side of a small
                // circle on the issuer's lifeline, facing the label
                if !a.contains_key("data-from")
                    && let Some((cx, cy)) = roots.get(&to)
                {
                    // (a self-delegation's label is centred on the lifeline)
                    let side = if centre(b) < *cx - 0.5 { -1.0 } else { 1.0 };
                    assert!(
                        near(start.0, cx + side * 5.0) && near(start.1, *cy),
                        "root #{to} leaves its circle at {start:?}\n{source}"
                    );
                }
                let invoke = matches!(
                    d.event_ix(to).and_then(|ix| d.event(ix)),
                    Some(Event::Invoke(_))
                );
                if invoke && ps.len() == 2 && a["d"].contains('L') {
                    assert!(
                        // within the renderer's half-pixel "straight down", plus rounding
                        (start.0 - end.0).abs() < 0.6,
                        "a straight drop into #{to} isn't vertical\n{source}"
                    );
                }
            }
        });
    }

    /// A certflow node: a principal's circle, or a certificate's box or a
    /// revocation's diamond (both measured as boxes, as the layout does).
    #[derive(Clone, Copy, Debug)]
    enum Node {
        Circle {
            c: (f64, f64),
            r: f64,
        },
        Box {
            c: (f64, f64),
            half: f64,
            half_h: f64,
        },
    }
    /// Whether `p` lies on the way from `from` towards `to`: along the line
    /// between them, on `to`'s side of `from`.
    fn heading(from: (f64, f64), p: (f64, f64), to: (f64, f64)) -> bool {
        let (ax, ay) = (p.0 - from.0, p.1 - from.1);
        let (bx, by) = (to.0 - from.0, to.1 - from.1);
        let cross = (ax * by - ay * bx).abs() / bx.hypot(by).max(1e-9);
        cross < 0.3 && ax * bx + ay * by > 0.0
    }

    impl Node {
        const fn centre(self) -> (f64, f64) {
            match self {
                Self::Circle { c, .. } | Self::Box { c, .. } => c,
            }
        }
        const fn radius(self) -> f64 {
            match self {
                Self::Circle { r, .. } => r,
                Self::Box { half, .. } => half,
            }
        }
        /// Whether `p` lies on the boundary.
        fn on_edge(self, p: (f64, f64)) -> bool {
            match self {
                Self::Circle { c, r } => ((p.0 - c.0).hypot(p.1 - c.1) - r).abs() < 0.2,
                Self::Box { c, half, half_h } => {
                    let k = ((p.0 - c.0).abs() / half).max((p.1 - c.1).abs() / half_h);
                    (k - 1.0).abs() < 0.02
                }
            }
        }
    }

    /// Certflow geometry: nodes framed by the margins, never overlapping;
    /// wires and powerlines leaving and entering exactly on node
    /// boundaries; regions wrapping their nodes; and, at each moment, only
    /// the principals involved so far.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn certflow_geometry_holds_together() {
        check!().with_type::<Vec<u8>>().for_each(|bytes| {
            let Some(scenario) = common::arbitrary::<Scenario>(bytes) else {
                return;
            };
            let (source, d) = scenario.diagram();
            let svg = d.to_certflow_svg();
            let all = tags(&svg);
            let near = |a: f64, b: f64| (a - b).abs() < 0.15;
            let view: Vec<f64> = all[0].1["viewBox"]
                .split(' ')
                .filter_map(|n| n.parse().ok())
                .collect();

            let mut principals: BTreeMap<&str, Node> = BTreeMap::new();
            let mut events: BTreeMap<usize, Node> = BTreeMap::new();
            let mut group: Option<&BTreeMap<&str, &str>> = None;
            for (name, a) in &all {
                if *name == "g" {
                    group = Some(a);
                    continue;
                }
                let Some(g) = group else { continue };
                let event = g.get("data-event").and_then(|e| e.parse::<usize>().ok());
                match (*name, a.get("class"), event) {
                    ("circle", Some(&"node"), _) => {
                        let key = g["data-principal"];
                        principals.insert(
                            key,
                            Node::Circle {
                                c: (num(a, "cx"), num(a, "cy")),
                                r: num(a, "r"),
                            },
                        );
                    }
                    ("rect", Some(&"pill"), Some(e)) => {
                        let (x, y, w, h) =
                            (num(a, "x"), num(a, "y"), num(a, "width"), num(a, "height"));
                        events.insert(
                            e,
                            Node::Box {
                                c: (x + w / 2.0, y + h / 2.0),
                                half: w / 2.0,
                                half_h: h / 2.0,
                            },
                        );
                    }
                    ("path", Some(&"diamond"), Some(e)) => {
                        let ps = points(a["d"]);
                        let c = (ps[0].0, ps[1].1);
                        events.insert(
                            e,
                            Node::Box {
                                c,
                                half: 22.0,
                                half_h: 16.0,
                            },
                        );
                    }
                    _ => {}
                }
            }
            let principal = |p: rain_diagram_core::diagram::PrincipalIx| {
                principals[d.principal(p).expect("in range").key()]
            };
            assert_eq!(principals.len(), d.principals().len(), "{source}");

            // Framed by the margins: the leftmost and topmost edges sit at
            // the margin (plus the region frame, and the title's room)
            let nodes: Vec<Node> = principals
                .values()
                .chain(events.values())
                .copied()
                .collect();
            if nodes.is_empty() {
                return;
            }
            let left = nodes
                .iter()
                .map(|n| n.centre().0 - n.radius())
                .fold(f64::MAX, f64::min);
            let top_edge = nodes
                .iter()
                .map(|n| n.centre().1 - n.radius())
                .fold(f64::MAX, f64::min);
            let right = nodes
                .iter()
                .map(|n| n.centre().0 + n.radius())
                .fold(f64::MIN, f64::max);
            let frame = left - 24.0;
            if d.regions().is_empty() {
                assert!(near(frame, 0.0), "frame {frame} without regions\n{source}");
            } else {
                assert!(
                    frame > 29.8 && near((frame - 30.0) / 24.0, ((frame - 30.0) / 24.0).round()),
                    "frame {frame}\n{source}"
                );
            }
            let title = if d.title().is_some() { 32.0 } else { 0.0 };
            assert!(
                near(top_edge, 24.0 + title + frame),
                "top edge {top_edge}\n{source}"
            );
            assert!(
                (view[2] - (right + 24.0 + frame)).abs() <= 0.6,
                "width {} for right edge {right}\n{source}",
                view[2]
            );
            let bottom = nodes
                .iter()
                .map(|n| n.centre().1 + n.radius())
                .fold(f64::MIN, f64::max);
            assert!(
                (view[3] - (bottom + 24.0 + frame)).abs() <= 0.6,
                "height {} for bottom edge {bottom}\n{source}",
                view[3]
            );

            // Nothing overlaps (the layout keeps 18 px between nodes)
            for (i, a) in nodes.iter().enumerate() {
                for b in &nodes[i + 1..] {
                    let (ca, cb) = (a.centre(), b.centre());
                    let gap = (ca.0 - cb.0).hypot(ca.1 - cb.1) - a.radius() - b.radius();
                    assert!(
                        gap >= 17.5,
                        "nodes {ca:?} and {cb:?} only {gap} apart\n{source}"
                    );
                }
            }

            for ix in d.event_ixs() {
                let i = ix.index();
                let parts: Vec<&(&str, BTreeMap<&str, &str>)> = {
                    let open = format!(r#"data-event="{i}""#);
                    let mut inside = false;
                    let mut out = Vec::new();
                    for t in &all {
                        if t.0 == "g" {
                            inside = t.1.get("class").is_some_and(|c| c.starts_with("event "))
                                && t.1
                                    .get("data-event")
                                    .is_some_and(|e| format!(r#"data-event="{e}""#) == open);
                            continue;
                        }
                        if inside {
                            out.push(t);
                        }
                    }
                    out
                };
                match d.event(ix) {
                    // Wires: issuer's edge → box, box → audience's edge,
                    // with an arrowhead whose tip is the wire's end
                    Some(Event::Delegate(cert)) => {
                        let node = events[&i];
                        let wire = parts
                            .iter()
                            .find(|t| t.1.get("class") == Some(&"wire"))
                            .expect("a wire");
                        let ps = points(wire.1["d"]);
                        let (issuer, audience) = (principal(cert.issuer), principal(cert.audience));
                        if cert.issuer != cert.audience {
                            assert!(
                                issuer.on_edge(ps[0]) && node.on_edge(ps[1]),
                                "#{i} wire in\n{source}"
                            );
                            assert!(
                                node.on_edge(ps[2]) && audience.on_edge(ps[3]),
                                "#{i} wire out\n{source}"
                            );
                            // Each end faces the other node
                            let (ic, nc, ac) = (issuer.centre(), node.centre(), audience.centre());
                            assert!(
                                heading(ic, ps[0], nc) && heading(nc, ps[1], ic),
                                "#{i} wire in faces away\n{source}"
                            );
                            assert!(
                                heading(nc, ps[2], ac) && heading(ac, ps[3], nc),
                                "#{i} wire out faces away\n{source}"
                            );
                        }
                        let chevron = parts
                            .iter()
                            .find(|t| t.1.get("class") == Some(&"chevron"))
                            .expect("an arrowhead");
                        let cs = points(chevron.1["d"]);
                        assert!(
                            near(cs[1].0, ps[3].0) && near(cs[1].1, ps[3].1),
                            "#{i} arrowhead off the tip\n{source}"
                        );
                        // The arms trail back towards the box, not past the tip
                        if cert.issuer != cert.audience {
                            let ac = principal(cert.audience).centre();
                            let arms = (
                                f64::midpoint(cs[0].0, cs[2].0),
                                f64::midpoint(cs[0].1, cs[2].1),
                            );
                            let (to_arms, to_tip) = (
                                (arms.0 - ac.0).hypot(arms.1 - ac.1),
                                (cs[1].0 - ac.0).hypot(cs[1].1 - ac.1),
                            );
                            assert!(
                                to_arms > to_tip,
                                "#{i} arrowhead points backwards\n{source}"
                            );
                        }
                        for arm in [cs[0], cs[2]] {
                            assert!(
                                ((arm.0 - cs[1].0).hypot(arm.1 - cs[1].1) - 106_f64.sqrt()).abs()
                                    < 0.2,
                                "#{i} arrowhead arm\n{source}"
                            );
                        }
                    }
                    // Revocations: revoker's edge → diamond, diamond → the
                    // target's box, crossed out there if it took effect
                    Some(Event::Revoke(r)) => {
                        let node = events[&i];
                        let lines: Vec<Vec<(f64, f64)>> = parts
                            .iter()
                            .filter(|t| t.1.get("class") == Some(&"revocation"))
                            .map(|t| points(t.1["d"]))
                            .collect();
                        assert!(
                            principal(r.by).on_edge(lines[0][0]) && node.on_edge(lines[0][1]),
                            "#{i} revocation in\n{source}"
                        );
                        if let Some(target) = events.get(&r.target.index()) {
                            let end = lines[1][1];
                            assert!(
                                node.on_edge(lines[1][0]) && target.on_edge(end),
                                "#{i} revocation out\n{source}"
                            );
                            if let Some(cross) =
                                parts.iter().find(|t| t.1.get("class") == Some(&"cross"))
                            {
                                let cs = points(cross.1["d"]);
                                let cx = cs.iter().map(|p| p.0).sum::<f64>() / cs.len() as f64;
                                let cy = cs.iter().map(|p| p.1).sum::<f64>() / cs.len() as f64;
                                assert!(
                                    near(cx, end.0) && near(cy, end.1),
                                    "#{i} cross off the target\n{source}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Powerlines: from a node's edge to the edge of the box they feed,
            // bowed out from the chord by max(0.3 × length, 40)
            for (_, a) in all.iter().filter(|(n, a)| {
                *n == "path" && a.get("class").is_some_and(|c| c.starts_with("powerline"))
            }) {
                let ps = points(a["d"]);
                let (start, control, end) = (ps[0], ps[1], ps[2]);
                let to: usize = a["data-to"].parse().expect("data-to");
                assert!(
                    events[&to].on_edge(end),
                    "powerline into #{to} ends off its edge at {end:?}\n{source}"
                );
                assert!(
                    nodes.iter().any(|n| n.on_edge(start)),
                    "powerline into #{to} starts at {start:?}, on no edge\n{source}"
                );
                let length = (end.0 - start.0).hypot(end.1 - start.1);
                let mid = (f64::midpoint(start.0, end.0), f64::midpoint(start.1, end.1));
                let bow = (control.0 - mid.0).hypot(control.1 - mid.1);
                assert!(
                    (bow - (0.3 * length).max(40.0)).abs() < 0.3,
                    "powerline into #{to} bows {bow} for length {length}\n{source}"
                );
                // Square to the chord
                let dot = (control.0 - mid.0) * (end.0 - start.0)
                    + (control.1 - mid.1) * (end.1 - start.1);
                assert!(
                    dot.abs() / (bow * length).max(1e-9) < 0.01,
                    "powerline into #{to} bows askew\n{source}"
                );
                // A root's bows away from the audience, clear of the wire
                if !a.contains_key("data-from")
                    && let Some(Event::Delegate(cert)) = d.event_ix(to).and_then(|ix| d.event(ix))
                {
                    // (meaningless when the audience is the subject: it's on the chord)
                    if cert.audience == cert.subject {
                        continue;
                    }
                    let ac = principal(cert.audience).centre();
                    let away =
                        (control.0 - mid.0) * (ac.0 - mid.0) + (control.1 - mid.1) * (ac.1 - mid.1);
                    assert!(
                        // as a cosine, with room for rounding when the audience is
                        // nearly in line with the chord
                        away / (bow * (ac.0 - mid.0).hypot(ac.1 - mid.1)).max(1e-9) <= 0.02,
                        "root powerline into #{to} bows towards its audience\n{source}"
                    );
                }
            }

            // Regions wrap their drawn nodes, with room for the caption tab
            let mut region: Option<&str> = None;
            for (name, a) in &all {
                if *name == "g" {
                    region = (a.get("class") == Some(&"region")).then(|| a["data-region"]);
                    continue;
                }
                let (Some(id), "rect", Some(&"region-box")) = (region, *name, a.get("class"))
                else {
                    continue;
                };
                let r = d
                    .regions()
                    .iter()
                    .find(|r| r.id.as_str() == id)
                    .expect("its region");
                let (x, y) = (num(a, "x"), num(a, "y"));
                let b = (x, y, x + num(a, "width"), y + num(a, "height"));
                // Exactly: the drawn nodes' extent, padded by 12 + 24k (k
                // more for each level nested inside), plus the tab on top
                let drawn: Vec<Node> = (r.first.index()..=r.last.index())
                    .filter_map(|e| events.get(&e).copied())
                    .collect();
                if !drawn.is_empty() {
                    let extent = drawn.iter().fold(
                        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
                        |(l, t, rr, bb), n| {
                            let (c, rad) = (n.centre(), n.radius());
                            (
                                l.min(c.0 - rad),
                                t.min(c.1 - rad),
                                rr.max(c.0 + rad),
                                bb.max(c.1 + rad),
                            )
                        },
                    );
                    let pad = extent.0 - b.0;
                    assert!(
                        pad > 11.8 && near((pad - 12.0) / 24.0, ((pad - 12.0) / 24.0).round()),
                        "region {id} pad {pad}\n{source}"
                    );
                    assert!(
                        // sums of several 0.1-rounded numbers, so a looser tolerance
                        (b.2 - extent.2 - pad).abs() < 0.3
                            && (b.3 - extent.3 - pad).abs() < 0.3
                            && (extent.1 - b.1 - pad - 18.0).abs() < 0.3,
                        "region {id} padded unevenly\n{source}"
                    );
                }
                for e in r.first.index()..=r.last.index() {
                    if let Some(n) = events.get(&e) {
                        let (c, rad) = (n.centre(), n.radius());
                        assert!(
                            b.0 <= c.0 - rad - 11.8
                                && c.0 + rad + 11.8 <= b.2
                                && b.1 <= c.1 - rad - 29.8
                                && c.1 + rad + 11.8 <= b.3,
                            "region {id} doesn't wrap #{e}\n{source}"
                        );
                    }
                }
            }

            // A region whose rows contain another's draws its box around it
            let mut boxes: Vec<(&rain_diagram_core::diagram::Region, Bbox)> = Vec::new();
            let mut current: Option<&str> = None;
            for (name, a) in &all {
                if *name == "g" {
                    current = (a.get("class") == Some(&"region")).then(|| a["data-region"]);
                } else if let (Some(id), "rect", Some(&"region-box")) =
                    (current, *name, a.get("class"))
                {
                    let r = d
                        .regions()
                        .iter()
                        .find(|r| r.id.as_str() == id)
                        .expect("its region");
                    let (x, y) = (num(a, "x"), num(a, "y"));
                    boxes.push((r, (x, y, x + num(a, "width"), y + num(a, "height"))));
                }
            }
            for (outer, ob) in &boxes {
                for (inner, ib) in &boxes {
                    let nested = outer.id != inner.id
                        && outer.first <= inner.first
                        && inner.last <= outer.last
                        && (outer.first, outer.last) != (inner.first, inner.last);
                    if nested {
                        assert!(
                            ob.0 < ib.0 && ob.1 < ib.1 && ib.2 < ob.2 && ib.3 < ob.3,
                            "region {} doesn't surround {}\n{source}",
                            outer.id.as_str(),
                            inner.id.as_str()
                        );
                    }
                }
            }

            // At each moment, only the principals involved so far appear
            for happened in 0..d.events().len() {
                let svg = d.to_certflow_svg_at(happened);
                for p in d.principals() {
                    let ix = d.principal_by_key(p.key()).expect("its own key");
                    let involved = d.events().iter().take(happened).any(|e| match &e.node {
                        Event::Delegate(c) => [c.issuer, c.audience, c.subject].contains(&ix),
                        Event::Invoke(i) => i.by == ix || i.on == ix,
                        Event::Revoke(r) => r.by == ix,
                    });
                    let shown = svg.contains(&format!(r#"data-principal="{}""#, p.key()));
                    assert_eq!(
                        shown,
                        involved,
                        "{} at moment {happened}\n{source}",
                        p.name()
                    );
                }
            }
        });
    }
}
