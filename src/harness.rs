//! Measurement harness (F1): intrinsic metrics over a continuous field.
//!
//! This is the discipline that lets us *make claims* instead of eyeballing.
//! Everything here reduces a raw `&[f32]` field of dimensions `w × h`
//! (row-major, toroidal) to numbers — so the same code measures matter, energy,
//! or detritus channels, on CPU now and GPU readback later.
//!
//! What it computes:
//! - **Field stats** — total mass, occupied fraction, spatial (Shannon) entropy
//!   and a derived localization/concentration score, peak density, variance.
//! - **Connected components** — threshold the field and label blobs with
//!   toroidal 8-connectivity; report per-blob cell count, mass, and centroid.
//!   The continuous analog of "how many organisms, and how big."
//! - **Temporal metrics** — field activity (per-step L1 change), and a `Tracker`
//!   that matches blobs across frames to recover a velocity distribution
//!   (center-of-mass drift — a plain observable, no movement black box).
//! - **`RunSummary`** — folds a whole run into a handful of behavior descriptors
//!   suitable as axes for the F2 outer-loop (MAP-Elites) search.
//!
//! Deferred: Bedau–Packard evolutionary activity statistics need a heritable
//! component to track, which arrives with parameter localization (M-γ-1). They
//! belong here once there is a genotype to count.

use crate::flow_lenia::World;

/// Scalar reductions of a single field snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldStats {
    /// Sum of the field over all cells.
    pub total: f64,
    /// Fraction of cells above the occupancy threshold.
    pub occupied_fraction: f32,
    /// Shannon entropy of the normalized field, in bits.
    pub entropy_bits: f32,
    /// Localization score in `[0, 1]`: `1 − H / log2(N)`. A point mass → 1,
    /// a uniform field → 0. High = matter is concentrated into structure.
    pub concentration: f32,
    /// Largest single-cell value.
    pub peak: f32,
    /// Variance of the field across cells.
    pub variance: f32,
}

/// Sum of a field.
pub fn total(field: &[f32]) -> f64 {
    field.iter().map(|&v| v as f64).sum()
}

/// Fraction of cells strictly above `threshold`.
pub fn occupied_fraction(field: &[f32], threshold: f32) -> f32 {
    if field.is_empty() {
        return 0.0;
    }
    let count = field.iter().filter(|&&v| v > threshold).count();
    count as f32 / field.len() as f32
}

/// Shannon entropy (bits) of the field treated as an unnormalized distribution.
/// Empty or zero-mass fields have entropy 0.
pub fn entropy_bits(field: &[f32]) -> f32 {
    let sum: f64 = field.iter().map(|&v| v.max(0.0) as f64).sum();
    if sum <= 0.0 {
        return 0.0;
    }
    let mut h = 0.0f64;
    for &v in field {
        let v = v.max(0.0) as f64;
        if v > 0.0 {
            let p = v / sum;
            h -= p * p.log2();
        }
    }
    h as f32
}

/// Localization score in `[0, 1]`: `1 − H / H_max`, with `H_max = log2(N)`.
pub fn concentration(field: &[f32]) -> f32 {
    if field.len() < 2 {
        return 0.0;
    }
    // An empty (zero-mass) field has no structure — entropy is 0 there only for
    // want of any distribution, so report 0 concentration rather than a spurious 1.
    let sum: f64 = field.iter().map(|&v| v.max(0.0) as f64).sum();
    if sum <= 0.0 {
        return 0.0;
    }
    let h = entropy_bits(field);
    let h_max = (field.len() as f32).log2();
    if h_max <= 0.0 {
        0.0
    } else {
        (1.0 - h / h_max).clamp(0.0, 1.0)
    }
}

/// Compute all scalar field stats in a couple of passes.
pub fn field_stats(field: &[f32], threshold: f32) -> FieldStats {
    let n = field.len();
    if n == 0 {
        return FieldStats {
            total: 0.0,
            occupied_fraction: 0.0,
            entropy_bits: 0.0,
            concentration: 0.0,
            peak: 0.0,
            variance: 0.0,
        };
    }
    let mut sum = 0.0f64;
    let mut peak = 0.0f32;
    let mut occupied = 0usize;
    for &v in field {
        sum += v as f64;
        if v > peak {
            peak = v;
        }
        if v > threshold {
            occupied += 1;
        }
    }
    let mean = (sum / n as f64) as f32;
    let mut var = 0.0f64;
    for &v in field {
        let d = (v - mean) as f64;
        var += d * d;
    }
    FieldStats {
        total: sum,
        occupied_fraction: occupied as f32 / n as f32,
        entropy_bits: entropy_bits(field),
        concentration: concentration(field),
        peak,
        variance: (var / n as f64) as f32,
    }
}

/// One connected blob of above-threshold matter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blob {
    /// Number of occupied cells.
    pub cells: usize,
    /// Total field mass over those cells.
    pub mass: f32,
    /// Mass-weighted centroid (toroidal circular mean), in cell coordinates.
    pub cx: f32,
    pub cy: f32,
}

/// Result of thresholded connected-component labeling.
#[derive(Clone, Debug, Default)]
pub struct Components {
    pub blobs: Vec<Blob>,
}

impl Components {
    pub fn count(&self) -> usize {
        self.blobs.len()
    }

    /// Fraction of total blob mass held by the single largest blob (0 if none).
    /// Near 1 = one dominant structure; low = mass split across many blobs.
    pub fn largest_mass_fraction(&self) -> f32 {
        let total: f32 = self.blobs.iter().map(|b| b.mass).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let max = self.blobs.iter().map(|b| b.mass).fold(0.0f32, f32::max);
        max / total
    }

    /// Mean blob size in cells (0 if none).
    pub fn mean_size(&self) -> f32 {
        if self.blobs.is_empty() {
            return 0.0;
        }
        self.blobs.iter().map(|b| b.cells).sum::<usize>() as f32 / self.blobs.len() as f32
    }
}

/// Label above-threshold cells into connected blobs using toroidal
/// 8-connectivity (union-find), then reduce each blob to cell count, mass, and
/// a toroidal circular-mean centroid.
pub fn connected_components(field: &[f32], w: usize, h: usize, threshold: f32) -> Components {
    let n = w * h;
    debug_assert_eq!(field.len(), n);
    if n == 0 {
        return Components::default();
    }
    let occupied: Vec<bool> = field.iter().map(|&v| v > threshold).collect();

    // Union-find over occupied cells.
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // Link each occupied cell to its forward (E, S, SE, SW) occupied neighbors.
    // Toroidal wrap. Forward-only set still covers full 8-connectivity because
    // the reverse links are made when the neighbor is the current cell.
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !occupied[i] {
                continue;
            }
            let xp = if x + 1 == w { 0 } else { x + 1 };
            let xm = if x == 0 { w - 1 } else { x - 1 };
            let yp = if y + 1 == h { 0 } else { y + 1 };
            for &(nx, ny) in &[(xp, y), (x, yp), (xp, yp), (xm, yp)] {
                let j = ny * w + nx;
                if occupied[j] {
                    union(&mut parent, i, j);
                }
            }
        }
    }

    // Accumulate per-root reductions with toroidal circular-mean centroids.
    use std::collections::HashMap;
    struct Acc {
        cells: usize,
        mass: f64,
        xc: f64,
        xs: f64,
        yc: f64,
        ys: f64,
    }
    let tau = std::f64::consts::TAU;
    let mut groups: HashMap<usize, Acc> = HashMap::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !occupied[i] {
                continue;
            }
            let root = find(&mut parent, i);
            let m = field[i].max(0.0) as f64;
            let ax = tau * x as f64 / w as f64;
            let ay = tau * y as f64 / h as f64;
            let acc = groups.entry(root).or_insert(Acc {
                cells: 0,
                mass: 0.0,
                xc: 0.0,
                xs: 0.0,
                yc: 0.0,
                ys: 0.0,
            });
            acc.cells += 1;
            acc.mass += m;
            acc.xc += m * ax.cos();
            acc.xs += m * ax.sin();
            acc.yc += m * ay.cos();
            acc.ys += m * ay.sin();
        }
    }

    let mut blobs: Vec<Blob> = groups
        .into_values()
        .map(|a| {
            let cx = a.xs.atan2(a.xc).rem_euclid(tau) / tau * w as f64;
            let cy = a.ys.atan2(a.yc).rem_euclid(tau) / tau * h as f64;
            Blob {
                cells: a.cells,
                mass: a.mass as f32,
                cx: cx as f32,
                cy: cy as f32,
            }
        })
        .collect();
    // Deterministic order: largest mass first.
    blobs.sort_by(|a, b| b.mass.partial_cmp(&a.mass).unwrap_or(std::cmp::Ordering::Equal));
    Components { blobs }
}

/// Mean absolute per-cell change between two field snapshots — the activity /
/// "dynamism" of the substrate. ~0 = converged/static; large = churning.
pub fn activity(prev: &[f32], curr: &[f32]) -> f32 {
    if prev.len() != curr.len() || prev.is_empty() {
        return 0.0;
    }
    let s: f64 = prev
        .iter()
        .zip(curr)
        .map(|(&a, &b)| (a - b).abs() as f64)
        .sum();
    (s / prev.len() as f64) as f32
}

/// Per-frame velocity statistics recovered by matching blobs across frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VelocityStats {
    /// Mass-weighted mean speed over matched blobs (cells per observed interval).
    pub mean_speed: f32,
    /// Maximum matched blob speed.
    pub max_speed: f32,
    /// Number of blobs matched to a previous blob.
    pub matched: usize,
}

/// Tracks blobs across frames to recover a velocity distribution. Greedy
/// nearest-centroid matching under toroidal distance, gated by `max_match_dist`
/// so a vanished blob is not spuriously matched to a distant new one.
pub struct Tracker {
    w: f32,
    h: f32,
    max_match_dist: f32,
    prev: Vec<Blob>,
}

impl Tracker {
    pub fn new(w: usize, h: usize, max_match_dist: f32) -> Self {
        Tracker {
            w: w as f32,
            h: h as f32,
            max_match_dist,
            prev: Vec::new(),
        }
    }

    /// Toroidal distance between two centroids.
    fn dist(&self, a: &Blob, b: &Blob) -> f32 {
        let dx = wrap_delta(a.cx - b.cx, self.w);
        let dy = wrap_delta(a.cy - b.cy, self.h);
        (dx * dx + dy * dy).sqrt()
    }

    /// Observe a new frame's components and return velocity stats vs. the
    /// previous frame. The first call establishes a baseline (0 matches).
    pub fn observe(&mut self, comps: &Components) -> VelocityStats {
        let mut stats = VelocityStats::default();
        if !self.prev.is_empty() {
            let mut sum_w = 0.0f64;
            let mut sum_ws = 0.0f64;
            for cur in &comps.blobs {
                // Nearest previous blob within gate.
                let mut best = f32::INFINITY;
                for p in &self.prev {
                    let d = self.dist(cur, p);
                    if d < best {
                        best = d;
                    }
                }
                if best.is_finite() && best <= self.max_match_dist {
                    stats.matched += 1;
                    stats.max_speed = stats.max_speed.max(best);
                    sum_w += cur.mass as f64;
                    sum_ws += cur.mass as f64 * best as f64;
                }
            }
            if sum_w > 0.0 {
                stats.mean_speed = (sum_ws / sum_w) as f32;
            }
        }
        self.prev = comps.blobs.clone();
        stats
    }
}

#[inline]
fn wrap_delta(mut d: f32, size: f32) -> f32 {
    if d > size * 0.5 {
        d -= size;
    } else if d < -size * 0.5 {
        d += size;
    }
    d
}

/// A behavior fingerprint of a whole run — the axes an outer-loop search (F2)
/// can illuminate. Every field here is intrinsic (measured, not designed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunSummary {
    pub steps: usize,
    /// Relative mass drift over the run (conservation check).
    pub mass_drift: f64,
    /// Time-averaged localization score (matter organized into structure).
    pub mean_concentration: f32,
    /// Time-averaged blob count.
    pub mean_components: f32,
    /// Blob count at the final step (survival of structure).
    pub final_components: usize,
    /// Time-averaged per-step activity (dynamism).
    pub mean_activity: f32,
    /// Activity of the final step (settled vs. still churning).
    pub final_activity: f32,
    /// Mean of per-frame mean blob speed (motility).
    pub mean_speed: f32,
    /// Largest matched blob speed seen anywhere in the run.
    pub peak_speed: f32,
}

/// Drives a `World` forward `steps` steps, sampling metrics every `sample_every`
/// steps, and folds the run into a `RunSummary`. `threshold` sets what counts as
/// occupied matter for occupancy and blob detection; `max_match_dist` gates blob
/// matching for velocity. Returns the summary and the per-sample time series.
pub fn measure_run(
    world: &mut World,
    steps: usize,
    sample_every: usize,
    threshold: f32,
    max_match_dist: f32,
) -> (RunSummary, Vec<Sample>) {
    let sample_every = sample_every.max(1);
    let initial_mass = world.total_mass();
    let mut tracker = Tracker::new(world.width(), world.height(), max_match_dist);

    let mut samples: Vec<Sample> = Vec::new();
    let mut prev_field: Option<Vec<f32>> = None;

    let mut sum_conc = 0.0f64;
    let mut sum_comp = 0.0f64;
    let mut sum_act = 0.0f64;
    let mut sum_speed = 0.0f64;
    let mut peak_speed = 0.0f32;
    let mut n_samples = 0.0f64;
    let mut last = Sample::default();

    // Include step 0 and then every sample_every steps.
    for step in 0..=steps {
        if step > 0 {
            world.step();
        }
        if step % sample_every != 0 && step != steps {
            continue;
        }
        let field = world.mass_field();
        let stats = field_stats(&field, threshold);
        let comps = connected_components(&field, world.width(), world.height(), threshold);
        let vel = tracker.observe(&comps);
        let act = match &prev_field {
            Some(p) => activity(p, &field),
            None => 0.0,
        };
        prev_field = Some(field);

        sum_conc += stats.concentration as f64;
        sum_comp += comps.count() as f64;
        sum_act += act as f64;
        sum_speed += vel.mean_speed as f64;
        peak_speed = peak_speed.max(vel.max_speed);
        n_samples += 1.0;

        last = Sample {
            step,
            stats,
            components: comps.count(),
            largest_mass_fraction: comps.largest_mass_fraction(),
            activity: act,
            velocity: vel,
        };
        samples.push(last.clone());
    }

    let denom = n_samples.max(1.0);
    let summary = RunSummary {
        steps,
        mass_drift: if initial_mass != 0.0 {
            (world.total_mass() - initial_mass).abs() / initial_mass
        } else {
            0.0
        },
        mean_concentration: (sum_conc / denom) as f32,
        mean_components: (sum_comp / denom) as f32,
        final_components: last.components,
        mean_activity: (sum_act / denom) as f32,
        final_activity: last.activity,
        mean_speed: (sum_speed / denom) as f32,
        peak_speed,
    };
    (summary, samples)
}

/// One sampled snapshot of a run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub step: usize,
    pub stats: FieldStats,
    pub components: usize,
    pub largest_mass_fraction: f32,
    pub activity: f32,
    pub velocity: VelocityStats,
}

impl Default for FieldStats {
    fn default() -> Self {
        FieldStats {
            total: 0.0,
            occupied_fraction: 0.0,
            entropy_bits: 0.0,
            concentration: 0.0,
            peak: 0.0,
            variance: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow_lenia::{FlowLeniaParams, World};

    #[test]
    fn entropy_of_point_mass_is_zero() {
        let mut f = vec![0.0f32; 64];
        f[10] = 1.0;
        assert!(entropy_bits(&f) < 1e-6);
        assert!((concentration(&f) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn entropy_of_uniform_is_maximal() {
        let f = vec![0.5f32; 64];
        let h = entropy_bits(&f);
        assert!((h - 6.0).abs() < 1e-4, "log2(64)=6, got {h}");
        assert!(concentration(&f) < 1e-4);
    }

    #[test]
    fn empty_field_is_well_defined() {
        let f = vec![0.0f32; 32];
        assert_eq!(entropy_bits(&f), 0.0);
        assert_eq!(concentration(&f), 0.0);
        assert_eq!(occupied_fraction(&f, 0.1), 0.0);
        let s = field_stats(&f, 0.1);
        assert_eq!(s.total, 0.0);
        assert_eq!(s.peak, 0.0);
    }

    #[test]
    fn single_blob_is_one_component() {
        let (w, h) = (16, 16);
        let mut f = vec![0.0f32; w * h];
        for y in 6..10 {
            for x in 6..10 {
                f[y * w + x] = 1.0;
            }
        }
        let c = connected_components(&f, w, h, 0.5);
        assert_eq!(c.count(), 1);
        assert_eq!(c.blobs[0].cells, 16);
        assert!((c.blobs[0].cx - 7.5).abs() < 0.6, "cx={}", c.blobs[0].cx);
        assert!((c.blobs[0].cy - 7.5).abs() < 0.6, "cy={}", c.blobs[0].cy);
        assert!((c.largest_mass_fraction() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn two_separated_blobs_are_two_components() {
        let (w, h) = (32, 32);
        let mut f = vec![0.0f32; w * h];
        // Blob A around (5,5), blob B around (25,25) — well separated.
        for (cx, cy) in [(5usize, 5usize), (25, 25)] {
            for dy in 0..3 {
                for dx in 0..3 {
                    f[(cy + dy) * w + (cx + dx)] = 1.0;
                }
            }
        }
        let c = connected_components(&f, w, h, 0.5);
        assert_eq!(c.count(), 2);
        assert!((c.largest_mass_fraction() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn blobs_touching_across_the_wrap_seam_merge() {
        // Two cells on opposite edges of the same row are neighbors on a torus.
        let (w, h) = (8, 8);
        let mut f = vec![0.0f32; w * h];
        f[3 * w] = 1.0; // (0,3)
        f[3 * w + (w - 1)] = 1.0; // (7,3)
        let c = connected_components(&f, w, h, 0.5);
        assert_eq!(c.count(), 1, "wrap-adjacent cells should be one blob");
    }

    #[test]
    fn activity_zero_for_identical_frames() {
        let f = vec![0.3f32; 40];
        assert_eq!(activity(&f, &f), 0.0);
        let mut g = f.clone();
        g[0] += 1.0;
        assert!((activity(&f, &g) - 1.0 / 40.0).abs() < 1e-6);
    }

    #[test]
    fn tracker_recovers_a_known_translation() {
        let (w, h) = (64, 64);
        let mut tracker = Tracker::new(w, h, 10.0);
        let a = Components {
            blobs: vec![Blob { cells: 4, mass: 4.0, cx: 10.0, cy: 10.0 }],
        };
        let b = Components {
            blobs: vec![Blob { cells: 4, mass: 4.0, cx: 13.0, cy: 14.0 }],
        };
        assert_eq!(tracker.observe(&a).matched, 0); // baseline
        let v = tracker.observe(&b);
        assert_eq!(v.matched, 1);
        assert!((v.mean_speed - 5.0).abs() < 1e-4, "3-4-5 triangle, got {}", v.mean_speed);
    }

    #[test]
    fn measure_run_reports_conservation_and_structure() {
        let mut world = World::new(64, 64, FlowLeniaParams::default());
        world.seed_blob(0, 32.0, 32.0, 6.0, 0.95);
        let (summary, samples) = measure_run(&mut world, 120, 20, 0.05, 8.0);
        assert!(summary.mass_drift < 1e-4, "drift {}", summary.mass_drift);
        assert!(!samples.is_empty());
        // A seeded blob should register as at least one concentrated component.
        assert!(summary.mean_components >= 1.0);
        assert!(summary.mean_concentration > 0.0);
    }
}
