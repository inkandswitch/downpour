//! Who could do what, when, and why.
//!
//! Every principal holds Admin over itself. Authority over anything else
//! arrives through certificates: `(I)-[L(S)]->(A)` lets A act on S at level
//! L, but only as far as I could when it signed. What actually flows is the
//! lesser of the two (_attenuation_).
//!
//! Groups make authority transitive. If Alice holds authority over Team, and
//! Team holds Edit over Doc, then Alice holds authority over Doc through
//! Team, at the least of the levels along the way. The certificate carrying
//! Doc's authority past Team (Team → Alice) is a _transitive handle_.
//!
//! The rules, applied in causal order:
//!
//! | Event        | Authorised when                                                       | Effect                                       |
//! |--------------|-----------------------------------------------------------------------|----------------------------------------------|
//! | Delegation   | the issuer holds the subject (at any level)                           | grants `min(claimed, held)`                  |
//! | Invocation   | the actor holds the subject at the level used                         |                                              |
//! | Revocation   | the revoker issued something the target relies on, or holds Admin     | voids the target and everything relying on it |
//!
//! Revocation is retrocausal: it voids what relied on the target even if
//! that happened earlier, including invocations. Once effective, a
//! revocation stays effective; revoking the revoker later doesn't undo it.
//!
//! Each delegation and invocation relies on exactly one [`Route`], chosen
//! when it happens: the one granting the most (up to what was claimed), then
//! the fewest hops, then the earliest certificates. Keyhive certificates name
//! their proof, so a later alternate route doesn't rescue a voided one.
//!
//! ```
//! use rain_diagram_core::{authority::Status, parse};
//!
//! let d = parse(
//!     "rain
//!      (Doc)-[edit]->(Alice)
//!      (Alice)-[edit]->(Bob)
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
    ast::{CertId, Name, Span, Spanned},
    diagram::{Cert, Event, EventIx, Invoke, PrincipalIx, Revoke},
    level::Level,
};
use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
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
    pub level: Level,

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
    /// For an authorised delegation, the level that actually flows.
    granted: Option<Level>,

    /// Certificates this event relies on, itself included for a delegation.
    deps: BTreeSet<EventIx>,

    /// For a certificate, the first revocation that took effect.
    revoked_by: Option<EventIx>,
}

/// The causal past: events with index below this.
#[derive(Debug, Clone, Copy)]
struct Horizon(usize);

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
            facts.granted = Some(cert.level);
            (Authority::Root, None)
        } else {
            let horizon = Horizon(ix.index());
            match self.best(events, cert.issuer, cert.subject, cert.level, via, horizon) {
                Some((granted, route)) => {
                    facts.granted = Some(granted);
                    facts.deps.extend(self.deps_of(&route));
                    let attenuated = (granted < cert.level).then(|| {
                        Diagnostic::new(
                            span,
                            DiagnosticKind::Attenuated {
                                issuer: names(cert.issuer),
                                subject: names(cert.subject),
                                claimed: cert.level,
                                granted,
                            },
                        )
                    });
                    (Authority::Route(route), attenuated)
                }
                None => (
                    Authority::Unauthorized,
                    Some(Diagnostic::new(
                        span,
                        DiagnosticKind::Unauthorized {
                            who: names(cert.issuer),
                            subject: names(cert.subject),
                        },
                    )),
                ),
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

        let (authority, diagnostic) = if invoke.by == invoke.on {
            (Authority::Root, None)
        } else {
            match self.best(events, invoke.by, invoke.on, invoke.level, &[], horizon) {
                Some((held, route)) if held >= invoke.level => {
                    facts.deps = self.deps_of(&route);
                    (Authority::Route(route), None)
                }
                held => (
                    Authority::Unauthorized,
                    Some(Diagnostic::new(
                        span,
                        DiagnosticKind::Insufficient {
                            who: names(invoke.by),
                            subject: names(invoke.on),
                            needed: invoke.level,
                            held: held.map(|(level, _)| level),
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
            self.search(events, revoke.by, subject, Level::Admin, &[], horizon)
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

    /// The level a delegation actually grants, if it was authorised.
    pub(crate) fn granted(&self, ix: EventIx) -> Option<Level> {
        self.facts.get(ix.index())?.granted
    }

    /// Whether `cert` relies on (or is) `on`.
    pub(crate) fn relies_on(&self, ix: EventIx, on: EventIx) -> bool {
        self.facts
            .get(ix.index())
            .is_some_and(|f| f.deps.contains(&on))
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

    pub(crate) fn reach(
        &self,
        events: &[Spanned<Event>],
        holder: PrincipalIx,
        principals: usize,
        now: EventIx,
    ) -> Vec<Reach> {
        let horizon = Horizon(now.index() + 1);
        (0..principals)
            .map(PrincipalIx::new)
            .filter(|&subject| subject != holder)
            .filter_map(|subject| {
                let (level, route) =
                    self.best(events, holder, subject, Level::Admin, &[], horizon)?;
                Some(Reach {
                    subject,
                    level,
                    route,
                })
            })
            .collect()
    }

    /// The route granting the most (at most `claimed`), with that level.
    fn best(
        &self,
        events: &[Spanned<Event>],
        holder: PrincipalIx,
        subject: PrincipalIx,
        claimed: Level,
        via: &[EventIx],
        horizon: Horizon,
    ) -> Option<(Level, Route)> {
        Level::ALL
            .iter()
            .rev()
            .filter(|&&level| level <= claimed)
            .find_map(|&level| {
                let path = self.search(events, holder, subject, level, via, horizon)?;
                Some((level, Route::from_path(path)?))
            })
    }

    /// Shortest path of usable certificates, each granting at least
    /// `at_least`, from `holder` to `subject`, starting with `via`.
    /// Returned holder first.
    fn search(
        &self,
        events: &[Spanned<Event>],
        holder: PrincipalIx,
        subject: PrincipalIx,
        at_least: Level,
        via: &[EventIx],
        horizon: Horizon,
    ) -> Option<Vec<EventIx>> {
        let usable = |ix: EventIx| self.usable(ix, horizon) && self.granted(ix) >= Some(at_least);

        let mut start = holder;
        for &ix in via {
            if !usable(ix) {
                return None;
            }
            start = cert_at(events, ix)?.subject;
        }

        let mut parent: BTreeMap<PrincipalIx, (PrincipalIx, EventIx)> = BTreeMap::new();
        let mut queue = VecDeque::from([start]);
        let mut seen = BTreeSet::from([start]);

        while let Some(node) = queue.pop_front() {
            if node == subject {
                let mut path = Vec::new();
                let mut at = node;
                while let Some(&(prev, ix)) = parent.get(&at) {
                    path.push(ix);
                    at = prev;
                }
                path.extend(via.iter().rev());
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
                if seen.insert(next) {
                    parent.insert(next, (node, ix));
                    queue.push_back(next);
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
        claimed: Level,

        /// What actually flows.
        granted: Level,
    },

    /// An invocation beyond what the actor holds.
    #[error("{who} can't use {needed} on {subject}: {}", held_phrase(*.held))]
    Insufficient {
        /// The actor.
        who: Name,

        /// The subject.
        subject: Name,

        /// The level used.
        needed: Level,

        /// The most they hold, if anything.
        held: Option<Level>,
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

fn held_phrase(held: Option<Level>) -> alloc::string::String {
    held.map_or_else(
        || "they hold no authority over it".into(),
        |level| alloc::format!("they only hold {level}"),
    )
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
             (Doc)-[read]->(Alice)
             (Alice)-[edit]->(Bob)",
        )?;
        assert_eq!(d.granted(ix(&d, 1)), Some(Level::Read));
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
             (Doc)-[edit]->(Team)
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
        assert_eq!(d.granted(ix(&d, 2)), Some(Level::Read));
        Ok(())
    }

    /// [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/), "Revocation".
    #[test]
    fn revoking_downstream() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit]->(Alice)
             (Alice)-[edit]->(Bob)
             (Bob)-[read]->(Carol)
             (Carol)-[read]->(Dan)
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
             (Doc)-[admin]->(Alice)
             (Alice)-[edit]->(Bob)
             (Bob)=[[edit]]=>(Doc)
             (Bob)-[edit]->(Carol)
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
             (Doc)-[edit]->(Alice)
             (Alice)-[read]->(Bob)
             (Doc)-<revoke>-x[alice-bob]
             (Bob)-[read]->(Carol)",
        )?;
        assert_eq!(statuses(&d).last(), Some(&Some(Status::Unauthorized)));
        Ok(())
    }

    #[test]
    fn only_upstream_or_admins_can_revoke() -> TestResult {
        let d = parse(
            "rain
             (Doc)-[edit]->(Alice)
             (Alice)-[read]->(Bob)
             (Doc)-[read]->(Dan)
             (Dan)-<revoke>-x[alice-bob]
             (Doc)-[admin]->(Eve)
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
             (Doc)-[read]->(Alice)
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
             (Doc)-[read]->(Alice)
             (Doc)-[edit]->(Alice)",
        )?;
        let alice = d.principal_by_key("alice").expect("Alice is in it");
        let level = |now| d.reach(alice, now).first().map(|r| r.level);
        assert_eq!(level(ix(&d, 0)), Some(Level::Read));
        assert_eq!(level(ix(&d, 1)), Some(Level::Edit));
        Ok(())
    }

    /// [Keyhive Lab Note #7](https://www.inkandswitch.com/keyhive/notebook/07/), "A More Involved Example": hover over Bob.
    #[test]
    fn reach_through_groups() -> TestResult {
        let d = parse(
            "certflow
             (Thread 1)-[admin]->(Commenters)
             (Commenters)-[admin]->(Alice)
             (Alice)-[edit(Commenters)]->(Carol)
             (Alice)-[edit(Commenters)]->(Bob)
             (Thread 2)-[admin]->(Commenters)
             (Essay)-[admin]->(Alice)
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
            .filter_map(|r| Some((d.principal(r.subject)?.key(), r.level)))
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
}
