//! Parser and authority model for _certflow_ and _rain_ diagrams.
//!
//! Certflow and rain diagrams show how authority flows through a certificate
//! capability system such as [Keyhive]. Certflow draws a snapshot: principals,
//! the certificates between them, and dashed _powerlines_ tracing where each
//! certificate's authority comes from. Rain diagrams add time: each principal
//! gets a lifeline, certificates become rows, and authority drips down the
//! page.
//!
//! Both share one plain-text statement language, so the same source can be
//! rendered either way. The header picks the default:
//!
//! ```text
//! rain                                    // or `certflow`
//! title Revoking two hops downstream
//!
//! (Doc)-[edit]->(Alice)                   : The Doc grants Alice Edit
//! (Alice)-[edit(Doc)]->(Bob)              : Alice passes Edit to Bob
//! (Bob)=[[edit]]=>(Doc)                   : Bob edits the Doc
//! (Bob)-[read]->(Carol)                   : Bob gives Carol Read
//! cd: (Carol)-[read]->(Dan)               : Carol shares Read with Dan
//! (Alice)-<revoke>-x[cd]                  : Alice revokes Carol→Dan
//! ```
//!
//! | Form                              | Meaning                                                    |
//! |-----------------------------------|------------------------------------------------------------|
//! | `(A)-[edit(S)]->(B)`              | A delegates Edit over S to B                               |
//! | `(A)-[edit]->(B)`                 | Same, with S inferred (`admin`: A itself; else what A has) |
//! | `(A)-[edit(S) via ta]->(B)`       | Route A's authority through certificate `ta`               |
//! | `label: (A)-[…]->(B)`             | Label a certificate, so revocations and `via` can name it  |
//! | `(B)=[[edit]]=>(S)`               | B uses its Edit authority on S                             |
//! | `(A)-<revoke>-x[label]`           | A revokes the certificate `label`                          |
//! | `(A)---[edit]->(B)`               | More dashes before the label slide it towards B (here ¾)   |
//! | `>>> A (P): note` … `<<< A`       | A captioned region, optionally in principal P's colour        |
//! | `principals: A, B`                | Fix column order (`participant A, B` works too)            |
//! | `title …`, `: note`, `// comment` | Annotations (`%%` comments work too)                      |
//!
//! Levels are `admin`, `edit`, `read`, and `relay` (or 👑 ✏️ 👀 📡).
//!
//! # Pipeline
//!
//! ```text
//! source ──parser::document──▶ ast::Document ──Diagram::resolve──▶ diagram::Diagram
//!                                                  (names, then authority analysis)
//!                                                                          │
//!                         ┌────────────────────────────┬───────────────────┤
//!                         ▼                            ▼                   ▼
//!                 Diagram::to_text             Diagram::to_svg   Diagram::to_site_json
//! ```
//!
//! [`parse`] runs both steps.
//!
//! ```
//! let diagram = rain_diagram_core::parse(
//!     "rain
//!      (Doc)-[edit]->(Alice)
//!      (Alice)=[[edit]]=>(Doc)",
//! )?;
//!
//! assert_eq!(diagram.principals().len(), 2);
//! assert_eq!(diagram.events().len(), 2);
//! # Ok::<(), rain_diagram_core::Error>(())
//! ```
//!
//! [Keyhive]: https://github.com/inkandswitch/keyhive

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

pub mod ability;
pub mod ast;
pub mod authority;
pub mod diagram;
pub mod edit;
pub mod level;
pub mod parser;

#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub mod site_json;

pub mod svg;
pub mod text;

use ast::Span;
use diagram::{Diagram, ResolveError};
use parser::ParseError;
use thiserror::Error;

/// Parse and resolve source text.
///
/// # Errors
///
/// Returns [`Error::Parse`] for malformed text, and [`Error::Resolve`] for
/// well-formed text that refers to things that don't exist.
pub fn parse(source: &str) -> Result<Diagram, Error> {
    Ok(Diagram::resolve(&parser::document(source)?)?)
}

/// Anything that can go wrong in [`parse`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
    /// Malformed source text.
    #[error(transparent)]
    Parse(#[from] ParseError),

    /// Well-formed text that doesn't resolve.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
}

impl Error {
    /// Where in the source the problem is.
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Parse(e) => e.span(),
            Self::Resolve(e) => e.span(),
        }
    }
}
