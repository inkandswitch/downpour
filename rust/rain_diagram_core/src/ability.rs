//! What a certificate grants and an invocation uses: a Keyhive access
//! [`Level`], or a command path like UCAN's `/crud/read`.
//!
//! Keyhive's levels form a strict hierarchy, but command paths only form a
//! tree, and two abilities from different branches (or of different kinds)
//! aren't comparable at all. So rather than a total order, abilities say
//! whether one [`covers`](Ability::covers) another, and delegation keeps the
//! [`narrower`](Ability::narrower) of what's claimed and what's held.
//!
//! ```
//! use rain_diagram_core::{ability::Ability, level::Level};
//!
//! let crud: Ability = "/crud".parse()?;
//! let read: Ability = "/crud/read".parse()?;
//! assert!(crud.covers(&read) && !read.covers(&crud));
//! assert_eq!(crud.narrower(&read), Some(read.clone()));
//! assert_eq!(read.narrower(&"/crud/write".parse()?), None);
//!
//! let edit = Ability::from(Level::Edit);
//! assert_eq!(edit.narrower(&Level::Read.into()), Some(Level::Read.into()));
//! assert!(!edit.covers(&read));
//! # Ok::<(), rain_diagram_core::ability::InvalidAbility>(())
//! ```

use crate::level::{Level, UnknownLevel};
use alloc::string::String;
use core::{fmt, str::FromStr};
use thiserror::Error;

/// A Keyhive [`Level`], or a [`Command`] path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Ability {
    /// A Keyhive access level.
    Level(Level),

    /// A command path.
    Command(Command),
}

impl Ability {
    /// Whether holding `self` is enough for `other`: a level covers itself
    /// and the levels below it, a path covers itself and the paths under it,
    /// and a level and a path never cover each other.
    #[must_use]
    pub fn covers(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Level(held), Self::Level(wanted)) => held >= wanted,
            (Self::Command(held), Self::Command(wanted)) => held.covers(wanted),
            (Self::Level(_), Self::Command(_)) | (Self::Command(_), Self::Level(_)) => false,
        }
    }

    /// The narrower of the two, if one covers the other: what flows when a
    /// certificate claiming one is backed by authority over the other.
    #[must_use]
    pub fn narrower(&self, other: &Self) -> Option<Self> {
        if self.covers(other) {
            Some(other.clone())
        } else if other.covers(self) {
            Some(self.clone())
        } else {
            None
        }
    }

    /// The label drawn in diagrams: `✏️ Edit`, or the path itself.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Level(level) => alloc::format!("{} {}", level.emoji(), level.name()),
            Self::Command(command) => command.as_str().into(),
        }
    }

    /// The Keyhive level, if this is one.
    #[must_use]
    pub const fn level(&self) -> Option<Level> {
        match self {
            Self::Level(level) => Some(*level),
            Self::Command(_) => None,
        }
    }
}

impl From<Level> for Ability {
    fn from(level: Level) -> Self {
        Self::Level(level)
    }
}

impl From<Command> for Ability {
    fn from(command: Command) -> Self {
        Self::Command(command)
    }
}

impl fmt::Display for Ability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Level(level) => level.fmt(f),
            Self::Command(command) => command.fmt(f),
        }
    }
}

/// A level keyword or emoji, or a command path starting with `/`.
impl FromStr for Ability {
    type Err = InvalidAbility;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('/') {
            Ok(Self::Command(Command::new(s)?))
        } else {
            Ok(Self::Level(s.parse()?))
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Ability {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A command path: `/` alone, or `/`-separated segments of letters, digits,
/// `-`, `_`, and `.` (`/crud/read`).
///
/// ```
/// use rain_diagram_core::ability::Command;
///
/// let msg = Command::new("/msg")?;
/// assert!(msg.covers(&Command::new("/msg/send")?));
/// assert!(!msg.covers(&Command::new("/msgs")?));
/// assert!(Command::new("/msg/").is_err());
/// # Ok::<(), rain_diagram_core::ability::InvalidCommand>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Command(String);

impl Command {
    /// Check and wrap a command path.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCommand`] if `path` doesn't start with `/`, has an
    /// empty segment (`//`, or a trailing `/`), or uses other characters.
    pub fn new(path: &str) -> Result<Self, InvalidCommand> {
        let valid = path == "/"
            || path
                .strip_prefix('/')
                .is_some_and(|rest| rest.split('/').all(Self::is_segment));
        if valid {
            Ok(Self(path.into()))
        } else {
            Err(InvalidCommand(path.into()))
        }
    }

    /// The path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// How many segments: 0 for `/`.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.split('/').filter(|s| !s.is_empty()).count()
    }

    /// Whether `other` is this path or under it.
    #[must_use]
    pub fn covers(&self, other: &Self) -> bool {
        self.0 == "/"
            || other
                .0
                .strip_prefix(self.0.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }

    /// Whether `c` can appear in a command path.
    #[must_use]
    pub fn is_path_char(c: char) -> bool {
        c == '/' || Self::is_segment_char(c)
    }

    fn is_segment_char(c: char) -> bool {
        c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
    }

    fn is_segment(s: &str) -> bool {
        !s.is_empty() && s.chars().all(Self::is_segment_char)
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Neither a level nor a command path.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InvalidAbility {
    /// Not a level keyword or emoji.
    #[error(transparent)]
    Level(#[from] UnknownLevel),

    /// Starts with `/` but isn't a well-formed path.
    #[error(transparent)]
    Command(#[from] InvalidCommand),
}

/// A malformed command path.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "malformed command {0:?}; expected `/` or segments like `/crud/read` (letters, digits, `-`, `_`, `.`)"
)]
pub struct InvalidCommand(String);

#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for Command {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        // A small vocabulary, so generated paths often share prefixes
        const SEGMENTS: &[&str] = &["crud", "read", "msg", "send", "a-b", "é"];
        let depth = u.int_in_range(0..=3)?;
        let mut path = String::new();
        for _ in 0..depth {
            path.push('/');
            path.push_str(u.choose(SEGMENTS)?);
        }
        Ok(Self(if path.is_empty() { "/".into() } else { path }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn ability(s: &str) -> Ability {
        s.parse().expect("valid ability")
    }

    #[test]
    fn paths_cover_by_segment_not_by_prefix() {
        let crud = Command::new("/crud").expect("valid");
        assert!(crud.covers(&Command::new("/crud").expect("valid")));
        assert!(crud.covers(&Command::new("/crud/read").expect("valid")));
        assert!(!crud.covers(&Command::new("/crudely").expect("valid")));
        assert!(Command::new("/").expect("valid").covers(&crud));
    }

    #[test]
    fn malformed_paths_are_rejected() {
        for bad in ["", "crud", "//", "/crud/", "/a//b", "/a b", "/a]"] {
            assert!(Command::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn levels_and_paths_are_incomparable() {
        let (edit, root) = (ability("edit"), ability("/"));
        assert!(!edit.covers(&root) && !root.covers(&edit));
        assert_eq!(edit.narrower(&root), None);
    }

    #[test]
    fn labels_are_what_diagrams_draw() {
        assert_eq!(ability("edit").label(), "✏️ Edit");
        assert_eq!(ability("/crud/read").label(), "/crud/read");
        assert_eq!(Command::new("/msg").expect("valid").as_str(), "/msg");
    }

    #[test]
    fn round_trips_through_display() {
        for s in ["admin", "relay", "/", "/crud/read"] {
            assert_eq!(ability(s).to_string(), s);
        }
    }

    #[cfg(feature = "bolero")]
    #[test]
    fn covering_is_a_partial_order_and_narrower_is_its_meet() {
        bolero::check!()
            .with_arbitrary::<(Ability, Ability, Ability)>()
            .for_each(|(a, b, c)| {
                assert!(a.covers(a));
                if a.covers(b) && b.covers(a) {
                    assert_eq!(a, b);
                }
                if a.covers(b) && b.covers(c) {
                    assert!(a.covers(c));
                }
                assert_eq!(a.narrower(b), b.narrower(a));
                if let Some(meet) = a.narrower(b) {
                    assert!(a.covers(&meet) && b.covers(&meet));
                }
                let round: Vec<Ability> = [a, b, c]
                    .iter()
                    .map(|x| x.to_string().parse().expect("displayed ability parses"))
                    .collect();
                assert_eq!(round, [a.clone(), b.clone(), c.clone()]);
            });
    }
}
