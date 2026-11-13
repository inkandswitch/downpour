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

    steps: Vec<Step>,
}

impl Scenario {
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
