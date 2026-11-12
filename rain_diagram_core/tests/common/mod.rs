//! Shared test helpers.

use arbitrary::{Arbitrary, Unstructured};

/// Decode a `T` from fuzzer bytes.
///
/// `bolero`'s `with_arbitrary` silently rejects every input for types
/// containing collections (`Vec`, and so `Document`), so property tests
/// would pass without running. Taking bytes and decoding them here always
/// produces a value.
pub(crate) fn arbitrary<T: for<'a> Arbitrary<'a>>(bytes: &[u8]) -> Option<T> {
    T::arbitrary_take_rest(Unstructured::new(bytes)).ok()
}
