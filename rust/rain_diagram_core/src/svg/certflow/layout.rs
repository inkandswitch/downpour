//! Automatic placement for certflow snapshots.
//!
//! [Keyhive Lab Note #7](https://www.inkandswitch.com/keyhive/notebook/07/) places every node by hand. Here, nodes (principals,
//! certificates, revocations) are placed by _stress majorization_: each
//! pair of nodes is pulled towards a distance proportional to the number of
//! hops between them in the graph. It's deterministic (no random start),
//! copes with cycles (Alice ↔ Commenters) where layered layouts struggle,
//! and needs nothing beyond `core`, so it runs in `no_std` and Wasm.
//!
//! Afterwards, overlapping nodes are pushed apart, and the picture is turned
//! so the first node sits at the top: authority flows down the page.

// Matrix arithmetic over `0..n`, where every index is in bounds by
// construction; `.get()` everywhere would bury the maths
#![allow(clippy::indexing_slicing)]

use alloc::{collections::VecDeque, vec, vec::Vec};

/// A position, in the same units as the radii passed to [`place`].
pub(super) type Point = (f64, f64);

const ITERATIONS: usize = 300;
const SEPARATION_PASSES: usize = 200;

/// Majorization has settled once no node moves more than this (in hops):
/// tight enough that settled layouts come out the same as running every
/// iteration, for the examples checked.
const SETTLED: f64 = 0.001;

/// Place `radii.len()` nodes joined by `edges`, so neighbours sit about
/// `unit` apart and no two nodes come closer than their radii plus `gap`.
pub(super) fn place(radii: &[f64], edges: &[(usize, usize)], unit: f64, gap: f64) -> Vec<Point> {
    let n = radii.len();
    if n == 0 {
        return Vec::new();
    }

    let hops = hops(n, edges);
    let mut points = start(n, &hops);
    let _passes = majorize(&mut points, &hops);

    for p in &mut points {
        *p = (p.0 * unit, p.1 * unit);
    }
    orient(&mut points);
    separate(&mut points, radii, gap);
    points
}

/// All-pairs hop counts. Disconnected pairs are one more than the longest
/// path, so separate components sit near, not on top of, each other.
fn hops(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<f64>> {
    let mut adjacent = vec![Vec::new(); n];
    for &(a, b) in edges {
        if a < n && b < n && a != b {
            adjacent[a].push(b);
            adjacent[b].push(a);
        }
    }

    let mut hops = vec![vec![f64::INFINITY; n]; n];
    for (source, row) in hops.iter_mut().enumerate() {
        row[source] = 0.0;
        let mut queue = VecDeque::from([source]);
        while let Some(at) = queue.pop_front() {
            for &next in &adjacent[at] {
                if row[next].is_infinite() {
                    row[next] = row[at] + 1.0;
                    queue.push_back(next);
                }
            }
        }
    }

    let longest = hops
        .iter()
        .flatten()
        .copied()
        .filter(|h| h.is_finite())
        .fold(1.0, f64::max);
    for h in hops.iter_mut().flatten() {
        if h.is_infinite() {
            *h = longest + 1.0;
        }
    }
    hops
}

/// Layers by distance from the first node, spread sideways within each
/// layer, with a small fixed jitter so nothing starts exactly collinear.
fn start(n: usize, hops: &[Vec<f64>]) -> Vec<Point> {
    let mut seen_in_layer: Vec<usize> = Vec::new();
    (0..n)
        .map(|i| {
            let layer = hops[0][i];
            let slot = layer_index(layer);
            if seen_in_layer.len() <= slot {
                seen_in_layer.resize(slot + 1, 0);
            }
            let x = to_f64(seen_in_layer[slot]);
            seen_in_layer[slot] += 1;
            let jitter = |k: usize, m: usize| to_f64((i * k) % m) / to_f64(m) * 0.3;
            (x + jitter(7919, 13), layer + jitter(104_729, 17))
        })
        .collect()
}

/// Localized stress majorization (Gansner, Koren & North, 2004), with
/// weights `1 / d²` so near neighbours matter most. Returns how many
/// passes it took to settle (at most [`ITERATIONS`]).
fn majorize(points: &mut [Point], hops: &[Vec<f64>]) -> usize {
    let n = points.len();
    for pass in 1..=ITERATIONS {
        // Each pass is O(n²), so stop as soon as the layout has settled
        // rather than always running all of them
        let mut moved: f64 = 0.0;
        for i in 0..n {
            let (mut sx, mut sy, mut total) = (0.0, 0.0, 0.0);
            for j in (0..n).filter(|&j| j != i) {
                let d = hops[i][j];
                let w = 1.0 / (d * d);
                let (dx, dy) = (points[i].0 - points[j].0, points[i].1 - points[j].1);
                let length = sqrt(dx * dx + dy * dy);
                let (ux, uy) = if length > 1e-9 {
                    (dx / length, dy / length)
                } else {
                    (0.0, 0.0)
                };
                sx += w * (points[j].0 + d * ux);
                sy += w * (points[j].1 + d * uy);
                total += w;
            }
            if total > 0.0 {
                let next = (sx / total, sy / total);
                moved = moved
                    .max((next.0 - points[i].0).abs())
                    .max((next.1 - points[i].1).abs());
                points[i] = next;
            }
        }
        if moved < SETTLED {
            return pass;
        }
    }
    ITERATIONS
}

/// Turn the picture so the first node is at the top, then mirror it so the
/// second is on the left. Uses the direction vector directly; no trig.
fn orient(points: &mut [Point]) {
    let n = to_f64(points.len());
    let centre = points
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.0 / n, y + p.1 / n));
    for p in points.iter_mut() {
        *p = (p.0 - centre.0, p.1 - centre.1);
    }

    let (vx, vy) = points[0];
    let length = sqrt(vx * vx + vy * vy);
    if length > 1e-9 {
        // The rotation taking (vx, vy) to (0, -length)
        let (c, s) = (-vy / length, -vx / length);
        for p in points.iter_mut() {
            *p = (c * p.0 - s * p.1, s * p.0 + c * p.1);
        }
    }

    if points.len() > 1 && points[1].0 > points[0].0 {
        for p in points.iter_mut() {
            p.0 = -p.0;
        }
    }
}

/// Push apart any two nodes closer than their radii plus `gap`.
fn separate(points: &mut [Point], radii: &[f64], gap: f64) {
    let n = points.len();
    for _ in 0..SEPARATION_PASSES {
        let mut moved = false;
        for i in 0..n {
            for j in i + 1..n {
                let wanted = radii[i] + radii[j] + gap;
                let (dx, dy) = (points[j].0 - points[i].0, points[j].1 - points[i].1);
                let length = sqrt(dx * dx + dy * dy);
                if length >= wanted {
                    continue;
                }
                // Coincident nodes part sideways, in index order
                let (ux, uy) = if length > 1e-9 {
                    (dx / length, dy / length)
                } else {
                    (1.0, 0.0)
                };
                let push = (wanted - length) / 2.0;
                points[i] = (points[i].0 - ux * push, points[i].1 - uy * push);
                points[j] = (points[j].0 + ux * push, points[j].1 + uy * push);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// Square root by Newton's method, for `no_std` (`f64::sqrt` needs `std`).
pub(super) fn sqrt(x: f64) -> f64 {
    if x <= 0.0 || !x.is_finite() {
        return 0.0;
    }
    // Halving the exponent gets within a few percent; Newton does the rest
    let mut guess = f64::from_bits((x.to_bits() >> 1) + (1023_u64 << 51));
    for _ in 0..6 {
        guess = 0.5 * (guess + x / guess);
    }
    guess
}

#[allow(clippy::cast_precision_loss)] // node counts are tiny
pub(super) const fn to_f64(n: usize) -> f64 {
    n as f64
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // hop counts are small whole numbers
const fn layer_index(layer: f64) -> usize {
    layer as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance(a: Point, b: Point) -> f64 {
        sqrt((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2))
    }

    #[test]
    fn square_roots() {
        for x in [1e-6, 0.25, 2.0, 9.0, 1234.5, 1e12] {
            let r = sqrt(x);
            assert!((r * r - x).abs() <= x * 1e-12, "sqrt({x}) = {r}");
        }
        assert!(sqrt(0.0).abs() < f64::EPSILON);
        assert!(sqrt(-1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_chain_is_spaced_evenly_with_the_first_on_top() {
        let points = place(&[10.0; 3], &[(0, 1), (1, 2)], 100.0, 10.0);
        assert!((distance(points[0], points[1]) - 100.0).abs() < 5.0);
        assert!((distance(points[1], points[2]) - 100.0).abs() < 5.0);
        assert!(points[0].1 < points[1].1 && points[0].1 < points[2].1);
    }

    #[test]
    fn nothing_overlaps_even_when_disconnected() {
        let radii = [30.0, 30.0, 20.0, 20.0, 40.0];
        let points = place(&radii, &[(0, 2), (2, 1)], 60.0, 8.0);
        for i in 0..radii.len() {
            for j in i + 1..radii.len() {
                assert!(distance(points[i], points[j]) >= radii[i] + radii[j] + 8.0 - 1e-6);
            }
        }
    }

    #[test]
    fn deterministic() {
        let edges = [(0, 3), (3, 1), (1, 4), (4, 2), (2, 5), (5, 0)];
        assert_eq!(
            place(&[20.0; 6], &edges, 100.0, 10.0),
            place(&[20.0; 6], &edges, 100.0, 10.0)
        );
    }

    /// The regression guard for layout speed: each pass is O(n²), so a
    /// long chain must settle well before the cap rather than always
    /// running every pass (which made 300 hops take a second).
    #[test]
    fn a_long_chain_settles_early() {
        let n = 120;
        let edges: Vec<(usize, usize)> = (1..n).map(|i| (i - 1, i)).collect();
        let hops = hops(n, &edges);
        let mut points = start(n, &hops);
        let passes = majorize(&mut points, &hops);
        assert!(passes < ITERATIONS / 2, "took {passes} passes");
    }
}
