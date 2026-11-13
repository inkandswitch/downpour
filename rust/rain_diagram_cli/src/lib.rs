//! The shared logic behind two binaries:
//!
//! - `rain-diagram`: render diagrams to text art, SVG, or JSON, and check
//!   them for errors and warnings (see [`render`]).
//! - `mdbook-rain-diagram`: an [mdBook] preprocessor that turns ` ```rain `
//!   and ` ```certflow ` code blocks into diagrams (see [`mdbook`]).
//!
//! Both are thin wrappers, so everything here works on strings and can be
//! tested without spawning processes.
//!
//! [mdBook]: https://rust-lang.github.io/mdBook/

pub mod mdbook;
pub mod render;
