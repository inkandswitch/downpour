//! The resolved, checked model of a diagram.
//!
//! [`Diagram::resolve`] turns a [`Document`] into indices instead of names:
//! every principal gets a stable column and a unique key, every omitted
//! subject is inferred, and every certificate reference (`via`, revocation
//! targets) points at exactly one earlier certificate. A `Diagram` can only
//! be built this way, so renderers never have to handle a dangling reference.

use crate::{
    ast::{CertId, Delegation, Document, Kind, Name, Span, Spanned, Statement, Text},
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
            let node = resolver
                .statement(&statement.node)
                .map_err(|kind| ResolveError::new(statement.span, kind))?;

            resolver.events.push(Spanned {
                node,
                span: statement.span,
            });
        }

        Ok(Self {
            kind: doc.kind,
            title: doc.title.as_ref().map(|t| t.node.clone()),
            principals: resolver.principals,
            events: resolver.events,
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

    /// A principal's key, or `""` for an index from another diagram.
    pub(crate) fn key(&self, ix: PrincipalIx) -> &str {
        self.principal(ix).map_or("", |p| p.key.as_str())
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

    /// The level it claims. What actually flows may be less (attenuation).
    pub level: Level,

    /// Certificates carrying the subject's authority to the issuer.
    pub via: Vec<EventIx>,

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

    fn statement(&mut self, statement: &Statement) -> Result<Event, ResolveErrorKind> {
        match statement {
            Statement::Delegate(d) => self.delegate(d).map(Event::Delegate),
            Statement::Invoke(i) => Ok(Event::Invoke(Invoke {
                by: self.intern(&i.by),
                level: i.level,
                on: self.intern(&i.on),
                note: i.note.clone(),
            })),
            Statement::Revoke(r) => Ok(Event::Revoke(Revoke {
                by: self.intern(&r.by),
                target: self.lookup(&r.target)?,
                note: r.note.clone(),
            })),
        }
    }

    fn delegate(&mut self, d: &Delegation) -> Result<Cert, ResolveErrorKind> {
        let issuer = self.intern(&d.issuer);
        let audience = self.intern(&d.audience);
        let subject = match &d.subject {
            Some(subject) => self.intern(subject),
            None => self.infer_subject(issuer)?,
        };

        let via = d
            .via
            .iter()
            .map(|id| {
                let ix = self.lookup(id)?;
                match self.cert(ix) {
                    Some(cert) if cert.audience == issuer => Ok(ix),
                    Some(_) | None => Err(ResolveErrorKind::ViaNotHeldByIssuer {
                        via: id.clone(),
                        issuer: d.issuer.clone(),
                    }),
                }
            })
            .collect::<Result<_, _>>()?;

        let id = match &d.id {
            Some(id) => {
                if !self.labels.insert(id.clone()) {
                    return Err(ResolveErrorKind::DuplicateLabel(id.clone()));
                }
                id.clone()
            }
            None => self.implicit_id(issuer, audience),
        };

        Ok(Cert {
            id,
            issuer,
            audience,
            subject,
            level: d.level,
            via,
            note: d.note.clone(),
        })
    }

    /// With no explicit subject, use the one other subject the issuer holds
    /// authority over, or the issuer itself if it holds none.
    fn infer_subject(&self, issuer: PrincipalIx) -> Result<PrincipalIx, ResolveErrorKind> {
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
        self.principals.get(ix.0).map_or_else(
            || unreachable!("indices come from intern"),
            |p| p.name.clone(),
        )
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

    /// A `via` certificate isn't one the issuer received.
    #[error("`{via}` is not a certificate to {issuer}, so it can't carry authority for them")]
    ViaNotHeldByIssuer {
        /// The `via` label.
        via: CertId,

        /// The issuer of the delegation.
        issuer: Name,
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
            ok.cert(EventIx(2)).map(|c| c.via.clone()),
            Some(Vec::from([EventIx(1)]))
        );

        assert!(matches!(
            resolve_err(
                "rain
                 dt: (Doc)-[edit]->(Team)
                 (Alice)-[edit(Doc) via dt]->(Bob)",
            ),
            Some(ResolveErrorKind::ViaNotHeldByIssuer { .. })
        ));
        Ok(())
    }
}
