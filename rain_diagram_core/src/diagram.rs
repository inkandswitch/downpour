//! The resolved, checked model of a diagram.
//!
//! [`Diagram::resolve`] turns a [`Document`] into indices instead of names:
//! every principal gets a stable column and a unique key, every omitted
//! subject is inferred, and every certificate reference (`via`, revocation
//! targets) points at exactly one earlier certificate. A `Diagram` can only
//! be built this way, so renderers never have to handle a dangling reference.
//!
//! Resolution also runs the [authority analysis](crate::authority): each
//! event records the route its authority took, and the diagram can answer
//! what was valid, revoked, or reachable at any moment.

use crate::{
    ast::{CertId, Delegation, Document, Kind, Name, Percent, Span, Spanned, Statement, Text},
    authority::{Authority, Diagnostic, Ledger, Reach, Status},
    level::Level,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    vec::Vec,
};
use thiserror::Error;

/// A resolved diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagram {
    kind: Kind,
    title: Option<Text>,
    principals: Vec<Principal>,
    events: Vec<Spanned<Event>>,
    regions: Vec<Region>,
    ledger: Ledger,
    diagnostics: Vec<Diagnostic>,
}

impl Diagram {
    /// Resolve names and references in a parsed document.
    ///
    /// ```
    /// use rain_diagram_core::{diagram::{Diagram, Event}, parser};
    ///
    /// let doc = parser::document(
    ///     "rain
    ///      (Doc)-[edit]->(Alice)
    ///      (Alice)-[read]->(Bob)",
    /// )?;
    /// let diagram = Diagram::resolve(&doc)?;
    ///
    /// // Alice's only authority is over the Doc, so that's the subject
    /// let Some(Event::Delegate(cert)) = diagram.events().get(1).map(|e| &e.node) else {
    ///     unreachable!()
    /// };
    /// assert_eq!(diagram.principal(cert.subject).map(|p| p.name().as_str()), Some("Doc"));
    /// # Ok::<(), rain_diagram_core::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`ResolveError`] for duplicate participants or labels,
    /// references to certificates that don't exist (or don't exist _yet_),
    /// ambiguous references, and subjects that can't be inferred.
    pub fn resolve(doc: &Document) -> Result<Self, ResolveError> {
        let mut resolver = Resolver::default();

        for participant in &doc.participants {
            if resolver.by_name.contains_key(&participant.node) {
                return Err(ResolveError::new(
                    participant.span,
                    ResolveErrorKind::DuplicateParticipant(participant.node.clone()),
                ));
            }
            resolver.intern(&participant.node);
        }

        for statement in &doc.statements {
            let span = statement.span;
            let (mut node, via) = resolver
                .statement(&statement.node)
                .map_err(|kind| ResolveError::new(span, kind))?;

            let Resolver {
                principals,
                events,
                ledger,
                diagnostics,
                ..
            } = &mut resolver;
            let names = |ix: PrincipalIx| {
                principals.get(ix.0).map_or_else(
                    || unreachable!("indices come from intern"),
                    |p| p.name.clone(),
                )
            };

            let (authority, diagnostic) = match &node {
                Event::Delegate(cert) => ledger.delegate(events, cert, &via, span, &names),
                Event::Invoke(invoke) => ledger.invoke(events, invoke, span, &names),
                Event::Revoke(revoke) => ledger.revoke(events, revoke, span, &names),
            };
            *node.authority_mut() = authority;
            diagnostics.extend(diagnostic);
            events.push(Spanned { node, span });
        }

        let regions = doc
            .regions
            .iter()
            .map(|r| {
                let colour = r
                    .node
                    .colour
                    .as_ref()
                    .map(|name| {
                        resolver.by_name.get(name).copied().ok_or_else(|| {
                            ResolveError::new(
                                r.span,
                                ResolveErrorKind::UnknownPrincipal(name.clone()),
                            )
                        })
                    })
                    .transpose()?;
                Ok(Region {
                    id: r.node.id.clone(),
                    colour,
                    note: r.node.note.clone(),
                    first: EventIx(r.node.statements.start),
                    last: EventIx(r.node.statements.end.saturating_sub(1)),
                })
            })
            .collect::<Result<_, ResolveError>>()?;

        Ok(Self {
            kind: doc.kind,
            title: doc.title.as_ref().map(|t| t.node.clone()),
            principals: resolver.principals,
            events: resolver.events,
            regions,
            ledger: resolver.ledger,
            diagnostics: resolver.diagnostics,
        })
    }

    /// Which notation to render.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// The title, if any.
    #[must_use]
    pub const fn title(&self) -> Option<&Text> {
        self.title.as_ref()
    }

    /// Principals in column order: `participant`s first, then by first
    /// appearance.
    #[must_use]
    pub fn principals(&self) -> &[Principal] {
        &self.principals
    }

    /// Find a principal by its [key](Principal::key).
    #[must_use]
    pub fn principal_by_key(&self, key: &str) -> Option<PrincipalIx> {
        self.principals
            .iter()
            .position(|p| p.key == key)
            .map(PrincipalIx)
    }

    /// Look up a principal.
    #[must_use]
    pub fn principal(&self, ix: PrincipalIx) -> Option<&Principal> {
        self.principals.get(ix.0)
    }

    /// Every event, in causal order.
    #[must_use]
    pub fn events(&self) -> &[Spanned<Event>] {
        &self.events
    }

    /// Look up an event.
    #[must_use]
    pub fn event(&self, ix: EventIx) -> Option<&Event> {
        self.events.get(ix.0).map(|e| &e.node)
    }

    /// Look up a certificate (a delegation event).
    #[must_use]
    pub fn cert(&self, ix: EventIx) -> Option<&Cert> {
        match self.event(ix)? {
            Event::Delegate(cert) => Some(cert),
            Event::Invoke(_) | Event::Revoke(_) => None,
        }
    }

    /// The index of the `index`th event, if there is one.
    #[must_use]
    pub const fn event_ix(&self, index: usize) -> Option<EventIx> {
        if index < self.events.len() {
            Some(EventIx(index))
        } else {
            None
        }
    }

    /// Every event index, in causal order.
    pub fn event_ixs(&self) -> impl DoubleEndedIterator<Item = EventIx> + ExactSizeIterator {
        (0..self.events.len()).map(EventIx)
    }

    /// The last event: the moment after everything has happened.
    #[must_use]
    pub const fn now(&self) -> Option<EventIx> {
        match self.events.len() {
            0 => None,
            len => Some(EventIx(len - 1)),
        }
    }

    /// An event's standing as of `now` (inclusive), or `None` if it hasn't
    /// happened yet.
    #[must_use]
    pub fn status(&self, ix: EventIx, now: EventIx) -> Option<Status> {
        self.ledger.status(&self.events, ix, now)
    }

    /// The level a certificate actually grants, after attenuation. `None`
    /// for unauthorised certificates and non-certificate events.
    #[must_use]
    pub fn granted(&self, ix: EventIx) -> Option<Level> {
        self.ledger.granted(ix)
    }

    /// Everything `holder` can act on as of `now`, at the highest level
    /// available. Excludes the holder itself.
    #[must_use]
    pub fn reach(&self, holder: PrincipalIx, now: EventIx) -> Vec<Reach> {
        self.ledger
            .reach(&self.events, holder, self.principals.len(), now)
    }

    /// Events that rely on `cert`, which revoking it would void.
    pub fn dependents(&self, cert: EventIx) -> impl Iterator<Item = EventIx> + '_ {
        self.event_ixs()
            .filter(move |&ix| ix != cert && self.ledger.relies_on(ix, cert))
    }

    /// Labelled regions, in the order they open.
    #[must_use]
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// A lane for each region, so regions that share rows never share a
    /// lane: outer regions get lower lanes than the regions nested inside
    /// them, and overlapping regions get different ones.
    pub(crate) fn region_lanes(&self) -> Vec<usize> {
        let mut order: Vec<(usize, &Region)> = self.regions.iter().enumerate().collect();
        // Earlier first; for the same start, longer (outer) first
        order.sort_by_key(|(_, r)| (r.first, core::cmp::Reverse(r.last)));

        let mut placed: Vec<(usize, &Region, usize)> = Vec::with_capacity(order.len());
        for (i, region) in order {
            let taken: BTreeSet<usize> = placed
                .iter()
                .filter(|(_, other, _)| other.first <= region.last && region.first <= other.last)
                .map(|&(_, _, lane)| lane)
                .collect();
            // n regions can take at most n lanes, so one of 0..=n is free
            let lane = (0..=taken.len())
                .find(|lane| !taken.contains(lane))
                .unwrap_or(taken.len());
            placed.push((i, region, lane));
        }

        placed.sort_by_key(|&(i, _, _)| i);
        placed.into_iter().map(|(_, _, lane)| lane).collect()
    }

    /// Things that resolve but don't do what they look like they do.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// A principal's key, or `""` for an index from another diagram.
    pub(crate) fn key(&self, ix: PrincipalIx) -> &str {
        self.principal(ix).map_or("", |p| p.key.as_str())
    }
}

/// A labelled stretch of events (`>>> A: note` … `<<< A`), drawn as a
/// coloured box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Its name.
    pub id: CertId,

    /// The principal whose colour it takes, if one was named.
    pub colour: Option<PrincipalIx>,

    /// What it's about, if anything.
    pub note: Option<Text>,

    /// The first event inside it.
    pub first: EventIx,

    /// The last event inside it.
    pub last: EventIx,
}

impl Region {
    /// The note, or the id if there's no note.
    #[must_use]
    pub fn caption(&self) -> &str {
        self.note.as_ref().map_or(self.id.as_str(), Text::as_str)
    }
}

/// A principal and its unique key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    name: Name,
    key: String,
}

impl Principal {
    /// The name as written.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }

    /// A unique, URL- and id-safe key derived from the name, such as
    /// `comment-thread-1`.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// A principal's position in [`Diagram::principals`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalIx(usize);

impl PrincipalIx {
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The column index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// An event's position in [`Diagram::events`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventIx(usize);

impl EventIx {
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The row index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// A resolved statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A certificate was signed.
    Delegate(Cert),

    /// A principal used its authority.
    Invoke(Invoke),

    /// A certificate was revoked.
    Revoke(Revoke),
}

impl Event {
    /// Why the event was allowed.
    #[must_use]
    pub const fn authority(&self) -> &Authority {
        match self {
            Self::Delegate(cert) => &cert.authority,
            Self::Invoke(invoke) => &invoke.authority,
            Self::Revoke(revoke) => &revoke.authority,
        }
    }

    const fn authority_mut(&mut self) -> &mut Authority {
        match self {
            Self::Delegate(cert) => &mut cert.authority,
            Self::Invoke(invoke) => &mut invoke.authority,
            Self::Revoke(revoke) => &mut revoke.authority,
        }
    }
}

/// A resolved certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cert {
    /// The explicit label, or the implicit `issuer-audience` one.
    pub id: CertId,

    /// Who signed it.
    pub issuer: PrincipalIx,

    /// Who receives it.
    pub audience: PrincipalIx,

    /// What it grants authority over.
    pub subject: PrincipalIx,

    /// The level it claims. What actually flows may be less: see
    /// [`Diagram::granted`].
    pub level: Level,

    /// How the issuer came to hold the subject.
    pub authority: Authority,

    /// Where its label sits along the arrow, if placed by hand.
    pub at: Option<Percent>,

    /// A note, if any.
    pub note: Option<Text>,
}

/// A resolved invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoke {
    /// Who acts.
    pub by: PrincipalIx,

    /// The access exercised.
    pub level: Level,

    /// What is acted on.
    pub on: PrincipalIx,

    /// How the actor came to hold the subject.
    pub authority: Authority,

    /// Where its label sits along the arrow, if placed by hand.
    pub at: Option<Percent>,

    /// A note, if any.
    pub note: Option<Text>,
}

/// A resolved revocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revoke {
    /// Who revokes.
    pub by: PrincipalIx,

    /// The certificate revoked.
    pub target: EventIx,

    /// Where the revoker's power to revoke came from.
    pub authority: Authority,

    /// Where its label sits along the arrow, if placed by hand.
    pub at: Option<Percent>,

    /// A note, if any.
    pub note: Option<Text>,
}

#[derive(Debug, Default)]
struct Resolver {
    principals: Vec<Principal>,
    by_name: BTreeMap<Name, PrincipalIx>,
    keys: BTreeSet<String>,
    labels: BTreeSet<CertId>,
    events: Vec<Spanned<Event>>,
    ledger: Ledger,
    diagnostics: Vec<Diagnostic>,
}

impl Resolver {
    fn intern(&mut self, name: &Name) -> PrincipalIx {
        if let Some(&ix) = self.by_name.get(name) {
            return ix;
        }

        let base = slug(name.as_str());
        // n principals hold at most n keys, so one of n + 1 candidates is free
        let key = (1..=self.principals.len() + 1)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base}-{n}")
                }
            })
            .find(|key| !self.keys.contains(key))
            .unwrap_or(base);

        let ix = PrincipalIx(self.principals.len());
        self.keys.insert(key.clone());
        self.by_name.insert(name.clone(), ix);
        self.principals.push(Principal {
            name: name.clone(),
            key,
        });
        ix
    }

    /// Resolve names. Authority is filled in afterwards by the ledger; `via`
    /// is returned alongside because it only constrains that analysis.
    fn statement(
        &mut self,
        statement: &Statement,
    ) -> Result<(Event, Vec<EventIx>), ResolveErrorKind> {
        match statement {
            Statement::Delegate(d) => self
                .delegate(d)
                .map(|(cert, via)| (Event::Delegate(cert), via)),
            Statement::Invoke(i) => Ok((
                Event::Invoke(Invoke {
                    by: self.intern(&i.by),
                    level: i.level,
                    on: self.intern(&i.on),
                    authority: Authority::Unauthorized,
                    at: i.at,
                    note: i.note.clone(),
                }),
                Vec::new(),
            )),
            Statement::Revoke(r) => Ok((
                Event::Revoke(Revoke {
                    by: self.intern(&r.by),
                    target: self.lookup(&r.target)?,
                    authority: Authority::Unauthorized,
                    at: r.at,
                    note: r.note.clone(),
                }),
                Vec::new(),
            )),
        }
    }

    fn delegate(&mut self, d: &Delegation) -> Result<(Cert, Vec<EventIx>), ResolveErrorKind> {
        let issuer = self.intern(&d.issuer);
        let audience = self.intern(&d.audience);
        let subject = match &d.subject {
            Some(subject) => self.intern(subject),
            None => self.infer_subject(issuer, d.level)?,
        };

        // `via` is a chain: the first is held by the issuer, and each next
        // one by the previous one's subject
        let mut via = Vec::with_capacity(d.via.len());
        let mut holder = (issuer, &d.issuer);
        for id in &d.via {
            let ix = self.lookup(id)?;
            let cert = self.cert(ix).filter(|cert| cert.audience == holder.0);
            let Some(cert) = cert else {
                return Err(ResolveErrorKind::ViaNotHeld {
                    via: id.clone(),
                    holder: holder.1.clone(),
                });
            };
            via.push(ix);
            holder = (cert.subject, self.name_ref(cert.subject));
        }

        let id = match &d.id {
            Some(id) => {
                if !self.labels.insert(id.clone()) {
                    return Err(ResolveErrorKind::DuplicateLabel(id.clone()));
                }
                id.clone()
            }
            None => self.implicit_id(issuer, audience),
        };

        let cert = Cert {
            id,
            issuer,
            audience,
            subject,
            level: d.level,
            authority: Authority::Unauthorized,
            at: d.at,
            note: d.note.clone(),
        };
        Ok((cert, via))
    }

    /// With no explicit subject:
    ///
    /// - Admin is over the issuer itself. Granting Admin is almost always
    ///   ownership or group membership: `(Team)-[admin]->(Alice)` adds Alice
    ///   to Team, even if Team already holds a Doc.
    /// - Anything else passes on the one other subject the issuer holds, as
    ///   in `(Alice)-[edit]->(Bob)`, or is over the issuer if it holds none.
    fn infer_subject(
        &self,
        issuer: PrincipalIx,
        level: Level,
    ) -> Result<PrincipalIx, ResolveErrorKind> {
        if level == Level::Admin {
            return Ok(issuer);
        }

        let held: BTreeSet<PrincipalIx> = self
            .certs()
            .filter(|(_, cert)| cert.audience == issuer && cert.subject != issuer)
            .map(|(_, cert)| cert.subject)
            .collect();

        let mut held = held.into_iter();
        match (held.next(), held.next()) {
            (None, _) => Ok(issuer),
            (Some(subject), None) => Ok(subject),
            (Some(first), Some(second)) => Err(ResolveErrorKind::AmbiguousSubject {
                issuer: self.name(issuer),
                candidates: [first, second]
                    .into_iter()
                    .chain(held)
                    .map(|ix| self.name(ix))
                    .collect(),
            }),
        }
    }

    /// Find the single earlier certificate labelled `id`.
    fn lookup(&self, id: &CertId) -> Result<EventIx, ResolveErrorKind> {
        let mut matches = self.certs().filter(|(_, cert)| &cert.id == id);
        match (matches.next(), matches.next()) {
            (Some((ix, _)), None) => Ok(ix),
            (None, _) => Err(ResolveErrorKind::UnknownCert(id.clone())),
            (Some(_), Some(_)) => Err(ResolveErrorKind::AmbiguousCert(id.clone())),
        }
    }

    fn certs(&self) -> impl Iterator<Item = (EventIx, &Cert)> {
        self.events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match &e.node {
                Event::Delegate(cert) => Some((EventIx(i), cert)),
                Event::Invoke(_) | Event::Revoke(_) => None,
            })
    }

    fn cert(&self, ix: EventIx) -> Option<&Cert> {
        self.certs().find(|&(i, _)| i == ix).map(|(_, cert)| cert)
    }

    fn name(&self, ix: PrincipalIx) -> Name {
        self.name_ref(ix).clone()
    }

    fn name_ref(&self, ix: PrincipalIx) -> &Name {
        self.principals
            .get(ix.0)
            .map_or_else(|| unreachable!("indices come from intern"), |p| &p.name)
    }

    fn implicit_id(&self, issuer: PrincipalIx, audience: PrincipalIx) -> CertId {
        let key = |ix: PrincipalIx| self.principals.get(ix.0).map_or("", |p| p.key.as_str());
        CertId::new(&format!("{}-{}", key(issuer), key(audience)))
            .unwrap_or_else(|_| unreachable!("keys are non-empty slugs"))
    }
}

/// Lowercase, with runs of anything but letters and digits collapsed to `-`.
fn slug(name: &str) -> String {
    let slug = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() { "p".into() } else { slug }
}

/// A document that parsed but doesn't make sense.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{kind}")]
pub struct ResolveError {
    span: Span,
    kind: ResolveErrorKind,
}

impl ResolveError {
    /// Create an error at `span`.
    #[must_use]
    pub const fn new(span: Span, kind: ResolveErrorKind) -> Self {
        Self { span, kind }
    }

    /// The line the problem is on.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// What the problem is.
    #[must_use]
    pub const fn kind(&self) -> &ResolveErrorKind {
        &self.kind
    }
}

/// The ways a document can fail to resolve.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveErrorKind {
    /// The same principal in `participant` twice.
    #[error("{0} is already a participant")]
    DuplicateParticipant(Name),

    /// A region coloured after someone who isn't in the diagram.
    #[error("{0} isn't in this diagram, so the region can't take their colour")]
    UnknownPrincipal(Name),

    /// The same explicit label on two certificates.
    #[error("the label `{0}` is already used")]
    DuplicateLabel(CertId),

    /// No earlier certificate has this label.
    #[error("no earlier certificate is labelled `{0}`")]
    UnknownCert(CertId),

    /// More than one earlier certificate has this (implicit) label.
    #[error("more than one certificate is labelled `{0}`; give the one you mean an explicit label")]
    AmbiguousCert(CertId),

    /// The subject was omitted, but the issuer holds several.
    #[error(
        "{issuer} holds authority over several subjects ({candidates:?}); name one, as in `[level(Subject)]`"
    )]
    AmbiguousSubject {
        /// The issuer whose subject was omitted.
        issuer: Name,

        /// The subjects it could have meant.
        candidates: Vec<Name>,
    },

    /// A `via` certificate isn't held by the issuer (for the first) or by
    /// the previous one's subject (for the rest).
    #[error("`{via}` is not a certificate to {holder}, so it can't carry authority for them")]
    ViaNotHeld {
        /// The `via` label.
        via: CertId,

        /// Who should have held it.
        holder: Name,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use testresult::TestResult;

    fn resolve(src: &str) -> Result<Diagram, crate::Error> {
        Ok(Diagram::resolve(&parser::document(src)?)?)
    }

    fn resolve_err(src: &str) -> Option<ResolveErrorKind> {
        match resolve(src) {
            Err(crate::Error::Resolve(e)) => Some(e.kind().clone()),
            Ok(_) | Err(crate::Error::Parse(_)) => None,
        }
    }

    fn keys(d: &Diagram) -> Vec<&str> {
        d.principals().iter().map(Principal::key).collect()
    }

    #[test]
    fn column_order_and_keys() -> TestResult {
        let d = resolve(
            "rain
             participant Bob
             (Comment Thread 1)-[admin]->(Commenters)
             (Commenters)-[admin]->(Bob)
             (bob)-[read]->(📱)",
        )?;

        assert_eq!(
            keys(&d),
            ["bob", "comment-thread-1", "commenters", "bob-2", "p"]
        );
        Ok(())
    }

    #[test]
    fn colliding_keys_get_suffixes() -> TestResult {
        let d = resolve("rain\n(A)-[read]->(a)\n(a!)-[read]->(Ä)")?;
        assert_eq!(keys(&d), ["a", "a-2", "a-3", "ä"]);
        Ok(())
    }

    #[test]
    fn event_indices_are_checked() -> TestResult {
        let d = resolve("rain\n(A)-[read]->(B)\n(B)=[[read]]=>(A)")?;
        assert_eq!(d.event_ix(1), Some(EventIx(1)));
        assert_eq!(d.event_ix(2), None);
        assert_eq!(d.now(), Some(EventIx(1)));
        assert_eq!(resolve("rain")?.now(), None);
        Ok(())
    }

    #[test]
    fn infers_subjects() -> TestResult {
        let d = resolve(
            "rain
             (Doc)-[edit]->(Alice)
             (Alice)-[edit]->(Bob)
             (Bob)-[read]->(Carol)",
        )?;

        let subjects: Vec<_> = (0..3)
            .filter_map(|i| d.cert(EventIx(i)))
            .map(|c| d.key(c.subject))
            .collect();
        assert_eq!(subjects, ["doc", "doc", "doc"]);
        Ok(())
    }

    #[test]
    fn ambiguous_subject() {
        assert!(matches!(
            resolve_err(
                "rain
                 (Essay)-[admin]->(Alice)
                 (Commenters)-[admin]->(Alice)
                 (Alice)-[read]->(Bob)",
            ),
            Some(ResolveErrorKind::AmbiguousSubject { candidates, .. }) if candidates.len() == 2
        ));
    }

    #[test]
    fn revocation_by_explicit_and_implicit_label() -> TestResult {
        let d = resolve(
            "rain
             (Doc)-[edit]->(Alice)
             cd: (Alice)-[read]->(Dan)
             (Alice)-<revoke>-x[cd]
             (Alice)-<revoke>-x[doc-alice]",
        )?;

        let targets: Vec<_> = d
            .events()
            .iter()
            .filter_map(|e| match &e.node {
                Event::Revoke(r) => d.cert(r.target).map(|c| c.id.as_str()),
                Event::Delegate(_) | Event::Invoke(_) => None,
            })
            .collect();
        assert_eq!(targets, ["cd", "doc-alice"]);
        Ok(())
    }

    #[test]
    fn revocation_must_come_after_its_target() {
        assert!(matches!(
            resolve_err(
                "rain
                 (Alice)-<revoke>-x[cd]
                 cd: (Alice)-[read]->(Dan)",
            ),
            Some(ResolveErrorKind::UnknownCert(_))
        ));
    }

    #[test]
    fn ambiguous_implicit_label() {
        assert!(matches!(
            resolve_err(
                "rain
                 (Alice)-[read]->(Dan)
                 (Alice)-[edit]->(Dan)
                 (Alice)-<revoke>-x[alice-dan]",
            ),
            Some(ResolveErrorKind::AmbiguousCert(_))
        ));
    }

    #[test]
    fn duplicate_label() {
        assert!(matches!(
            resolve_err("rain\nx: (A)-[read]->(B)\nx: (A)-[read]->(C)"),
            Some(ResolveErrorKind::DuplicateLabel(_))
        ));
    }

    #[test]
    fn via_must_reach_the_issuer() -> TestResult {
        let ok = resolve(
            "rain
             (Doc)-[edit]->(Team)
             ta: (Team)-[admin]->(Alice)
             (Alice)-[edit(Doc) via ta]->(Bob)",
        )?;
        assert_eq!(
            ok.cert(EventIx(2))
                .and_then(|c| c.authority.route())
                .map(|r| (r.source(), r.through().to_vec())),
            Some((EventIx(0), Vec::from([EventIx(1)])))
        );

        assert!(matches!(
            resolve_err(
                "rain
                 dt: (Doc)-[edit]->(Team)
                 (Alice)-[edit(Doc) via dt]->(Bob)",
            ),
            Some(ResolveErrorKind::ViaNotHeld { .. })
        ));
        Ok(())
    }
}
