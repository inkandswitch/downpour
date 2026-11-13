//! Export to the JSON format used by the renderer in the [Keyhive Lab Notes](https://www.inkandswitch.com/keyhive/notebook/08/).
//!
//! The [Keyhive Lab Notes'](https://www.inkandswitch.com/keyhive/notebook/08/) interactive rain diagrams read `{ agents, events }`, where
//! events are delegations (`from`/`to`/`level`/`subject`), invocations
//! (`invoke`/`by`/`on`), and revocations (`revoke`/`by`). Exporting to it
//! lets the existing renderer draw diagrams written in the textual notation.

use crate::{
    authority::Route,
    diagram::{Diagram, Event},
    level::Level,
};
use alloc::{string::String, vec::Vec};
use serde::Serialize;

impl Diagram {
    /// Serialize to the [Keyhive Lab Notes'](https://www.inkandswitch.com/keyhive/notebook/08/) rain diagram JSON.
    ///
    /// ```
    /// let diagram = rain_diagram_core::parse("rain\n(Doc)-[edit]->(Alice)")?;
    /// assert_eq!(
    ///     diagram.to_site_json()?,
    ///     r#"{"agents":[{"id":"doc","label":"Doc"},{"id":"alice","label":"Alice"}],"events":[{"id":"doc-alice","from":"doc","to":"alice","level":"edit"}]}"#
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Only if serialization itself fails, which it shouldn't for these types.
    pub fn to_site_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.site())
    }

    fn site(&self) -> Site<'_> {
        Site {
            agents: self
                .principals()
                .iter()
                .map(|p| Agent {
                    id: p.key(),
                    label: p.name().as_str(),
                })
                .collect(),
            events: self
                .events()
                .iter()
                .map(|event| match &event.node {
                    Event::Delegate(cert) => SiteEvent::Delegate {
                        id: cert.id.as_str(),
                        from: self.key(cert.issuer),
                        to: self.key(cert.audience),
                        level: cert.level,
                        subject: (cert.subject != cert.issuer).then(|| self.key(cert.subject)),
                        transitive: cert
                            .authority
                            .route()
                            .map(Route::through)
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|&ix| self.cert(ix).map(|c| c.id.as_str()))
                            .collect(),
                    },
                    Event::Invoke(invoke) => SiteEvent::Invoke {
                        invoke: invoke.level,
                        by: self.key(invoke.by),
                        on: self.key(invoke.on),
                    },
                    Event::Revoke(revoke) => SiteEvent::Revoke {
                        revoke: self.cert(revoke.target).map_or("", |c| c.id.as_str()),
                        by: self.key(revoke.by),
                    },
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct Site<'a> {
    agents: Vec<Agent<'a>>,
    events: Vec<SiteEvent<'a>>,
}

#[derive(Serialize)]
struct Agent<'a> {
    id: &'a str,
    label: &'a str,
}

#[derive(Serialize)]
#[serde(untagged)]
enum SiteEvent<'a> {
    Delegate {
        id: &'a str,
        from: &'a str,
        to: &'a str,
        level: Level,
        #[serde(skip_serializing_if = "Option::is_none")]
        subject: Option<&'a str>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        transitive: Vec<&'a str>,
    },
    Invoke {
        invoke: Level,
        by: &'a str,
        on: &'a str,
    },
    Revoke {
        revoke: &'a str,
        by: &'a str,
    },
}
