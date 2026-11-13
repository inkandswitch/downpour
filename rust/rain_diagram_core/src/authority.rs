//! Who could do what, when, and why.
//!
//! Every principal holds everything over itself. Authority over anything
//! else arrives through certificates: `(I)-[X(S)]->(A)` lets A act on S
//! with ability X, but only as far as I could when it signed. What actually
//! flows is the narrower of the two (_attenuation_), and nothing at all if
//! neither covers the other (see [`Ability`]).
//!
//! Groups make authority transitive. If Alice holds authority over Team, and
//! Team holds Edit over Doc, then Alice holds authority over Doc through
//! Team, at the narrowest of the abilities along the way. The certificate
//! carrying Doc's authority past Team (Team → Alice) is a _transitive
//! handle_.
//!
//! A certificate can also carry a `where` caveat. Caveats are opaque, so
//! they never stop authority flowing here; instead each event lists the
//! caveats along everything it relies on (see
//! [`Diagram::caveats`](crate::diagram::Diagram::caveats)), which only grow
//! as authority is passed on.
//!
//! The rules, applied in causal order:
//!
//! | Event        | Authorised when                                                       | Effect                                       |
//! |--------------|-----------------------------------------------------------------------|----------------------------------------------|
//! | Delegation   | the issuer holds something over the subject comparable to the claim   | grants the narrower of claimed and held       |
//! | Invocation   | the actor holds something over the subject covering the ability used  |                                              |
//! | Revocation   | the revoker issued something the target relies on, or holds Admin     | voids the target and everything relying on it |
//!
//! Revocation is retrocausal: it voids what relied on the target even if
//! that happened earlier, including invocations. Once effective, a
//! revocation stays effective; revoking the revoker later doesn't undo it.
//!
//! Each delegation and invocation relies on exactly one [`Route`], chosen
//! when it happens: the one granting the broadest ability (up to what was
//! claimed), then relying on the fewest caveats, then the fewest hops, then
//! the earliest certificates. Keyhive certificates name
//! their proof, so a later alternate route doesn't rescue a voided one.
//!
//! ```
//! use rain_diagram_core::{authority::Status, parse};
//!
//! let d = parse(
//!     "rain
//!      doc-alice: (Doc)-[edit(Doc)]->(Alice)
//!      (Alice)-[edit(Doc)]->(Bob)
//!      (Bob)=[[edit]]=>(Doc)
//!      (Doc)-<revoke>-x[doc-alice]",
//! )?;
//! let edit = d.event_ix(2).expect("four events");
//! let now = d.now().expect("not empty");
//!
//! // Bob's edit relied on Alice's certificate, so it's void now
//! assert!(matches!(d.status(edit, now), Some(Status::Void { .. })));
//! # Ok::<(), rain_diagram_core::Error>(())
//! ```

use crate::{
    ability::Ability,
    ast::{Caveat, CertId, Name, Span, Spanned},
    diagram::{Cert, Event, EventIx, Invoke, PrincipalIx, Revoke},
    level::Level,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use thiserror::Error;

/// Why an event was allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authority {
    /// Acting on yourself needs no certificate.
    Root,

    /// Authority arrived through certificates.
    Route(Route),

    /// Nothing gave the actor this authority when it happened.
    Unauthorized,
}

impl Authority {
    /// The route, if authority came through certificates.
    #[must_use]
    pub const fn route(&self) -> Option<&Route> {
        match self {
            Self::Route(route) => Some(route),
            Self::Root | Self::Unauthorized => None,
        }
    }

    /// Whether the event was allowed when it happened.
    #[must_use]
    pub const fn is_authorized(&self) -> bool {
        !matches!(self, Self::Unauthorized)
    }
}

/// The certificates carrying a subject's authority to a holder.
///
/// The [`source`](Self::source) grants authority over the subject itself.
/// Each certificate in [`through`](Self::through) is a transitive handle,
/// carrying it one group closer to the holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    source: EventIx,
    through: Vec<EventIx>,
}

impl Route {
    /// The certificate granting authority over the subject.
    #[must_use]
    pub const fn source(&self) -> EventIx {
        self.source
    }

    /// Memberships carrying the source's authority, from the source's
    /// audience towards the holder.
    #[must_use]
    pub fn through(&self) -> &[EventIx] {
        &self.through
    }

    /// The certificate addressed to the holder: the last handle, or the
    /// source itself for direct authority.
    #[must_use]
    pub fn held(&self) -> EventIx {
        self.through.last().copied().unwrap_or(self.source)
    }

    /// The source, then the handles.
    pub fn certs(&self) -> impl Iterator<Item = EventIx> + '_ {
        core::iter::once(self.source).chain(self.through.iter().copied())
    }

    /// From a path in holder-to-subject order.
    fn from_path(mut path: Vec<EventIx>) -> Option<Self> {
        let source = path.pop()?;
        path.reverse();
        Some(Self {
            source,
            through: path,
        })
    }
}

/// An event's standing at some moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// In force (for a revocation: took effect).
    Valid,

    /// Never authorised.
    Unauthorized,

    /// This certificate was revoked.
    Revoked {
        /// The revocation.
        revocation: EventIx,
    },

    /// Something this relied on was revoked.
    Void {
        /// The revoked certificate.
        revoked: EventIx,

        /// The revocation.
        revocation: EventIx,
    },
}

/// Authority a principal holds over a subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach {
    /// What can be acted on.
    pub subject: PrincipalIx,

    /// How far.
    pub ability: Ability,

    /// Through which certificates.
    pub route: Route,
}

/// What the analysis knows about each event. Indexed like the events.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ledger {
    facts: Vec<Facts>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Facts {
    /// For an authorised delegation, the ability that actually flows.
    granted: Option<Ability>,

    /// Certificates this event relies on, itself included for a delegation.
    deps: BTreeSet<EventIx>,

    /// For a certificate, the first revocation that took effect.
    revoked_by: Option<EventIx>,
}

/// The causal past: events with index below this.
#[derive(Debug, Clone, Copy)]
struct Horizon(usize);

/// Where a search starts: the holder, and the certificates a route must
/// begin with (holder first).
#[derive(Debug, Clone, Copy)]
struct Start<'a> {
    holder: PrincipalIx,
    via: &'a [EventIx],
}

impl Ledger {
    /// Admit the next delegation. `via` is the (shape-checked) required
    /// prefix of its route, holder first.
    pub(crate) fn delegate(
        &mut self,
        events: &[Spanned<Event>],
        cert: &Cert,
        via: &[EventIx],
        span: Span,
        names: &dyn Fn(PrincipalIx) -> Name,
    ) -> (Authority, Option<Diagnostic>) {
        let ix = EventIx::new(self.facts.len());
        let mut facts = Facts {
            deps: BTreeSet::from([ix]),
            ..Facts::default()
        };

        let (authority, diagnostic) = if cert.issuer == cert.subject && via.is_empty() {
            facts.granted = Some(cert.ability.clone());
            (Authority::Root, None)
        } else {
            let horizon = Horizon(ix.index());
            let from = Start {
                holder: cert.issuer,
                via,
            };
            if let Some((granted, route)) =
                self.best(events, from, cert.subject, &cert.ability, horizon)
            {
                facts.deps.extend(self.deps_of(&route));
                let attenuated = (granted != cert.ability).then(|| {
                    Diagnostic::new(
                        span,
                        DiagnosticKind::Attenuated {
                            issuer: names(cert.issuer),
                            subject: names(cert.subject),
                            claimed: cert.ability.clone(),
                            granted: granted.clone(),
                        },
                    )
                });
                facts.granted = Some(granted);
                (Authority::Route(route), attenuated)
            } else {
                let held = self.held(events, from, cert.subject, horizon);
                let kind = if held.is_empty() {
                    DiagnosticKind::Unauthorized {
                        who: names(cert.issuer),
                        subject: names(cert.subject),
                    }
                } else {
                    DiagnosticKind::Unrelated {
                        issuer: names(cert.issuer),
                        subject: names(cert.subject),
                        claimed: cert.ability.clone(),
                        held,
                    }
                };
                (Authority::Unauthorized, Some(Diagnostic::new(span, kind)))
            }
        };

        self.facts.push(facts);
        (authority, diagnostic)
    }

    /// Admit the next invocation.
    pub(crate) fn invoke(
        &mut self,
        events: &[Spanned<Event>],
        invoke: &Invoke,
        span: Span,
        names: &dyn Fn(PrincipalIx) -> Name,
    ) -> (Authority, Option<Diagnostic>) {
        let horizon = Horizon(self.facts.len());
        let mut facts = Facts::default();
        let from = Start {
            holder: invoke.by,
            via: &[],
        };

        let (authority, diagnostic) = if invoke.by == invoke.on {
            (Authority::Root, None)
        } else {
            match self.best(events, from, invoke.on, &invoke.ability, horizon) {
                Some((held, route)) if held == invoke.ability => {
                    facts.deps = self.deps_of(&route);
                    (Authority::Route(route), None)
                }
                Some(_) | None => (
                    Authority::Unauthorized,
                    Some(Diagnostic::new(
                        span,
                        DiagnosticKind::Insufficient {
                            who: names(invoke.by),
                            subject: names(invoke.on),
                            needed: invoke.ability.clone(),
                            held: self.held(events, from, invoke.on, horizon),
                        },
                    )),
                ),
            }
        };

        self.facts.push(facts);
        (authority, diagnostic)
    }

    /// Admit the next revocation, recording its effect on the target.
    pub(crate) fn revoke(
        &mut self,
        events: &[Spanned<Event>],
        revoke: &Revoke,
        span: Span,
        names: &dyn Fn(PrincipalIx) -> Name,
    ) -> (Authority, Option<Diagnostic>) {
        let ix = EventIx::new(self.facts.len());
        let horizon = Horizon(ix.index());
        let target = cert_at(events, revoke.target);

        let upstream = self
            .facts(revoke.target)
            .deps
            .iter()
            .rev()
            .filter_map(|&k| Some((k, cert_at(events, k)?)))
            .filter(|(_, k)| k.issuer == revoke.by)
            .find_map(|(_, k)| match &k.authority {
                Authority::Root => Some(Authority::Root),
                Authority::Route(route) => route
                    .certs()
                    .all(|c| self.usable(c, horizon))
                    .then(|| Authority::Route(route.clone())),
                Authority::Unauthorized => None,
            });

        let admin = || {
            let subject = target?.subject;
            if subject == revoke.by {
                return Some(Authority::Root);
            }
            let from = Start {
                holder: revoke.by,
                via: &[],
            };
            self.search(events, from, subject, &Level::Admin.into(), horizon)
                .and_then(Route::from_path)
                .map(Authority::Route)
        };

        let mut facts = Facts::default();
        let (authority, diagnostic) = match upstream.or_else(admin) {
            Some(authority) => {
                if let Some(route) = authority.route() {
                    facts.deps = self.deps_of(route);
                }
                // Targets are earlier events, so they always have facts
                if let Some(target) = self.facts.get_mut(revoke.target.index()) {
                    target.revoked_by.get_or_insert(ix);
                }
                (authority, None)
            }
            None => (
                Authority::Unauthorized,
                Some(Diagnostic::new(
                    span,
                    DiagnosticKind::NotUpstream {
                        who: names(revoke.by),
                        target: target.map(|t| t.id.clone()),
                    },
                )),
            ),
        };

        self.facts.push(facts);
        (authority, diagnostic)
    }

    /// The ability a delegation actually grants, if it was authorised.
    pub(crate) fn granted(&self, ix: EventIx) -> Option<&Ability> {
        self.facts.get(ix.index())?.granted.as_ref()
    }

    /// Whether `cert` relies on (or is) `on`.
    pub(crate) fn relies_on(&self, ix: EventIx, on: EventIx) -> bool {
        self.facts
            .get(ix.index())
            .is_some_and(|f| f.deps.contains(&on))
    }

    /// The certificates with caveats that `ix` relies on, earliest first.
    pub(crate) fn caveats<'a>(
        &'a self,
        events: &'a [Spanned<Event>],
        ix: EventIx,
    ) -> impl Iterator<Item = (EventIx, &'a Caveat)> + 'a {
        caveats_in(events, &self.facts(ix).deps)
    }

    /// The certificates with caveats that `route` relies on, earliest first.
    pub(crate) fn route_caveats<'a>(
        &self,
        events: &'a [Spanned<Event>],
        route: &Route,
    ) -> Vec<(EventIx, &'a Caveat)> {
        self.deps_of(route)
            .into_iter()
            .filter_map(|ix| Some((ix, cert_at(events, ix)?.caveat.as_ref()?)))
            .collect()
    }

    pub(crate) fn status(
        &self,
        events: &[Spanned<Event>],
        ix: EventIx,
        now: EventIx,
    ) -> Option<Status> {
        if ix > now {
            return None;
        }

        let event = &events.get(ix.index())?.node;
        let authorized = match event {
            Event::Delegate(c) => c.authority.is_authorized(),
            Event::Invoke(i) => i.authority.is_authorized(),
            Event::Revoke(r) => return Some(r.authority.is_authorized().into()),
        };
        if !authorized {
            return Some(Status::Unauthorized);
        }

        let first_revoked = self
            .facts(ix)
            .deps
            .iter()
            .filter_map(|&d| Some((self.facts(d).revoked_by?, d)))
            .filter(|&(r, _)| r <= now)
            .min();

        Some(match first_revoked {
            None => Status::Valid,
            Some((revocation, revoked)) if revoked == ix => Status::Revoked { revocation },
            Some((revocation, revoked)) => Status::Void {
                revoked,
                revocation,
            },
        })
    }

    /// For each subject, each broadest ability `holder` has over it as of
    /// `now`. Excludes the holder itself.
    pub(crate) fn reach(
        &self,
        events: &[Spanned<Event>],
        holder: PrincipalIx,
        principals: usize,
        now: EventIx,
    ) -> Vec<Reach> {
        let horizon = Horizon(now.index() + 1);
        let from = Start { holder, via: &[] };
        (0..principals)
            .map(PrincipalIx::new)
            .filter(|&subject| subject != holder)
            .flat_map(|subject| {
                self.routes(events, from, subject, None, horizon)
                    .into_iter()
                    .map(move |(ability, route)| Reach {
                        subject,
                        ability,
                        route,
                    })
            })
            .collect()
    }

    /// The broadest abilities `from` holds over `subject`.
    fn held(
        &self,
        events: &[Spanned<Event>],
        from: Start<'_>,
        subject: PrincipalIx,
        horizon: Horizon,
    ) -> Vec<Ability> {
        self.routes(events, from, subject, None, horizon)
            .into_iter()
            .map(|(ability, _)| ability)
            .collect()
    }

    /// The route granting the broadest ability within `claimed`, with that
    /// ability. Between abilities equally broad but different (`/crud/read`
    /// and `/crud/write` under `/crud`), the one with fewer caveats, then
    /// fewer hops, then earlier certificates.
    fn best(
        &self,
        events: &[Spanned<Event>],
        from: Start<'_>,
        subject: PrincipalIx,
        claimed: &Ability,
        horizon: Horizon,
    ) -> Option<(Ability, Route)> {
        let mut routes = self.routes(events, from, subject, Some(claimed), horizon);
        // Sorting is stable, so earlier certificates still win ties
        routes.sort_by_cached_key(|(ability, route)| {
            (
                breadth(ability),
                caveats_in(events, &self.deps_of(route)).count(),
                route.certs().count(),
            )
        });
        routes.into_iter().next()
    }

    /// The broadest abilities over `subject` that `from` can reach, each with
    /// its route: only those within `claimed`, if given.
    ///
    /// What flows down a route is the narrowest of its certificates, which
    /// is always one of the abilities granted along it (or `claimed`). So
    /// trying those candidates broadest first, and searching for a route
    /// whose every certificate covers the candidate, finds each broadest
    /// one exactly.
    fn routes(
        &self,
        events: &[Spanned<Event>],
        from: Start<'_>,
        subject: PrincipalIx,
        claimed: Option<&Ability>,
        horizon: Horizon,
    ) -> Vec<(Ability, Route)> {
        let mut candidates: Vec<&Ability> = (0..horizon.0)
            .map(EventIx::new)
            .filter(|&ix| self.usable(ix, horizon))
            .filter_map(|ix| self.granted(ix))
            .chain(claimed)
            .filter(|a| claimed.is_none_or(|c| c.covers(a)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        candidates.sort_by_key(|a| breadth(a));

        let mut found: Vec<(Ability, Route)> = Vec::new();
        for candidate in candidates {
            if found.iter().any(|(held, _)| held.covers(candidate)) {
                continue;
            }
            if let Some(route) = self
                .search(events, from, subject, candidate, horizon)
                .and_then(Route::from_path)
            {
                found.push((candidate.clone(), route));
            }
        }
        found
    }

    /// The path of usable certificates, each covering `at_least`, from
    /// `from` to `subject` that relies on the fewest caveats, then has the
    /// fewest hops, then takes the earliest certificates. Returned holder
    /// first.
    fn search(
        &self,
        events: &[Spanned<Event>],
        from: Start<'_>,
        subject: PrincipalIx,
        at_least: &Ability,
        horizon: Horizon,
    ) -> Option<Vec<EventIx>> {
        let usable = |ix: EventIx| {
            self.usable(ix, horizon) && self.granted(ix).is_some_and(|g| g.covers(at_least))
        };

        let mut start = from.holder;
        for &ix in from.via {
            if !usable(ix) {
                return None;
            }
            start = cert_at(events, ix)?.subject;
        }

        // Dijkstra over (caveats, hops), first come first served among
        // equals; with no caveats, this is a breadth-first search
        let mut best: BTreeMap<PrincipalIx, Cost> = BTreeMap::from([(start, (0, 0))]);
        let mut parent: BTreeMap<PrincipalIx, (PrincipalIx, EventIx)> = BTreeMap::new();
        let mut queue: BTreeSet<(Cost, usize, PrincipalIx)> = BTreeSet::from([((0, 0), 0, start)]);
        let mut done = BTreeSet::new();
        let mut arrivals = 0;

        while let Some((cost, _, node)) = queue.pop_first() {
            if !done.insert(node) {
                continue;
            }

            if node == subject {
                let mut path = Vec::new();
                let mut at = node;
                while let Some(&(prev, ix)) = parent.get(&at) {
                    path.push(ix);
                    at = prev;
                }
                path.extend(from.via.iter().rev());
                path.reverse();
                return (!path.is_empty()).then_some(path);
            }

            let edges = events.iter().enumerate().filter_map(|(i, e)| {
                let ix = EventIx::new(i);
                match &e.node {
                    Event::Delegate(c) if c.audience == node && c.subject != node && usable(ix) => {
                        Some((c.subject, ix))
                    }
                    Event::Delegate(_) | Event::Invoke(_) | Event::Revoke(_) => None,
                }
            });

            for (next, ix) in edges {
                let caveats = caveats_in(events, &self.facts(ix).deps).count();
                let reached = (cost.0 + caveats, cost.1 + 1);
                if best.get(&next).is_none_or(|&known| reached < known) {
                    best.insert(next, reached);
                    parent.insert(next, (node, ix));
                    arrivals += 1;
                    queue.insert((reached, arrivals, next));
                }
            }
        }

        None
    }

    /// An authorised certificate before the horizon that nothing before the
    /// horizon has voided.
    fn usable(&self, ix: EventIx, horizon: Horizon) -> bool {
        ix.index() < horizon.0
            && self.granted(ix).is_some()
            && self.facts(ix).deps.iter().all(|&d| {
                self.facts(d)
                    .revoked_by
                    .is_none_or(|r| r.index() >= horizon.0)
            })
    }

    fn deps_of(&self, route: &Route) -> BTreeSet<EventIx> {
        route
            .certs()
            .flat_map(|c| self.facts(c).deps.iter().copied())
            .collect()
    }

    fn facts(&self, ix: EventIx) -> &Facts {
        static EMPTY: Facts = Facts {
            granted: None,
            deps: BTreeSet::new(),
            revoked_by: None,
        };
        self.facts.get(ix.index()).unwrap_or(&EMPTY)
    }
}

/// A search cost: caveats relied on, then hops.
type Cost = (usize, usize);

/// A sort key putting broader abilities first: if `a` covers `b`, then
/// `breadth(a) < breadth(b)`.
fn breadth(ability: &Ability) -> (usize, core::cmp::Reverse<Level>) {
    match ability {
        Ability::Level(level) => (0, core::cmp::Reverse(*level)),
        Ability::Command(command) => (command.depth(), core::cmp::Reverse(Level::Admin)),
    }
}

/// The caveats on the certificates in `deps`, earliest first.
fn caveats_in<'a>(
    events: &'a [Spanned<Event>],
    deps: &'a BTreeSet<EventIx>,
) -> impl Iterator<Item = (EventIx, &'a Caveat)> + 'a {
    deps.iter()
        .filter_map(|&ix| Some((ix, cert_at(events, ix)?.caveat.as_ref()?)))
}

impl From<bool> for Status {
    fn from(authorized: bool) -> Self {
        if authorized {
            Self::Valid
        } else {
            Self::Unauthorized
        }
    }
}

fn cert_at(events: &[Spanned<Event>], ix: EventIx) -> Option<&Cert> {
    match &events.get(ix.index())?.node {
        Event::Delegate(cert) => Some(cert),
        Event::Invoke(_) | Event::Revoke(_) => None,
    }
}

/// Something that parses and resolves, but doesn't do what it looks like it
/// does. Diagrams may show these on purpose ("try to break it"), so they're
/// warnings, not errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{kind}")]
pub struct Diagnostic {
    span: Span,
    kind: DiagnosticKind,
}

impl Diagnostic {
    /// A diagnostic at `span`.
    #[must_use]
    pub const fn new(span: Span, kind: DiagnosticKind) -> Self {
        Self { span, kind }
    }

    /// The line it's about.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// What it's about.
    #[must_use]
    pub const fn kind(&self) -> &DiagnosticKind {
        &self.kind
    }
}

/// The kinds of [`Diagnostic`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiagnosticKind {
    /// A delegation from someone with no authority over the subject.
    #[error("{who} holds no authority over {subject} here, so this certificate grants nothing")]
    Unauthorized {
        /// The issuer.
        who: Name,

        /// The subject.
        subject: Name,
    },

    /// A delegation claiming something the issuer holds nothing comparable
    /// to, such as `/msg` when it holds `/crud`.
    #[error(
        "{issuer} holds {} over {subject}, none of which covers or falls under {claimed}, so this certificate grants nothing",
        list(.held)
    )]
    Unrelated {
        /// The issuer.
        issuer: Name,

        /// The subject.
        subject: Name,

        /// What the certificate says.
        claimed: Ability,

        /// The broadest abilities the issuer holds.
        held: Vec<Ability>,
    },

    /// A delegation claiming more than the issuer holds.
    #[error(
        "{issuer} only holds {granted} over {subject}, so this grants {granted}, not {claimed}"
    )]
    Attenuated {
        /// The issuer.
        issuer: Name,

        /// The subject.
        subject: Name,

        /// What the certificate says.
        claimed: Ability,

        /// What actually flows.
        granted: Ability,
    },

    /// An invocation beyond what the actor holds.
    #[error("{who} can't use {needed} on {subject}: {}", held_phrase(.held))]
    Insufficient {
        /// The actor.
        who: Name,

        /// The subject.
        subject: Name,

        /// The ability used.
        needed: Ability,

        /// The broadest abilities they hold, if any.
        held: Vec<Ability>,
    },

    /// A revocation from someone who isn't upstream of the target and isn't
    /// an admin of its subject.
    #[error(
        "{who} didn't issue anything {} relies on and isn't an admin of its subject, so this revocation does nothing",
        target_phrase(.target.as_ref())
    )]
    NotUpstream {
        /// The revoker.
        who: Name,

        /// The target's label.
        target: Option<CertId>,
    },
}

fn held_phrase(held: &[Ability]) -> alloc::string::String {
    if held.is_empty() {
        "they hold no authority over it".into()
    } else {
        alloc::format!("they only hold {}", list(held))
    }
}

/// `a`, `a and b`, or `a, b, and c`.
fn list(items: &[Ability]) -> alloc::string::String {
    use core::fmt::Write as _;
    let mut out = alloc::string::String::new();
    for (i, item) in items.iter().enumerate() {
        let sep = match (i, items.len()) {
            (0, _) => "",
            (_, 2) => " and ",
            (i, n) if i + 1 == n => ", and ",
            _ => ", ",
        };
        let _ = write!(out, "{sep}{item}");
    }
    out
}

fn target_phrase(target: Option<&CertId>) -> alloc::string::String {
    target.map_or_else(|| "it".into(), |t| alloc::format!("`{t}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{diagram::Diagram, parse};
    use alloc::{string::String, vec::Vec};
    use testresult::TestResult;

    fn ix(d: &Diagram, i: usize) -> EventIx {
        d.event_ix(i).expect("test indexes exist")
    }

    fn statuses(d: &Diagram) -> Vec<Option<Status>> {
        let now = d.now().expect("not empty");
        d.event_ixs().map(|e| d.status(e, now)).collect()
    }

    fn kinds(d: &Diagram) -> Vec<String> {
        d.diagnostics()
            .iter()
            .map(|diag| alloc::format!("{diag}"))
            .collect()
    }

    #[test]
    fn attenuates_to_what_the_issuer_holds() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[read(Doc)]->(Alice)
             (Alice)-[edit(Doc)]->(Bob)",
        )?;
        assert_eq!(d.granted(ix(&d, 1)), Some(&Level::Read.into()));
        assert_eq!(
            kinds(&d),
            ["Alice only holds read over Doc, so this grants read, not edit"]
        );
        Ok(())
    }

    #[test]
    fn delegating_without_authority() -> TestResult {
        let d = parse("rain\n(Alice)-[edit(Doc)]->(Bob)")?;
        assert_eq!(statuses(&d), [Some(Status::Unauthorized)]);
        assert_eq!(d.granted(ix(&d, 0)), None);
        assert_eq!(d.diagnostics().len(), 1);
        Ok(())
    }

    #[test]
    fn derives_transitive_handles() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit(Doc)]->(Team)
             (Team)-[read(Team)]->(Alice)
             (Alice)-[edit(Doc)]->(Bob)",
        )?;
        let route = d
            .cert(ix(&d, 2))
            .and_then(|c| c.authority.route())
            .expect("Alice reaches Doc through Team");
        assert_eq!(route.source(), ix(&d, 0));
        assert_eq!(route.through(), [ix(&d, 1)]);
        assert_eq!(route.held(), ix(&d, 1));
        // Alice is only a Read member of Team
        assert_eq!(d.granted(ix(&d, 2)), Some(&Level::Read.into()));
        Ok(())
    }

    /// [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/), "Revocation".
    #[test]
    fn revoking_downstream() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit(Doc)]->(Alice)
             (Alice)-[edit(Doc)]->(Bob)
             (Bob)-[read(Doc)]->(Carol)
             carol-dan: (Carol)-[read(Doc)]->(Dan)
             (Alice)-<revoke>-x[carol-dan]",
        )?;
        let revocation = ix(&d, 4);
        assert_eq!(
            statuses(&d),
            [
                Some(Status::Valid),
                Some(Status::Valid),
                Some(Status::Valid),
                Some(Status::Revoked { revocation }),
                Some(Status::Valid),
            ]
        );

        // The red dashes come from the Doc→Alice certificate
        let power = d
            .event(revocation)
            .and_then(|e| e.authority().route())
            .map(Route::held);
        assert_eq!(power, Some(ix(&d, 0)));

        // Before the revocation, Dan still had access
        assert_eq!(d.status(ix(&d, 3), ix(&d, 3)), Some(Status::Valid));
        Ok(())
    }

    /// [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/), "Invocation": revocation voids earlier edits.
    #[test]
    fn revocation_is_retrocausal() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[admin(Doc)]->(Alice)
             (Alice)-[edit(Doc)]->(Bob)
             (Bob)=[[edit]]=>(Doc)
             bob-carol: (Bob)-[edit(Doc)]->(Carol)
             (Carol)=[[edit]]=>(Doc)
             (Alice)-<revoke>-x[bob-carol]
             (Bob)=[[edit]]=>(Doc)",
        )?;
        let revocation = ix(&d, 5);
        let carol_edit = ix(&d, 4);

        assert_eq!(d.status(carol_edit, carol_edit), Some(Status::Valid));
        assert_eq!(
            statuses(&d),
            [
                Some(Status::Valid),
                Some(Status::Valid),
                Some(Status::Valid),
                Some(Status::Revoked { revocation }),
                Some(Status::Void {
                    revoked: ix(&d, 3),
                    revocation
                }),
                Some(Status::Valid),
                Some(Status::Valid),
            ]
        );
        assert_eq!(
            d.dependents(ix(&d, 1)).collect::<Vec<_>>(),
            [ix(&d, 2), ix(&d, 3), ix(&d, 4), ix(&d, 6)]
        );
        Ok(())
    }

    #[test]
    fn revoked_certificates_back_nothing_new() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit(Doc)]->(Alice)
             alice-bob: (Alice)-[read(Doc)]->(Bob)
             (Doc)-<revoke>-x[alice-bob]
             (Bob)-[read(Doc)]->(Carol)",
        )?;
        assert_eq!(statuses(&d).last(), Some(&Some(Status::Unauthorized)));
        Ok(())
    }

    #[test]
    fn only_upstream_or_admins_can_revoke() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit(Doc)]->(Alice)
             alice-bob: (Alice)-[read(Doc)]->(Bob)
             (Doc)-[read(Doc)]->(Dan)
             (Dan)-<revoke>-x[alice-bob]
             (Doc)-[admin(Doc)]->(Eve)
             (Eve)-<revoke>-x[alice-bob]",
        )?;
        let [.., dan, _, eve] = statuses(&d)[..] else {
            unreachable!("six events")
        };
        assert_eq!(dan, Some(Status::Unauthorized));
        assert_eq!(eve, Some(Status::Valid));
        assert_eq!(
            kinds(&d),
            [
                "Dan didn't issue anything `alice-bob` relies on and isn't an admin of its subject, so this revocation does nothing"
            ]
        );
        assert_eq!(
            d.status(ix(&d, 1), d.now().expect("not empty")),
            Some(Status::Revoked {
                revocation: ix(&d, 5)
            })
        );
        Ok(())
    }

    #[test]
    fn invoking_beyond_what_you_hold() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[read(Doc)]->(Alice)
             (Alice)=[[read]]=>(Doc)
             (Alice)=[[edit]]=>(Doc)",
        )?;
        assert_eq!(
            statuses(&d),
            [
                Some(Status::Valid),
                Some(Status::Valid),
                Some(Status::Unauthorized)
            ]
        );
        assert_eq!(
            kinds(&d),
            ["Alice can't use edit on Doc: they only hold read"]
        );
        Ok(())
    }

    #[test]
    fn reach_only_sees_the_past() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[read(Doc)]->(Alice)
             (Doc)-[edit(Doc)]->(Alice)",
        )?;
        let alice = d.principal_by_key("alice").expect("Alice is in it");
        let level = |now| d.reach(alice, now).first().map(|r| r.ability.clone());
        assert_eq!(level(ix(&d, 0)), Some(Level::Read.into()));
        assert_eq!(level(ix(&d, 1)), Some(Level::Edit.into()));
        Ok(())
    }

    /// [Keyhive Lab Note #7](https://www.inkandswitch.com/keyhive/notebook/07/), "A More Involved Example": hover over Bob.
    #[test]
    fn reach_through_groups() -> TestResult {
        let d = parse(
            "certflow
             (Thread 1)-[admin(Thread 1)]->(Commenters)
             (Commenters)-[admin(Commenters)]->(Alice)
             (Alice)-[edit(Commenters)]->(Carol)
             (Alice)-[edit(Commenters)]->(Bob)
             (Thread 2)-[admin(Thread 2)]->(Commenters)
             (Essay)-[admin(Essay)]->(Alice)
             (Alice)-[read(Essay)]->(Commenters)",
        )?;
        let bob = d
            .principals()
            .iter()
            .position(|p| p.key() == "bob")
            .map(PrincipalIx::new)
            .expect("Bob is in it");

        let reach: Vec<_> = d
            .reach(bob, d.now().expect("not empty"))
            .into_iter()
            .filter_map(|r| Some((d.principal(r.subject)?.key(), r.ability.level()?)))
            .collect();
        assert_eq!(
            reach,
            [
                ("thread-1", Level::Edit),
                ("commenters", Level::Edit),
                ("thread-2", Level::Edit),
                ("essay", Level::Read),
            ]
        );
        Ok(())
    }

    fn ability(s: &str) -> Ability {
        s.parse().expect("test abilities are valid")
    }

    #[test]
    fn command_paths_attenuate_by_prefix() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/crud(Doc)]->(Alice)
             (Alice)-[/crud/read(Doc)]->(Bob)
             (Bob)=[[/crud/read]]=>(Doc)
             (Bob)=[[/crud/write]]=>(Doc)",
        )?;
        assert_eq!(d.granted(ix(&d, 1)), Some(&ability("/crud/read")));
        assert_eq!(
            statuses(&d),
            [
                Some(Status::Valid),
                Some(Status::Valid),
                Some(Status::Valid),
                Some(Status::Unauthorized)
            ]
        );
        assert_eq!(
            kinds(&d),
            ["Bob can't use /crud/write on Doc: they only hold /crud/read"]
        );
        Ok(())
    }

    #[test]
    fn claiming_more_than_a_path_grants_the_path() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/crud/read(Doc)]->(Alice)
             (Alice)-[/crud(Doc)]->(Bob)",
        )?;
        assert_eq!(d.granted(ix(&d, 1)), Some(&ability("/crud/read")));
        assert_eq!(
            kinds(&d),
            ["Alice only holds /crud/read over Doc, so this grants /crud/read, not /crud"]
        );
        Ok(())
    }

    #[test]
    fn unrelated_abilities_grant_nothing() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/msg(Doc)]->(Alice)
             (Doc)-[read(Doc)]->(Alice)
             (Alice)-[/crud(Doc)]->(Bob)",
        )?;
        assert_eq!(d.granted(ix(&d, 2)), None);
        assert_eq!(
            kinds(&d),
            [
                "Alice holds read and /msg over Doc, none of which covers or falls under /crud, so this certificate grants nothing"
            ]
        );
        Ok(())
    }

    #[test]
    fn levels_and_paths_dont_mix_along_a_chain() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit(Doc)]->(Team)
             (Team)-[/(Team)]->(Alice)
             (Alice)-[edit(Doc)]->(Bob)",
        )?;
        assert_eq!(statuses(&d).last(), Some(&Some(Status::Unauthorized)));
        Ok(())
    }

    #[test]
    fn reach_lists_each_broadest_ability() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/crud/read(Doc)]->(Alice)
             (Doc)-[/msg(Doc)]->(Alice)
             (Doc)-[/msg/send(Doc)]->(Alice)",
        )?;
        let alice = d.principal_by_key("alice").expect("Alice is in it");
        let held: Vec<String> = d
            .reach(alice, d.now().expect("not empty"))
            .iter()
            .map(|r| r.ability.to_string())
            .collect();
        assert_eq!(held, ["/msg", "/crud/read"]);
        Ok(())
    }

    #[test]
    fn caveats_pile_up_along_routes() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/crud(Doc) where path = /docs]->(Team)
             (Team)-[/(Doc) where team.active]->(Alice)
             (Alice)-[/crud/read(Doc) where size < 1MB]->(Bob)
             (Bob)=[[/crud/read]]=>(Doc)",
        )?;
        let caveats = |i| -> Vec<&str> { d.caveats(ix(&d, i)).map(|(_, c)| c.as_str()).collect() };
        assert_eq!(caveats(0), ["path = /docs"]);
        assert_eq!(caveats(2), ["path = /docs", "team.active", "size < 1MB"]);
        assert_eq!(caveats(3), caveats(2));
        Ok(())
    }

    #[test]
    fn routes_prefer_fewer_caveats_then_earlier() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/(Doc) where x]->(Alice)
             (Doc)-[/(Doc)]->(Alice)
             (Doc)-[/(Doc)]->(Alice)
             (Alice)-[/crud(Doc)]->(Bob)",
        )?;
        let route = d
            .cert(ix(&d, 3))
            .and_then(|c| c.authority.route())
            .expect("Alice holds Doc");
        assert_eq!(route.source(), ix(&d, 1));
        assert_eq!(d.caveats(ix(&d, 3)).count(), 0);
        Ok(())
    }

    #[test]
    fn breadth_beats_fewer_caveats() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/crud/read(Doc)]->(Alice)
             (Doc)-[/crud(Doc) where x]->(Alice)
             (Alice)-[/crud(Doc)]->(Bob)",
        )?;
        assert_eq!(d.granted(ix(&d, 2)), Some(&ability("/crud")));
        assert_eq!(d.caveats(ix(&d, 2)).count(), 1);
        Ok(())
    }

    /// Among routes with as many caveats, the one with fewer hops, even
    /// when a longer one reaches the subject first.
    #[test]
    fn equal_caveats_prefer_fewer_hops() -> TestResult {
        let d = parse(
            "rain
             (G1)-[admin(G1) where c]->(Alice)
             (Doc)-[edit(Doc)]->(G1)
             (G2)-[admin(G2)]->(Alice)
             (G3)-[admin(G3)]->(G2)
             (Y)-[admin(Y)]->(G3)
             (Doc)-[edit(Doc) where d]->(Y)
             (Alice)=[[edit]]=>(Doc)",
        )?;
        let route = d
            .event(ix(&d, 6))
            .and_then(|e| e.authority().route())
            .expect("Alice reaches Doc");
        assert_eq!(route.source(), ix(&d, 1));
        assert_eq!(route.through(), [ix(&d, 0)]);
        Ok(())
    }

    /// Among routes equal in caveats and hops, the one through the earliest
    /// certificate to the holder, whatever order the groups were named in.
    #[test]
    fn equal_routes_go_through_the_earliest_membership() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit(Doc)]->(Team A)
             (Doc)-[edit(Doc)]->(Team Z)
             (Team Z)-[admin(Team Z)]->(Alice)
             (Team A)-[admin(Team A)]->(Alice)
             (Alice)=[[edit]]=>(Doc)",
        )?;
        let route = d
            .event(ix(&d, 4))
            .and_then(|e| e.authority().route())
            .expect("Alice reaches Doc");
        assert_eq!(route.through(), [ix(&d, 2)]);
        assert_eq!(route.source(), ix(&d, 1));
        Ok(())
    }

    #[test]
    fn reach_carries_its_routes_caveats() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/crud(Doc) where path = /docs]->(Team)
             (Team)-[/(Team) where team.active]->(Alice)",
        )?;
        let alice = d.principal_by_key("alice").expect("Alice is in it");
        let reach = d.reach(alice, d.now().expect("not empty"));
        let doc = reach
            .iter()
            .find(|r| d.principal(r.subject).map(crate::diagram::Principal::key) == Some("doc"))
            .expect("Alice reaches Doc through Team");
        let caveats: Vec<&str> = d
            .route_caveats(&doc.route)
            .into_iter()
            .map(|(_, c)| c.as_str())
            .collect();
        assert_eq!(caveats, ["path = /docs", "team.active"]);
        Ok(())
    }

    #[test]
    fn several_held_abilities_are_listed_in_words() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[/msg(Doc)]->(Alice)
             (Doc)-[read(Doc)]->(Alice)
             (Doc)-[/crud/read(Doc)]->(Alice)
             (Alice)-[/crud/write(Doc)]->(Bob)",
        )?;
        assert_eq!(
            kinds(&d),
            [
                "Alice holds read, /msg, and /crud/read over Doc, none of which covers or falls under /crud/write, so this certificate grants nothing"
            ]
        );
        Ok(())
    }
}
