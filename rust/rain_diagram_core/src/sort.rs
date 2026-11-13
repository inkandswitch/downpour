//! Stable sorting through one shared copy of the standard sort.
//!
//! `slice::sort_by_key` is compiled afresh for every element type and key
//! closure, at several kilobytes each: about 60 KB of Wasm across this
//! crate's handful of call sites. [`by_key`] instead sorts indices with a
//! type-erased comparator, so the standard sort is compiled once, then
//! moves the items into that order. Still stable, and still O(n log n).
//!
//! Collecting into a `BTreeSet` (or `BTreeSet::from`) sorts its input
//! first, with yet another copy of the sort per element type; [`set`]
//! inserts one at a time instead, at the same O(n log n).

use alloc::{collections::BTreeSet, vec::Vec};
use core::cmp::Ordering;

/// Sort `items` stably by `key`, computing each key once.
pub(crate) fn by_key<T, K: Ord>(items: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let keys: Vec<K> = items.iter().map(key).collect();
    let order = order(keys.len(), &|a, b| match (keys.get(a), keys.get(b)) {
        (Some(a), Some(b)) => a.cmp(b),
        _ => Ordering::Equal,
    });
    let mut slots: Vec<Option<T>> = items.drain(..).map(Some).collect();
    items.extend(
        order
            .into_iter()
            .filter_map(|i| slots.get_mut(i).and_then(Option::take)),
    );
}

/// A set of `items`, built by inserting rather than by sorting.
pub(crate) fn set<T: Ord>(items: impl IntoIterator<Item = T>) -> BTreeSet<T> {
    let mut set = BTreeSet::new();
    set.extend(items);
    set
}

/// `0..n`, stably sorted by `cmp`: the one place the standard sort is used.
fn order(n: usize, cmp: &dyn Fn(usize, usize) -> Ordering) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| cmp(a, b));
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same order as the standard stable sort, ties included.
    #[test]
    fn matches_the_standard_stable_sort() {
        bolero::check!()
            .with_type::<Vec<(u8, u8)>>()
            .for_each(|items| {
                let key = |&(k, _): &(u8, u8)| k % 8;
                let mut ours = items.clone();
                by_key(&mut ours, key);
                let mut std = items.clone();
                std.sort_by_key(key);
                assert_eq!(ours, std);
            });
    }
}
