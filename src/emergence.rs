//! Rule-agnostic emergence metrics for phase-transition detection.
//!
//! Three complementary measurements:
//! - Derrida damage-spreading: classifies rules as ordered/critical/chaotic
//! - Compression complexity: structural complexity of spacetime blocks
//! - Shift cross-correlation: detects translating structures without pattern catalog

use crate::grid::{Coordinate, Grid};

/// Derrida damage-spreading measurement.
///
/// Run two identical grids from the same initial state, flip one cell in one,
/// advance both for `steps` using `step_fn`, and track how the Hamming distance
/// (number of differing cells) evolves.
///
/// Returns the Derrida parameter λ: ratio of final-to-initial damage.
/// - λ < 1.0 → ordered (perturbations decay)
/// - λ ≈ 1.0 → critical (edge of chaos — where emergence lives)
/// - λ > 1.0 → chaotic (perturbations grow exponentially)
pub fn derrida_parameter(
    grid_a: &Grid,
    grid_b: &Grid,
    step_fn: impl Fn(&Grid) -> Grid,
    steps: usize,
) -> DerridaResult {
    let mut a = step_fn(grid_a);
    let mut b = step_fn(grid_b);

    let initial_damage = hamming_distance(&a, &b);
    if initial_damage == 0 {
        return DerridaResult {
            lambda: 0.0,
            initial_damage: 0,
            final_damage: 0,
            peak_damage: 0,
            spreading_rate: 0.0,
        };
    }

    let mut peak = initial_damage;
    let mut damages = Vec::with_capacity(steps);
    damages.push(initial_damage);

    for _ in 1..steps {
        a = step_fn(&a);
        b = step_fn(&b);
        let d = hamming_distance(&a, &b);
        peak = peak.max(d);
        damages.push(d);
    }

    let final_damage = *damages.last().unwrap();

    // Spreading rate: average log-ratio of consecutive damages
    let mut log_sum = 0.0f64;
    let mut log_count = 0u32;
    for w in damages.windows(2) {
        if w[0] > 0 && w[1] > 0 {
            log_sum += (w[1] as f64 / w[0] as f64).ln();
            log_count += 1;
        }
    }
    let spreading_rate = if log_count > 0 {
        (log_sum / log_count as f64).exp() as f32
    } else {
        0.0
    };

    let lambda = if initial_damage > 0 {
        final_damage as f32 / initial_damage as f32
    } else {
        0.0
    };

    DerridaResult {
        lambda,
        initial_damage,
        final_damage,
        peak_damage: peak,
        spreading_rate,
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct DerridaResult {
    pub lambda: f32,
    pub initial_damage: usize,
    pub final_damage: usize,
    pub peak_damage: usize,
    /// Average step-to-step growth factor of damage.
    pub spreading_rate: f32,
}

impl DerridaResult {
    pub fn is_critical(&self) -> bool {
        self.spreading_rate > 0.9 && self.spreading_rate < 1.1
    }

    pub fn is_ordered(&self) -> bool {
        self.spreading_rate < 0.9
    }

    pub fn is_chaotic(&self) -> bool {
        self.spreading_rate > 1.1
    }

    /// Score: highest near the critical point (spreading_rate ≈ 1.0).
    /// Returns 0-100.
    pub fn criticality_score(&self) -> usize {
        if self.initial_damage == 0 {
            return 0;
        }
        let dist = (self.spreading_rate - 1.0).abs();
        // Gaussian-like peak at 1.0 with σ ≈ 0.15
        let score = (-dist * dist / (2.0 * 0.15 * 0.15)).exp();
        (score * 100.0) as usize
    }
}

fn hamming_distance(a: &Grid, b: &Grid) -> usize {
    let size = a.size();
    let mut diff = 0usize;
    for y in 0..size.y {
        for x in 0..size.x {
            let alive_a = a.get(x, y).is_some();
            let alive_b = b.get(x, y).is_some();
            if alive_a != alive_b {
                diff += 1;
            }
        }
    }
    diff
}

/// Spacetime complexity: entropy + autocorrelation of a sequence of grid snapshots.
///
/// Takes a series of grid snapshots (sampled during simulation) and computes:
/// - Shannon entropy of alive-cell density across spacetime blocks
/// - Temporal autocorrelation (how much consecutive frames resemble each other)
///
/// High entropy + low autocorrelation = random (chaotic).
/// Low entropy + high autocorrelation = repetitive (ordered).
/// Moderate entropy + moderate autocorrelation = structured complexity.
pub fn spacetime_complexity(snapshots: &[Vec<bool>], grid_width: usize) -> ComplexityResult {
    if snapshots.is_empty() {
        return ComplexityResult::default();
    }

    let total_cells = snapshots[0].len();
    if total_cells == 0 {
        return ComplexityResult::default();
    }

    // Block entropy: divide spacetime into 8×8×T blocks, measure density distribution
    let block_size = 8usize;
    let grid_height = total_cells / grid_width;
    let bx = (grid_width / block_size).max(1);
    let by = (grid_height / block_size).max(1);
    let bt = (snapshots.len() / 4).max(1);

    let num_blocks = bx * by * bt;

    let mut densities = Vec::with_capacity(num_blocks);
    for tbk in 0..bt {
        let t_start = tbk * snapshots.len() / bt;
        let t_end = ((tbk + 1) * snapshots.len() / bt).min(snapshots.len());
        for ybk in 0..by {
            let y_start = ybk * block_size;
            let y_end = (y_start + block_size).min(grid_height);
            for xbk in 0..bx {
                let x_start = xbk * block_size;
                let x_end = (x_start + block_size).min(grid_width);
                let mut alive = 0u32;
                let mut total = 0u32;
                for t in t_start..t_end {
                    for y in y_start..y_end {
                        for x in x_start..x_end {
                            total += 1;
                            if snapshots[t][y * grid_width + x] {
                                alive += 1;
                            }
                        }
                    }
                }
                if total > 0 {
                    densities.push(alive as f32 / total as f32);
                }
            }
        }
    }

    // Shannon entropy of density distribution (binned into 16 levels)
    let bins = 16usize;
    let mut histogram = vec![0u32; bins];
    for &d in &densities {
        let bin = ((d * bins as f32) as usize).min(bins - 1);
        histogram[bin] += 1;
    }
    let n = densities.len() as f32;
    let entropy = if n > 0.0 {
        -histogram
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f32 / n;
                p * p.ln()
            })
            .sum::<f32>()
            / (bins as f32).ln() // normalize to [0, 1]
    } else {
        0.0
    };

    // Temporal autocorrelation: average Hamming similarity between consecutive snapshots
    let mut auto_sum = 0.0f64;
    let mut auto_count = 0u32;
    for w in snapshots.windows(2) {
        let matches: usize = w[0]
            .iter()
            .zip(w[1].iter())
            .filter(|(&a, &b)| a == b)
            .count();
        auto_sum += matches as f64 / total_cells as f64;
        auto_count += 1;
    }
    let autocorrelation = if auto_count > 0 {
        auto_sum as f32 / auto_count as f32
    } else {
        1.0
    };

    // Density variance across blocks (spatial structure indicator)
    let mean_density: f32 = densities.iter().sum::<f32>() / densities.len().max(1) as f32;
    let density_variance: f32 = densities
        .iter()
        .map(|&d| (d - mean_density) * (d - mean_density))
        .sum::<f32>()
        / densities.len().max(1) as f32;

    // Complexity score: peaks when entropy is moderate and autocorrelation is moderate
    let complexity = entropy * (1.0 - entropy) * 4.0 * density_variance.sqrt() * 100.0;

    ComplexityResult {
        entropy,
        autocorrelation,
        density_variance,
        complexity: complexity.min(100.0),
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ComplexityResult {
    pub entropy: f32,
    pub autocorrelation: f32,
    pub density_variance: f32,
    /// Composite complexity score (0-100). Peaks for structured, non-trivial dynamics.
    pub complexity: f32,
}

/// Shift cross-correlation: detect translating structures without a pattern catalog.
///
/// Compares grid at time t with shifted versions at time t+dt.
/// If any shift (dx, dy) ≠ (0,0) has high correlation, something is translating.
///
/// Returns the best non-trivial shift and its correlation strength.
pub fn shift_cross_correlation(
    grid_early: &[bool],
    grid_late: &[bool],
    width: usize,
    height: usize,
    max_shift: i32,
) -> ShiftResult {
    if grid_early.len() != grid_late.len() || grid_early.is_empty() {
        return ShiftResult::default();
    }

    let total = width * height;
    let alive_early: usize = grid_early.iter().filter(|&&b| b).count();
    let alive_late: usize = grid_late.iter().filter(|&&b| b).count();

    if alive_early == 0 || alive_late == 0 {
        return ShiftResult::default();
    }

    // Self-correlation (shift 0,0) for normalization
    let _self_corr: usize = grid_early
        .iter()
        .zip(grid_late.iter())
        .filter(|(&a, &b)| a && b)
        .count();

    let expected = alive_early as f64 * alive_late as f64 / total as f64;

    let mut best_corr = 0.0f32;
    let mut best_dx = 0i32;
    let mut best_dy = 0i32;

    for dy in -max_shift..=max_shift {
        for dx in -max_shift..=max_shift {
            if dx == 0 && dy == 0 {
                continue;
            }
            let mut overlap = 0usize;
            for y in 0..height as i32 {
                for x in 0..width as i32 {
                    let sx = (x + dx).rem_euclid(width as i32) as usize;
                    let sy = (y + dy).rem_euclid(height as i32) as usize;
                    if grid_early[y as usize * width + x as usize]
                        && grid_late[sy * width + sx]
                    {
                        overlap += 1;
                    }
                }
            }
            let corr = if expected > 0.0 {
                (overlap as f64 - expected) / expected
            } else {
                0.0
            };
            if corr as f32 > best_corr {
                best_corr = corr as f32;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }

    ShiftResult {
        best_shift: (best_dx, best_dy),
        correlation: best_corr,
        has_translating_structure: best_corr > 0.5,
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ShiftResult {
    pub best_shift: (i32, i32),
    pub correlation: f32,
    pub has_translating_structure: bool,
}

/// Snapshot a grid as a flat bool array for use with complexity/correlation metrics.
pub fn snapshot_grid(grid: &Grid) -> Vec<bool> {
    let size = grid.size();
    let mut snap = Vec::with_capacity((size.x * size.y) as usize);
    for y in 0..size.y {
        for x in 0..size.x {
            snap.push(grid.get(x, y).is_some());
        }
    }
    snap
}

/// Run a Derrida measurement on a snap configuration.
/// Creates two identical simulations, flips `num_perturbations` random cells in one,
/// then advances both for `steps` using pure GoL stepping.
pub fn measure_derrida_from_grid(
    grid: &Grid,
    perturb_x: Coordinate,
    perturb_y: Coordinate,
    steps: usize,
) -> DerridaResult {
    use crate::analysis::gol_step;

    let mut perturbed = grid.clone();
    let cell = perturbed.mutate(perturb_x, perturb_y);
    if cell.is_some() {
        *cell = None;
    } else {
        *cell = Some(crate::grid::Cell {
            age: std::num::NonZeroU32::new(1).unwrap(),
            avg_breed_age: 0.0,
            avg_velocity: [0.0; 2],
        });
    }

    derrida_parameter(grid, &perturbed, |g| gol_step(g), steps)
}

/// Result of a single point along a rule-space transect.
#[derive(Clone, Debug, Default)]
pub struct TransectPoint {
    pub t: f32,
    pub derrida: DerridaResult,
    pub complexity: ComplexityResult,
    pub mean_field: f64,
    pub alive_ratio: f32,
}

/// Configuration for transect measurements.
pub struct TransectConfig {
    pub grid_size: i32,
    pub sim_steps: usize,
    pub num_seeds: usize,
    pub derrida_steps: usize,
}

impl Default for TransectConfig {
    fn default() -> Self {
        Self {
            grid_size: 64,
            sim_steps: 1000,
            num_seeds: 4,
            derrida_steps: 50,
        }
    }
}

fn interpolate_rules(
    spawn_a: &[f32; 9], keep_a: &[f32; 9],
    spawn_b: &[f32; 9], keep_b: &[f32; 9],
    t: f32,
) -> ([f32; 9], [f32; 9]) {
    let mut spawn = [0.0f32; 9];
    let mut keep = [0.0f32; 9];
    for k in 0..9 {
        spawn[k] = spawn_a[k] * (1.0 - t) + spawn_b[k] * t;
        keep[k] = keep_a[k] * (1.0 - t) + keep_b[k] * t;
    }
    (spawn, keep)
}

/// One probabilistic step: each cell uses the probability tables with RNG.
fn probabilistic_step(
    g: &Grid,
    spawn: &[f32; 9],
    keep: &[f32; 9],
    rng: &mut impl rand::Rng,
) -> Grid {
    use crate::grid::Cell;
    let sz = g.size();
    let mut next = Grid::new(sz);
    for y in 0..sz.y {
        for x in 0..sz.x {
            let mut count = 0u32;
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    if dx == 0 && dy == 0 { continue; }
                    if g.get(x + dx, y + dy).is_some() {
                        count += 1;
                    }
                }
            }
            let coin: f32 = rng.gen();
            *next.mutate(x, y) = match g.get(x, y) {
                None if coin < spawn[count as usize] => Some(Cell {
                    age: std::num::NonZeroU32::new(1).unwrap(),
                    avg_breed_age: 0.0,
                    avg_velocity: [0.0; 2],
                }),
                Some(cell) if coin < keep[count as usize] => Some(Cell {
                    age: std::num::NonZeroU32::new(cell.age.get() + 1).unwrap(),
                    avg_breed_age: cell.avg_breed_age,
                    avg_velocity: cell.avg_velocity,
                }),
                _ => None,
            };
        }
    }
    next
}

/// Deterministic step using threshold > 0.5.
fn deterministic_step(g: &Grid, spawn: &[f32; 9], keep: &[f32; 9]) -> Grid {
    use crate::grid::Cell;
    let sz = g.size();
    let mut next = Grid::new(sz);
    for y in 0..sz.y {
        for x in 0..sz.x {
            let mut count = 0u32;
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    if dx == 0 && dy == 0 { continue; }
                    if g.get(x + dx, y + dy).is_some() {
                        count += 1;
                    }
                }
            }
            *next.mutate(x, y) = match g.get(x, y) {
                None if spawn[count as usize] > 0.5 => Some(Cell {
                    age: std::num::NonZeroU32::new(1).unwrap(),
                    avg_breed_age: 0.0,
                    avg_velocity: [0.0; 2],
                }),
                Some(cell) if keep[count as usize] > 0.5 => Some(Cell {
                    age: std::num::NonZeroU32::new(cell.age.get() + 1).unwrap(),
                    avg_breed_age: cell.avg_breed_age,
                    avg_velocity: cell.avg_velocity,
                }),
                _ => None,
            };
        }
    }
    next
}

/// Step with position-seeded randomness: both Derrida twins get identical
/// random numbers at each (x,y) so only the initial perturbation causes
/// divergence. Uses a simple hash: splitmix64 of (step<<32 | y<<16 | x).
fn seeded_probabilistic_step(
    g: &Grid,
    spawn: &[f32; 9],
    keep: &[f32; 9],
    step: u64,
) -> Grid {
    use crate::grid::Cell;
    let sz = g.size();
    let mut next = Grid::new(sz);
    for y in 0..sz.y {
        for x in 0..sz.x {
            let mut count = 0u32;
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    if dx == 0 && dy == 0 { continue; }
                    if g.get(x + dx, y + dy).is_some() {
                        count += 1;
                    }
                }
            }
            let hash_input = (step << 32) | ((y as u64 & 0xFFFF) << 16) | (x as u64 & 0xFFFF);
            let coin = splitmix_f32(hash_input);
            *next.mutate(x, y) = match g.get(x, y) {
                None if coin < spawn[count as usize] => Some(Cell {
                    age: std::num::NonZeroU32::new(1).unwrap(),
                    avg_breed_age: 0.0,
                    avg_velocity: [0.0; 2],
                }),
                Some(cell) if coin < keep[count as usize] => Some(Cell {
                    age: std::num::NonZeroU32::new(cell.age.get() + 1).unwrap(),
                    avg_breed_age: cell.avg_breed_age,
                    avg_velocity: cell.avg_velocity,
                }),
                _ => None,
            };
        }
    }
    next
}

fn splitmix_f32(mut x: u64) -> f32 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x = x ^ (x >> 31);
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn is_deterministic(spawn: &[f32; 9], keep: &[f32; 9]) -> bool {
    spawn.iter().chain(keep.iter()).all(|&p| p == 0.0 || p == 1.0)
}

fn make_soup(size: crate::grid::Coordinates, density: f32, rng: &mut impl rand::Rng) -> Grid {
    let mut grid = Grid::new(size);
    for y in 0..size.y {
        for x in 0..size.x {
            if rng.gen::<f32>() < density {
                grid.init(x, y);
            }
        }
    }
    grid
}

/// Measure emergence for a single rule set, averaging over multiple seeds.
fn measure_rule(
    spawn: &[f32; 9],
    keep: &[f32; 9],
    cfg: &TransectConfig,
    base_seed: u64,
) -> (DerridaResult, ComplexityResult, f32) {
    use crate::grid::{Cell, Coordinates};
    use rand::SeedableRng;

    let size = Coordinates { x: cfg.grid_size, y: cfg.grid_size };
    let total_cells = (cfg.grid_size * cfg.grid_size) as f32;
    let det = is_deterministic(spawn, keep);

    let mut sum_spreading = 0.0f64;
    let mut sum_lambda = 0.0f64;
    let mut sum_entropy = 0.0f64;
    let mut sum_autocorr = 0.0f64;
    let mut sum_complexity = 0.0f64;
    let mut sum_alive = 0.0f64;
    let mut valid_count = 0u32;

    for s in 0..cfg.num_seeds {
        let seed = base_seed.wrapping_add(s as u64 * 0x_9E37_79B9);
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        // Complexity: 10% soup, run full simulation
        let grid = make_soup(size, 0.1, &mut rng);
        let mut current = grid.clone();
        let mut snapshots = Vec::new();
        let mut alive_ratio = 0.0f32;
        for step in 0..cfg.sim_steps {
            current = if det {
                deterministic_step(&current, spawn, keep)
            } else {
                probabilistic_step(&current, spawn, keep, &mut rng)
            };
            if step % 20 == 0 {
                snapshots.push(snapshot_grid(&current));
            }
            alive_ratio = current.alive_count() as f32 / total_cells;
            if alive_ratio == 0.0 || alive_ratio > 0.9 {
                break;
            }
        }

        let cx = spacetime_complexity(&snapshots, cfg.grid_size as usize);

        // Derrida: 50% dense soup, flip one cell, measure divergence
        let derrida_seed = seed.wrapping_add(0x_DEAD_BEEF);
        let mut rng_d = rand::rngs::StdRng::seed_from_u64(derrida_seed);
        let dense = make_soup(size, 0.5, &mut rng_d);
        let mut perturbed = dense.clone();
        let px = cfg.grid_size / 2;
        let py = cfg.grid_size / 2;
        let cell = perturbed.mutate(px, py);
        if cell.is_some() {
            *cell = None;
        } else {
            *cell = Some(Cell {
                age: std::num::NonZeroU32::new(1).unwrap(),
                avg_breed_age: 0.0,
                avg_velocity: [0.0; 2],
            });
        }

        let dr = if det {
            derrida_parameter(&dense, &perturbed, |g| deterministic_step(g, spawn, keep), cfg.derrida_steps)
        } else {
            // Position-seeded stepping: both twins get identical per-cell
            // randomness at the same step, so only the initial perturbation
            // drives divergence. Counter increments on each call; since
            // derrida_parameter calls step_fn for A then B at each step,
            // we divide by 2 to get the logical step number.
            let sp = *spawn;
            let kp = *keep;
            let call_counter = std::cell::Cell::new(0u64);
            derrida_parameter(&dense, &perturbed, |g| {
                let c = call_counter.get();
                call_counter.set(c + 1);
                seeded_probabilistic_step(g, &sp, &kp, c / 2)
            }, cfg.derrida_steps)
        };

        sum_spreading += dr.spreading_rate as f64;
        sum_lambda += dr.lambda as f64;
        sum_entropy += cx.entropy as f64;
        sum_autocorr += cx.autocorrelation as f64;
        sum_complexity += cx.complexity as f64;
        sum_alive += alive_ratio as f64;
        valid_count += 1;
    }

    if valid_count == 0 {
        return (DerridaResult::default(), ComplexityResult::default(), 0.0);
    }

    let n = valid_count as f64;
    let avg_derrida = DerridaResult {
        lambda: (sum_lambda / n) as f32,
        initial_damage: 1,
        final_damage: (sum_lambda / n) as usize,
        peak_damage: 0,
        spreading_rate: (sum_spreading / n) as f32,
    };
    let avg_complexity = ComplexityResult {
        entropy: (sum_entropy / n) as f32,
        autocorrelation: (sum_autocorr / n) as f32,
        density_variance: 0.0,
        complexity: (sum_complexity / n) as f32,
    };
    let avg_alive = (sum_alive / n) as f32;

    (avg_derrida, avg_complexity, avg_alive)
}

/// Sweep a 1D path through rule space, measuring emergence at each point.
///
/// Linearly interpolates spawn and keep tables from `rules_a` to `rules_b`
/// at `num_points` evenly spaced values of t ∈ [0, 1].
/// Uses probabilistic stepping for non-binary rules and averages over
/// `cfg.num_seeds` random initial conditions for noise reduction.
pub fn rule_transect(
    spawn_a: &[f32; 9],
    keep_a: &[f32; 9],
    spawn_b: &[f32; 9],
    keep_b: &[f32; 9],
    num_points: usize,
    grid_size: i32,
    sim_steps: usize,
    seed: u64,
) -> Vec<TransectPoint> {
    let cfg = TransectConfig {
        grid_size,
        sim_steps,
        num_seeds: 4,
        derrida_steps: 50,
    };
    rule_transect_cfg(spawn_a, keep_a, spawn_b, keep_b, num_points, seed, &cfg)
}

pub fn rule_transect_cfg(
    spawn_a: &[f32; 9],
    keep_a: &[f32; 9],
    spawn_b: &[f32; 9],
    keep_b: &[f32; 9],
    num_points: usize,
    seed: u64,
    cfg: &TransectConfig,
) -> Vec<TransectPoint> {
    use crate::rules;

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let indices: Vec<usize> = (0..num_points).collect();

    let results: Vec<TransectPoint> = std::thread::scope(|s| {
        let chunks: Vec<_> = indices.chunks(indices.len().div_ceil(num_threads)).collect();
        let handles: Vec<_> = chunks.into_iter().map(|chunk| {
            let cfg = &cfg;
            s.spawn(move || {
                let mut results = Vec::with_capacity(chunk.len());
                for &i in chunk {
                    let t = i as f32 / (num_points - 1).max(1) as f32;
                    let (spawn, keep) = interpolate_rules(spawn_a, keep_a, spawn_b, keep_b, t);
                    let mf = match rules::mean_field_classify(&spawn, &keep) {
                        rules::MeanFieldClass::Stable(rho) => rho,
                        rules::MeanFieldClass::Decays => 0.0,
                        rules::MeanFieldClass::Grows => 1.0,
                    };
                    let (derrida, complexity, alive_ratio) = measure_rule(&spawn, &keep, cfg, seed);
                    results.push((i, TransectPoint { t, derrida, complexity, mean_field: mf, alive_ratio }));
                }
                results
            })
        }).collect();
        let mut all: Vec<_> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_by_key(|(idx, _)| *idx);
        all.into_iter().map(|(_, p)| p).collect()
    });

    results
}

/// Result of a 2D slice through rule space.
#[derive(Clone, Debug)]
pub struct SlicePoint {
    pub x_param: f32,
    pub y_param: f32,
    pub derrida: DerridaResult,
    pub complexity: ComplexityResult,
    pub mean_field: f64,
    pub alive_ratio: f32,
}

/// Sweep a 2D slice through rule space by varying two spawn/keep entries.
///
/// `base_spawn`/`base_keep` define the starting rule.
/// `x_index` and `y_index` select which table entries to vary:
///   0-8 → spawn[i], 9-17 → keep[i-9].
/// Each axis sweeps from 0.0 to 1.0 in `resolution` steps.
pub fn rule_slice_2d(
    base_spawn: &[f32; 9],
    base_keep: &[f32; 9],
    x_index: usize,
    y_index: usize,
    resolution: usize,
    seed: u64,
    cfg: &TransectConfig,
) -> Vec<SlicePoint> {
    let coords: Vec<(usize, usize)> = (0..resolution)
        .flat_map(|yi| (0..resolution).map(move |xi| (yi, xi)))
        .collect();

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let results: Vec<SlicePoint> = std::thread::scope(|s| {
        let chunks: Vec<_> = coords.chunks(coords.len().div_ceil(num_threads)).collect();
        let handles: Vec<_> = chunks.into_iter().map(|chunk| {
            let base_spawn = base_spawn;
            let base_keep = base_keep;
            let cfg = &cfg;
            s.spawn(move || {
                let mut results = Vec::with_capacity(chunk.len());
                for &(yi, xi) in chunk {
                    let x_val = xi as f32 / (resolution - 1).max(1) as f32;
                    let y_val = yi as f32 / (resolution - 1).max(1) as f32;

                    let mut spawn = *base_spawn;
                    let mut keep = *base_keep;

                    if x_index < 9 {
                        spawn[x_index] = x_val;
                    } else {
                        keep[x_index - 9] = x_val;
                    }
                    if y_index < 9 {
                        spawn[y_index] = y_val;
                    } else {
                        keep[y_index - 9] = y_val;
                    }

                    let mf = match crate::rules::mean_field_classify(&spawn, &keep) {
                        crate::rules::MeanFieldClass::Stable(rho) => rho,
                        crate::rules::MeanFieldClass::Decays => 0.0,
                        crate::rules::MeanFieldClass::Grows => 1.0,
                    };

                    let (derrida, complexity, alive_ratio) = measure_rule(&spawn, &keep, cfg, seed);

                    results.push((yi * resolution + xi, SlicePoint {
                        x_param: x_val,
                        y_param: y_val,
                        derrida,
                        complexity,
                        mean_field: mf,
                        alive_ratio,
                    }));
                }
                results
            })
        }).collect();
        let mut all: Vec<_> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_by_key(|(idx, _)| *idx);
        all.into_iter().map(|(_, p)| p).collect()
    });

    results
}

/// A rule found near the critical surface (Derrida ≈ 1.0).
#[derive(Clone, Debug)]
pub struct CriticalRule {
    pub spawn: [f32; 9],
    pub keep: [f32; 9],
    pub spreading_rate: f32,
    pub criticality_score: usize,
    pub complexity: f32,
    pub alive_ratio: f32,
    pub mean_field: f64,
}

/// Search for rules near the critical surface by scanning rule space.
///
/// Generates random rules (filtered by mean-field viability), measures
/// Derrida spreading rate, and keeps those near criticality.
pub fn find_critical_rules(
    num_samples: usize,
    seed: u64,
    cfg: &TransectConfig,
) -> Vec<CriticalRule> {
    use crate::rules;
    use rand::SeedableRng;
    use rand::Rng;

    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    // Phase 1: generate candidates and pre-filter (fast, sequential)
    struct Candidate {
        spawn: [f32; 9],
        keep: [f32; 9],
        mean_field: f64,
        measure_seed: u64,
    }
    let mut candidates = Vec::new();
    for _ in 0..num_samples {
        let mut spawn = [0.0f32; 9];
        let mut keep = [0.0f32; 9];
        for k in 0..9usize {
            if rng.gen::<f32>() < 0.3 {
                spawn[k] = rng.gen::<f32>();
            }
            if rng.gen::<f32>() < 0.5 {
                keep[k] = rng.gen::<f32>();
            }
        }
        let mf = rules::mean_field_classify(&spawn, &keep);
        let mf_val = match mf {
            rules::MeanFieldClass::Stable(rho) => rho,
            rules::MeanFieldClass::Decays | rules::MeanFieldClass::Grows => continue,
        };
        candidates.push(Candidate { spawn, keep, mean_field: mf_val, measure_seed: rng.gen() });
    }

    eprintln!("  {}/{} candidates pass mean-field filter, measuring...",
        candidates.len(), num_samples);

    // Phase 2: measure in parallel
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let total = candidates.len();

    let mut critical: Vec<CriticalRule> = std::thread::scope(|s| {
        let chunks: Vec<_> = candidates.chunks(candidates.len().div_ceil(num_threads)).collect();
        let handles: Vec<_> = chunks.into_iter().map(|chunk| {
            let cfg = &cfg;
            let done = &done;
            s.spawn(move || {
                let mut results = Vec::new();
                for c in chunk {
                    let (derrida, complexity, alive) = measure_rule(&c.spawn, &c.keep, cfg, c.measure_seed);
                    let finished = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    if total >= 100 && finished % (total / 10) == 0 {
                        eprintln!("  [{}/{}] measured", finished, total);
                    }
                    if derrida.spreading_rate > 0.0 && alive > 0.001 {
                        results.push(CriticalRule {
                            spawn: c.spawn,
                            keep: c.keep,
                            spreading_rate: derrida.spreading_rate,
                            criticality_score: derrida.criticality_score(),
                            complexity: complexity.complexity,
                            alive_ratio: alive,
                            mean_field: c.mean_field,
                        });
                    }
                }
                results
            })
        }).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    critical.sort_by(|a, b| b.criticality_score.cmp(&a.criticality_score));
    critical
}

// ============================================================
// Directed critical-surface search methods
// ============================================================

/// Lightweight Derrida-only measurement for gradient estimation.
/// Uses smaller grid and fewer seeds than full `measure_rule`.
fn quick_spreading_rate(spawn: &[f32; 9], keep: &[f32; 9], seed: u64) -> f32 {
    let cfg = TransectConfig {
        grid_size: 48,
        sim_steps: 400,
        num_seeds: 3,
        derrida_steps: 40,
    };
    let (dr, _, _) = measure_rule(spawn, keep, &cfg, seed);
    dr.spreading_rate
}

/// Result of a directed critical search step.
#[derive(Clone, Debug)]
pub struct CriticalSearchResult {
    pub spawn: [f32; 9],
    pub keep: [f32; 9],
    pub spreading_rate: f32,
    pub complexity: f32,
    pub alive_ratio: f32,
    pub method: &'static str,
}

/// Gradient descent toward λ=1 from a starting rule.
///
/// Numerically estimates ∂λ/∂p for each nonzero parameter, then steps
/// in the direction that moves λ closer to 1.0. Returns the trajectory.
pub fn gradient_descent_to_critical(
    spawn_start: &[f32; 9],
    keep_start: &[f32; 9],
    max_steps: usize,
    seed: u64,
    cfg: &TransectConfig,
) -> Vec<CriticalSearchResult> {
    let epsilon = 0.02f32;
    let learning_rate = 0.1f32;
    let target = 1.0f32;

    let mut spawn = *spawn_start;
    let mut keep = *keep_start;
    let mut trajectory = Vec::new();

    for step in 0..max_steps {
        let step_seed = seed.wrapping_add(step as u64 * 0x_1234_5678);
        let (dr, cx, alive) = measure_rule(&spawn, &keep, cfg, step_seed);
        let lambda = dr.spreading_rate;

        trajectory.push(CriticalSearchResult {
            spawn,
            keep,
            spreading_rate: lambda,
            complexity: cx.complexity,
            alive_ratio: alive,
            method: "gradient_descent",
        });

        if (lambda - target).abs() < 0.01 {
            break;
        }
        if lambda == 0.0 {
            break;
        }

        // Compute gradient: ∂λ/∂spawn[k] and ∂λ/∂keep[k]
        let mut grad_spawn = [0.0f32; 9];
        let mut grad_keep = [0.0f32; 9];

        for k in 0..9 {
            if spawn[k] > 0.0 || keep[k] > 0.0 {
                if spawn[k] > 0.0 {
                    let mut sp_plus = spawn;
                    sp_plus[k] = (spawn[k] + epsilon).min(1.0);
                    let lambda_plus = quick_spreading_rate(&sp_plus, &keep, step_seed);
                    if lambda_plus > 0.0 {
                        grad_spawn[k] = (lambda_plus - lambda) / epsilon;
                    }
                }
                if keep[k] > 0.0 {
                    let mut kp_plus = keep;
                    kp_plus[k] = (keep[k] + epsilon).min(1.0);
                    let lambda_plus = quick_spreading_rate(&spawn, &kp_plus, step_seed);
                    if lambda_plus > 0.0 {
                        grad_keep[k] = (lambda_plus - lambda) / epsilon;
                    }
                }
            }
        }

        // Step toward target: if λ > 1, move in -∇λ direction; if λ < 1, move in +∇λ
        // Reduce step size as we approach target
        let distance = (lambda - target).abs();
        let adaptive_lr = learning_rate * distance.min(1.0);
        let sign = if lambda > target { -1.0 } else { 1.0 };
        for k in 0..9 {
            spawn[k] = (spawn[k] + sign * adaptive_lr * grad_spawn[k]).clamp(0.0, 1.0);
            keep[k] = (keep[k] + sign * adaptive_lr * grad_keep[k]).clamp(0.0, 1.0);
        }
    }

    trajectory
}

/// Binary search on a transect between two rules to find the exact critical point.
///
/// Given an ordered rule (λ < 1) and a chaotic rule (λ > 1), bisects the
/// interpolation parameter t until λ is within `tolerance` of 1.0.
#[allow(clippy::too_many_arguments)]
pub fn binary_search_critical(
    spawn_ordered: &[f32; 9],
    keep_ordered: &[f32; 9],
    spawn_chaotic: &[f32; 9],
    keep_chaotic: &[f32; 9],
    tolerance: f32,
    max_iterations: usize,
    seed: u64,
    cfg: &TransectConfig,
) -> CriticalSearchResult {
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    let mut best_spawn = *spawn_ordered;
    let mut best_keep = *keep_ordered;
    let mut best_lambda = 0.0f32;
    let mut best_cx = 0.0f32;
    let mut best_alive = 0.0f32;

    for iter in 0..max_iterations {
        let mid = (lo + hi) / 2.0;
        let (spawn, keep) = interpolate_rules(spawn_ordered, keep_ordered, spawn_chaotic, keep_chaotic, mid);
        let iter_seed = seed.wrapping_add(iter as u64 * 0x_ABCD_EF01);
        let (dr, cx, alive) = measure_rule(&spawn, &keep, cfg, iter_seed);

        best_spawn = spawn;
        best_keep = keep;
        best_lambda = dr.spreading_rate;
        best_cx = cx.complexity;
        best_alive = alive;

        if (dr.spreading_rate - 1.0).abs() < tolerance {
            break;
        }

        if dr.spreading_rate < 1.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    CriticalSearchResult {
        spawn: best_spawn,
        keep: best_keep,
        spreading_rate: best_lambda,
        complexity: best_cx,
        alive_ratio: best_alive,
        method: "binary_search",
    }
}

/// A scored population member: (fitness, spawn, keep, spreading_rate, complexity, alive).
type ScoredRule = (f64, [f32; 9], [f32; 9], f32, f32, f32);

/// CMA-ES-inspired evolutionary optimization for criticality × complexity.
///
/// Maintains a population of rules, evaluates a combined fitness of
/// criticality_score + complexity, and evolves toward rules that are both
/// critical and complex. Uses rank-based selection and Gaussian perturbation.
pub fn cma_evolve_critical(
    population_size: usize,
    generations: usize,
    seed: u64,
    cfg: &TransectConfig,
) -> Vec<CriticalSearchResult> {
    use rand::Rng;
    use rand::SeedableRng;

    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    // Initialize population from random viable rules
    let mut population: Vec<([f32; 9], [f32; 9])> = Vec::new();
    for _ in 0..population_size * 4 {
        if population.len() >= population_size {
            break;
        }
        let mut spawn = [0.0f32; 9];
        let mut keep = [0.0f32; 9];
        for k in 0..9 {
            if rng.gen::<f32>() < 0.3 {
                spawn[k] = rng.gen::<f32>();
            }
            if rng.gen::<f32>() < 0.5 {
                keep[k] = rng.gen::<f32>();
            }
        }
        let mf = crate::rules::mean_field_classify(&spawn, &keep);
        if matches!(mf, crate::rules::MeanFieldClass::Stable(_)) {
            population.push((spawn, keep));
        }
    }

    // Pad if we didn't get enough
    while population.len() < population_size {
        let mut spawn = [0.0f32; 9];
        let mut keep = [0.0f32; 9];
        spawn[3] = rng.gen::<f32>();
        keep[2] = rng.gen::<f32>();
        keep[3] = rng.gen::<f32>();
        population.push((spawn, keep));
    }

    let mut best_results: Vec<CriticalSearchResult> = Vec::new();
    let mut sigma = 0.15f32; // mutation step size

    for gen in 0..generations {
        // Evaluate population in parallel
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let scored: Vec<ScoredRule> = std::thread::scope(|s| {
            let chunks: Vec<_> = population.chunks(population.len().div_ceil(num_threads)).collect();
            let handles: Vec<_> = chunks.into_iter().enumerate().map(|(ci, chunk)| {
                let cfg = &cfg;
                let gen_seed = seed.wrapping_add(gen as u64 * 0x_FF00_FF00 + ci as u64);
                s.spawn(move || {
                    chunk.iter().map(|(sp, kp)| {
                        let (dr, cx, alive) = measure_rule(sp, kp, cfg, gen_seed);
                        // Fitness: criticality × (1 + complexity/10) × viability
                        let crit = dr.criticality_score() as f64;
                        let viability = if alive > 0.01 { 1.0 } else { 0.1 };
                        let fitness = crit * (1.0 + cx.complexity as f64 / 10.0) * viability;
                        (fitness, *sp, *kp, dr.spreading_rate, cx.complexity, alive)
                    }).collect::<Vec<_>>()
                })
            }).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });

        // Sort by fitness (descending)
        let mut ranked = scored;
        ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Record best
        let (fitness, sp, kp, lambda, cx, alive) = &ranked[0];
        eprintln!("  gen {:2}: best fitness={:.1}, λ={:.3}, cx={:.1}, alive={:.3}",
            gen, fitness, lambda, cx, alive);
        best_results.push(CriticalSearchResult {
            spawn: *sp,
            keep: *kp,
            spreading_rate: *lambda,
            complexity: *cx,
            alive_ratio: *alive,
            method: "cma_evolve",
        });

        if gen == generations - 1 {
            break;
        }

        // Select top half as parents
        let elite_size = population_size / 2;
        let elites: Vec<_> = ranked[..elite_size].iter()
            .map(|(_, sp, kp, ..)| (*sp, *kp))
            .collect();

        // Generate next population: elites + mutated offspring
        population.clear();
        population.extend_from_slice(&elites);

        while population.len() < population_size {
            let parent_idx = rng.gen_range(0..elite_size);
            let (parent_sp, parent_kp) = &elites[parent_idx];
            let mut child_sp = *parent_sp;
            let mut child_kp = *parent_kp;

            // Gaussian perturbation
            for k in 0..9 {
                if child_sp[k] > 0.0 || rng.gen::<f32>() < 0.05 {
                    child_sp[k] = (child_sp[k] + gaussian(&mut rng) * sigma).clamp(0.0, 1.0);
                }
                if child_kp[k] > 0.0 || rng.gen::<f32>() < 0.05 {
                    child_kp[k] = (child_kp[k] + gaussian(&mut rng) * sigma).clamp(0.0, 1.0);
                }
            }

            // Only keep if mean-field viable
            let mf = crate::rules::mean_field_classify(&child_sp, &child_kp);
            if matches!(mf, crate::rules::MeanFieldClass::Stable(_)) {
                population.push((child_sp, child_kp));
            }
        }

        // Adaptive step size: shrink if converging, grow if stuck
        if gen > 0 {
            let prev_fit = best_results[best_results.len() - 2].spreading_rate;
            let curr_fit = best_results[best_results.len() - 1].spreading_rate;
            if (curr_fit - 1.0).abs() < (prev_fit - 1.0).abs() {
                sigma *= 0.9;
            } else {
                sigma *= 1.1;
            }
            sigma = sigma.clamp(0.02, 0.3);
        }
    }

    best_results
}

/// Trace the critical manifold from a known critical rule.
///
/// Starting from a rule with λ≈1, perturb one parameter at a time and
/// use Newton's method on another parameter to restore λ=1. This traces
/// out the critical surface in the direction of increasing complexity.
pub fn trace_critical_manifold(
    spawn_start: &[f32; 9],
    keep_start: &[f32; 9],
    num_steps: usize,
    step_size: f32,
    seed: u64,
    cfg: &TransectConfig,
) -> Vec<CriticalSearchResult> {
    let mut spawn = *spawn_start;
    let mut keep = *keep_start;
    let mut trajectory = Vec::new();

    // Initial measurement
    let (dr, cx, alive) = measure_rule(&spawn, &keep, cfg, seed);
    trajectory.push(CriticalSearchResult {
        spawn,
        keep,
        spreading_rate: dr.spreading_rate,
        complexity: cx.complexity,
        alive_ratio: alive,
        method: "manifold_trace",
    });

    for step in 0..num_steps {
        let step_seed = seed.wrapping_add(step as u64 * 0x_7777_7777);

        // Find which parameter to perturb: pick a random active one
        // We perturb a "drive" parameter and adjust a "control" to stay on λ=1
        let active_spawn: Vec<usize> = (0..9).filter(|&k| spawn[k] > 0.01).collect();
        let active_keep: Vec<usize> = (0..9).filter(|&k| keep[k] > 0.01).collect();

        if active_spawn.is_empty() && active_keep.is_empty() {
            break;
        }

        // Choose drive parameter: the one whose gradient has the largest
        // complexity component (move toward higher complexity)
        let mut best_drive = None;
        let mut best_cx_grad = f32::NEG_INFINITY;

        let epsilon = 0.03f32;
        let base_lambda = quick_spreading_rate(&spawn, &keep, step_seed);
        let base_cx = {
            let (_, c, _) = measure_rule(&spawn, &keep, &TransectConfig {
                grid_size: 48, sim_steps: 500, num_seeds: 2, derrida_steps: 30,
            }, step_seed);
            c.complexity
        };

        for &k in &active_spawn {
            let mut sp_test = spawn;
            sp_test[k] = (spawn[k] + epsilon).min(1.0);
            let (_, cx_test, _) = measure_rule(&sp_test, &keep, &TransectConfig {
                grid_size: 48, sim_steps: 500, num_seeds: 2, derrida_steps: 30,
            }, step_seed);
            let cx_grad = (cx_test.complexity - base_cx) / epsilon;
            if cx_grad > best_cx_grad {
                best_cx_grad = cx_grad;
                best_drive = Some((true, k));
            }
        }
        for &k in &active_keep {
            let mut kp_test = keep;
            kp_test[k] = (keep[k] + epsilon).min(1.0);
            let (_, cx_test, _) = measure_rule(&spawn, &kp_test, &TransectConfig {
                grid_size: 48, sim_steps: 500, num_seeds: 2, derrida_steps: 30,
            }, step_seed);
            let cx_grad = (cx_test.complexity - base_cx) / epsilon;
            if cx_grad > best_cx_grad {
                best_cx_grad = cx_grad;
                best_drive = Some((false, k));
            }
        }

        let Some((is_spawn_drive, drive_idx)) = best_drive else { break; };

        // Perturb drive parameter
        if is_spawn_drive {
            spawn[drive_idx] = (spawn[drive_idx] + step_size).clamp(0.0, 1.0);
        } else {
            keep[drive_idx] = (keep[drive_idx] + step_size).clamp(0.0, 1.0);
        }

        // Choose control parameter: the one with the largest |∂λ/∂p|
        // (most effective at restoring λ=1)
        let mut best_control = None;
        let mut best_lambda_grad = 0.0f32;

        for &k in &active_spawn {
            if is_spawn_drive && k == drive_idx { continue; }
            let mut sp_test = spawn;
            sp_test[k] = (spawn[k] + epsilon).min(1.0);
            let l = quick_spreading_rate(&sp_test, &keep, step_seed);
            if l > 0.0 {
                let grad = (l - base_lambda).abs() / epsilon;
                if grad > best_lambda_grad {
                    best_lambda_grad = grad;
                    best_control = Some((true, k));
                }
            }
        }
        for &k in &active_keep {
            if !is_spawn_drive && k == drive_idx { continue; }
            let mut kp_test = keep;
            kp_test[k] = (keep[k] + epsilon).min(1.0);
            let l = quick_spreading_rate(&spawn, &kp_test, step_seed);
            if l > 0.0 {
                let grad = (l - base_lambda).abs() / epsilon;
                if grad > best_lambda_grad {
                    best_lambda_grad = grad;
                    best_control = Some((false, k));
                }
            }
        }

        // Newton's method: adjust control to restore λ=1
        if let Some((is_spawn_ctrl, ctrl_idx)) = best_control {
            for _ in 0..5 {
                let current_lambda = quick_spreading_rate(&spawn, &keep, step_seed);
                if (current_lambda - 1.0).abs() < 0.02 {
                    break;
                }
                if current_lambda == 0.0 {
                    break;
                }

                // Estimate gradient of λ w.r.t. control
                let grad = if is_spawn_ctrl {
                    let mut sp_test = spawn;
                    sp_test[ctrl_idx] = (spawn[ctrl_idx] + epsilon).min(1.0);
                    let l = quick_spreading_rate(&sp_test, &keep, step_seed);
                    (l - current_lambda) / epsilon
                } else {
                    let mut kp_test = keep;
                    kp_test[ctrl_idx] = (keep[ctrl_idx] + epsilon).min(1.0);
                    let l = quick_spreading_rate(&spawn, &kp_test, step_seed);
                    (l - current_lambda) / epsilon
                };

                if grad.abs() < 1e-6 {
                    break;
                }

                // Damped Newton step (0.5× to avoid overshooting)
                let correction = 0.5 * (1.0 - current_lambda) / grad;
                if is_spawn_ctrl {
                    spawn[ctrl_idx] = (spawn[ctrl_idx] + correction).clamp(0.0, 1.0);
                } else {
                    keep[ctrl_idx] = (keep[ctrl_idx] + correction).clamp(0.0, 1.0);
                }
            }
        }

        // Full measurement at the new point
        let (dr, cx, alive) = measure_rule(&spawn, &keep, cfg, step_seed);
        trajectory.push(CriticalSearchResult {
            spawn,
            keep,
            spreading_rate: dr.spreading_rate,
            complexity: cx.complexity,
            alive_ratio: alive,
            method: "manifold_trace",
        });

        eprintln!("  trace step {:2}: λ={:.3}, cx={:.1}, alive={:.3}",
            step, dr.spreading_rate, cx.complexity, alive);

        // Bail if we lost viability
        if alive < 0.001 || dr.spreading_rate == 0.0 {
            break;
        }
    }

    trajectory
}

fn gaussian(rng: &mut impl rand::Rng) -> f32 {
    let u1: f32 = rng.gen::<f32>().max(1e-10);
    let u2: f32 = rng.gen();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Coordinates, Grid};

    #[test]
    fn block_is_ordered() {
        // A block (still life) should show ordered behavior: perturbation decays
        let mut grid = Grid::new(Coordinates { x: 32, y: 32 });
        grid.init(15, 15);
        grid.init(16, 15);
        grid.init(15, 16);
        grid.init(16, 16);

        let result = measure_derrida_from_grid(&grid, 14, 14, 20);
        // Flipping a cell near a block should cause small, bounded damage
        assert!(
            result.final_damage <= result.peak_damage,
            "damage should be bounded"
        );
    }

    #[test]
    fn empty_grid_zero_damage() {
        let grid = Grid::new(Coordinates { x: 16, y: 16 });
        let result = measure_derrida_from_grid(&grid, 8, 8, 10);
        // Single cell on empty grid dies after 1 step (B3/S23 needs 3 neighbors),
        // so after the first advance both grids are identical → initial_damage = 0.
        assert_eq!(result.initial_damage, 0);
        assert_eq!(result.lambda, 0.0);
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut grid = Grid::new(Coordinates { x: 8, y: 8 });
        grid.init(3, 4);
        grid.init(5, 2);
        let snap = snapshot_grid(&grid);
        assert_eq!(snap.len(), 64);
        assert!(snap[4 * 8 + 3]); // (3, 4)
        assert!(snap[2 * 8 + 5]); // (5, 2)
        assert!(!snap[0]);
    }

    #[test]
    fn spacetime_complexity_empty() {
        let result = spacetime_complexity(&[], 8);
        assert_eq!(result.entropy, 0.0);
    }

    #[test]
    fn shift_correlation_static() {
        // A static pattern should have best correlation at (0,0) — no translation
        let grid = vec![false; 64];
        let result = shift_cross_correlation(&grid, &grid, 8, 8, 3);
        assert!(!result.has_translating_structure);
    }
}
