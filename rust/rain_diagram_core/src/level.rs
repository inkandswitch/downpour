//! Access levels a certificate can grant.

use alloc::string::String;
use core::{fmt, str::FromStr};
use thiserror::Error;

/// The access a certificate grants over its subject.
///
/// Levels are totally ordered, and each implies every level below it
/// (`Admin > Edit > Read > Relay`). Delegations connect like pipes, so the
/// level that actually reaches the end of a path is the [`Ord::min`] of its
/// segments.
///
/// ```
/// use rain_diagram_core::level::Level;
///
/// let path = [Level::Admin, Level::Read, Level::Admin];
/// assert_eq!(path.into_iter().min(), Some(Level::Read));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(rename_all = "lowercase")
)]
pub enum Level {
    /// 📡 Fetch and store ciphertext. Sync servers live here.
    Relay,

    /// 👀 Hold the key and decrypt.
    Read,

    /// ✏️ Add signed changes.
    Edit,

    /// 👑 Full control.
    Admin,
}

impl Level {
    /// Every level, weakest first.
    pub const ALL: [Self; 4] = [Self::Relay, Self::Read, Self::Edit, Self::Admin];

    /// The keyword used in the textual notation.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Relay => "relay",
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Admin => "admin",
        }
    }

    /// The emoji used in rendered diagrams.
    #[must_use]
    pub const fn emoji(self) -> &'static str {
        match self {
            Self::Relay => "📡",
            Self::Read => "👀",
            Self::Edit => "✏️",
            Self::Admin => "👑",
        }
    }

    /// The capitalised name, as in diagram labels: `Admin`, `Edit`, …
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Relay => "Relay",
            Self::Read => "Read",
            Self::Edit => "Edit",
            Self::Admin => "Admin",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.keyword())
    }
}

/// Accepts the keyword (case-insensitively) or the emoji.
impl FromStr for Level {
    type Err = UnknownLevel;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|level| {
                s.eq_ignore_ascii_case(level.keyword())
                    || s.trim_end_matches('\u{fe0f}') == level.emoji().trim_end_matches('\u{fe0f}')
            })
            .ok_or_else(|| UnknownLevel(s.into()))
    }
}

/// A level that isn't one of `admin`, `edit`, `read`, or `relay`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown access level {0:?}; expected admin, edit, read, or relay")]
pub struct UnknownLevel(String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keywords_and_emoji() {
        for level in Level::ALL {
            assert_eq!(level.keyword().parse(), Ok(level));
            assert_eq!(level.emoji().parse(), Ok(level));
        }

        assert_eq!("EDIT".parse(), Ok(Level::Edit));
        assert_eq!("✏".parse(), Ok(Level::Edit));
    }

    #[test]
    fn rejects_unknown() {
        assert!("write".parse::<Level>().is_err());
    }
}
