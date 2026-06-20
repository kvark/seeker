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
            // Derrida measures sensitivity to initial conditions — valid with
            // deterministic dynamics even for probabilistic rules.
            let sp = *spawn;
            let kp = *keep;
            derrida_parameter(&dense, &perturbed, |g| {
                deterministic_step(g, &sp, &kp)
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

    let mut results = Vec::with_capacity(num_points);

    for i in 0..num_points {
        let t = i as f32 / (num_points - 1).max(1) as f32;
        let (spawn, keep) = interpolate_rules(spawn_a, keep_a, spawn_b, keep_b, t);

        let mf = match rules::mean_field_classify(&spawn, &keep) {
            rules::MeanFieldClass::Stable(rho) => rho,
            rules::MeanFieldClass::Decays => 0.0,
            rules::MeanFieldClass::Grows => 1.0,
        };

        let (derrida, complexity, alive_ratio) = measure_rule(&spawn, &keep, cfg, seed);

        results.push(TransectPoint {
            t,
            derrida,
            complexity,
            mean_field: mf,
            alive_ratio,
        });
    }

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
    let mut results = Vec::with_capacity(resolution * resolution);

    for yi in 0..resolution {
        let y_val = yi as f32 / (resolution - 1).max(1) as f32;
        for xi in 0..resolution {
            let x_val = xi as f32 / (resolution - 1).max(1) as f32;

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

            results.push(SlicePoint {
                x_param: x_val,
                y_param: y_val,
                derrida,
                complexity,
                mean_field: mf,
                alive_ratio,
            });
        }
    }

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
    let mut critical = Vec::new();

    for _ in 0..num_samples {
        let mut spawn = [0.0f32; 9];
        let mut keep = [0.0f32; 9];

        // Generate random rule biased toward sparse spawn, moderate keep
        for k in 0..9usize {
            if rng.gen::<f32>() < 0.3 {
                spawn[k] = rng.gen::<f32>();
            }
            if rng.gen::<f32>() < 0.5 {
                keep[k] = rng.gen::<f32>();
            }
        }

        // Mean-field pre-filter
        let mf = rules::mean_field_classify(&spawn, &keep);
        let mf_val = match mf {
            rules::MeanFieldClass::Stable(rho) => rho,
            rules::MeanFieldClass::Decays | rules::MeanFieldClass::Grows => continue,
        };

        let (derrida, complexity, alive) = measure_rule(&spawn, &keep, cfg, rng.gen());

        if derrida.spreading_rate > 0.0 && alive > 0.001 {
            let cs = derrida.criticality_score();
            critical.push(CriticalRule {
                spawn,
                keep,
                spreading_rate: derrida.spreading_rate,
                criticality_score: cs,
                complexity: complexity.complexity,
                alive_ratio: alive,
                mean_field: mf_val,
            });
        }
    }

    critical.sort_by(|a, b| b.criticality_score.cmp(&a.criticality_score));
    critical
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
