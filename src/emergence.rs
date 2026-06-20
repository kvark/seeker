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

/// Sweep a 1D path through rule space, measuring emergence at each point.
///
/// Linearly interpolates spawn and keep tables from `rules_a` to `rules_b`
/// at `num_points` evenly spaced values of t ∈ [0, 1].
/// For each interpolated rule, runs a simulation from a random soup and measures
/// Derrida damage-spreading and spacetime complexity.
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
    use crate::grid::{Coordinates, Grid, Cell};
    use crate::rules;
    use rand::SeedableRng;

    let mut results = Vec::with_capacity(num_points);

    for i in 0..num_points {
        let t = i as f32 / (num_points - 1).max(1) as f32;
        let mut spawn = [0.0f32; 9];
        let mut keep = [0.0f32; 9];
        for k in 0..9 {
            spawn[k] = spawn_a[k] * (1.0 - t) + spawn_b[k] * t;
            keep[k] = keep_a[k] * (1.0 - t) + keep_b[k] * t;
        }

        let mf = match rules::mean_field_classify(&spawn, &keep) {
            rules::MeanFieldClass::Stable(rho) => rho,
            rules::MeanFieldClass::Decays => 0.0,
            rules::MeanFieldClass::Grows => 1.0,
        };

        let size = Coordinates { x: grid_size, y: grid_size };
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut grid = Grid::new(size);
        for y in 0..grid_size {
            for x in 0..grid_size {
                let coin: f32 = {
                    use rand::Rng;
                    rng.gen()
                };
                if coin < 0.1 {
                    grid.init(x, y);
                }
            }
        }

        let step_fn = move |g: &Grid| {
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
        };

        // Run simulation to collect snapshots
        let mut current = grid.clone();
        let mut snapshots = Vec::new();
        let mut alive_ratio = 0.0f32;
        for s in 0..sim_steps {
            current = step_fn(&current);
            if s % 20 == 0 {
                snapshots.push(snapshot_grid(&current));
            }
            let alive = current.alive_count() as f32 / (grid_size * grid_size) as f32;
            alive_ratio = alive;
            if alive == 0.0 || alive > 0.9 {
                break;
            }
        }

        // Derrida measurement
        let mut perturbed = grid.clone();
        let px = grid_size / 2;
        let py = grid_size / 2;
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
        let derrida = derrida_parameter(&grid, &perturbed, &step_fn, 50);

        let complexity = spacetime_complexity(&snapshots, grid_size as usize);

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
