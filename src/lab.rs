use crate::analysis;
use crate::grid::BoundaryMode;
use crate::sim::{
    Conclusion, Data, Probability, ProbabilityTable, Simulation, Snap, Statistics, Weight,
};
use rand::{rngs::ThreadRng, Rng as _};
use std::{
    collections::HashMap,
    fs,
    io::Write as _,
    ops::Range,
    path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const UPDATE_FREQUENCY: usize = 64;
const CHANNEL_BOUND: usize = 200;
/// Steps before we start checking for boringness.
const EARLY_DISCARD_WARMUP: usize = 128;
/// Check interval for early discard (in steps).
const EARLY_DISCARD_INTERVAL: usize = 256;
/// If alive_ratio variance stays below this for consecutive checks, discard.
const BORING_VARIANCE_THRESHOLD: f32 = 0.0001;
/// Number of consecutive boring checks before discarding.
const BORING_STREAK_LIMIT: usize = 3;

// --- MAP-Elites behavior space ---
// Axis 1: alive_ratio_average bucketed into density bins
// Axis 2: "interestingness" combining transient ships, oscillator period, narrative
const MAP_DENSITY_BINS: u8 = 10;
const MAP_INTEREST_BINS: u8 = 8;

/// A cell in the MAP-Elites behavior space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct BehaviorCell {
    density: u8,
    interest: u8,
}

impl BehaviorCell {
    fn from_stats(stats: &Statistics) -> Self {
        // Density axis: alive_ratio_average in [0, 0.10] → 10 bins
        let density = ((stats.alive_ratio_average * 100.0) as u8).min(MAP_DENSITY_BINS - 1);
        // Interest axis: composite score bucketed
        let raw_interest = stats.transient_spaceships as u32 * 3
            + stats.max_oscillator_period.saturating_sub(1) as u32 * 4
            + stats.narrative.event_diversity() as u32 * 2
            + if stats.alive_ratio_variance > 0.001 {
                2
            } else {
                0
            };
        let interest = match raw_interest {
            0 => 0,
            1..=2 => 1,
            3..=5 => 2,
            6..=10 => 3,
            11..=18 => 4,
            19..=30 => 5,
            31..=50 => 6,
            _ => 7,
        };
        BehaviorCell {
            density: density.min(MAP_DENSITY_BINS - 1),
            interest: interest.min(MAP_INTEREST_BINS - 1),
        }
    }
}

/// Entry in the MAP-Elites archive: the best experiment for a behavior cell.
struct EliteEntry {
    snap: Snap,
    fit: usize,
    id: usize,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Configuration {
    max_active: usize,
    max_in_flight: usize,
    max_archive: usize,
    size_power: Range<usize>,
    probability_step: Probability,
    max_probability_weight: Weight,
    /// When true, rules are locked and only initial conditions are mutated.
    #[serde(default)]
    pub frozen_rules: bool,
}

pub struct Experiment {
    pub id: usize,
    snap: Snap,
    pub conclusion: Option<Conclusion>,
    pub steps: usize,
    pub fit: usize,
    /// Abort flag: set to true to tell the worker to stop early.
    abort: Arc<AtomicBool>,
    /// Number of consecutive boring variance checks.
    boring_streak: usize,
}

impl Experiment {
    pub fn snap(&self) -> &Snap {
        &self.snap
    }
}

struct TaskStatus {
    experiment_id: usize,
    step: usize,
    conclusion: Option<Conclusion>,
    /// Running alive ratio variance (for early discard decisions).
    alive_ratio_variance: f32,
}

pub struct Laboratory {
    config: Configuration,
    rng: ThreadRng,
    sender_origin: crossbeam_channel::Sender<TaskStatus>,
    receiver: crossbeam_channel::Receiver<TaskStatus>,
    choir: Arc<choir::Choir>,
    // Better destroy them after the channel, so that workers
    // can see that this end is gone.
    _workers: Vec<choir::WorkerHandle>,
    experiments: Vec<Experiment>,
    next_id: usize,
    active_dir: path::PathBuf,
    log: fs::File,
    /// MAP-Elites archive: best experiment per behavior cell.
    elite_map: HashMap<BehaviorCell, EliteEntry>,
    /// Number of early discards so far.
    pub early_discards: usize,
    /// Total concluded experiments (for random injection scheduling).
    total_concluded: usize,
}

impl Laboratory {
    pub fn new(config: Configuration, active_dir_ref: impl AsRef<path::Path>) -> Self {
        let active_dir = path::PathBuf::from(active_dir_ref.as_ref());
        fs::create_dir_all(active_dir_ref).unwrap();
        {
            let file = fs::File::create(active_dir.join("config.ron")).unwrap();
            ron::ser::to_writer_pretty(file, &config, ron::ser::PrettyConfig::default()).unwrap();
        }
        let mut log = fs::File::create(active_dir.join("find.log")).unwrap();
        writeln!(log, "Seeker {}", env!("CARGO_PKG_VERSION")).unwrap();

        let choir = choir::Choir::new();
        let num_workers = config.max_active.max(2);
        let mut workers = Vec::with_capacity(num_workers);
        for i in 0..num_workers {
            workers.push(choir.add_worker(&format!("w{}", i)));
        }
        let (sender_origin, receiver) = crossbeam_channel::bounded(CHANNEL_BOUND);
        Self {
            config,
            rng: ThreadRng::default(),
            sender_origin,
            receiver,
            choir,
            _workers: workers,
            experiments: Vec::new(),
            next_id: 0,
            active_dir,
            log,
            elite_map: HashMap::new(),
            early_discards: 0,
            total_concluded: 0,
        }
    }

    pub fn experiments(&self) -> &[Experiment] {
        &self.experiments
    }

    /// Number of occupied cells in the MAP-Elites behavior map.
    pub fn map_coverage(&self) -> usize {
        self.elite_map.len()
    }

    /// Total possible cells in the MAP-Elites behavior map.
    pub fn map_capacity(&self) -> usize {
        MAP_DENSITY_BINS as usize * MAP_INTEREST_BINS as usize
    }

    pub fn add_experiment(&mut self, snap: Snap, parent_id: usize) {
        let id = self.next_id;
        self.next_id += 1;
        let sender = self.sender_origin.clone();

        {
            let name = format!("e{}.ron", id);
            let file = fs::File::create(self.active_dir.join(name)).unwrap();
            ron::ser::to_writer_pretty(file, &snap, ron::ser::PrettyConfig::default()).unwrap();
        }

        let mut sim = match Simulation::new(&snap) {
            Ok(sim) => {
                writeln!(self.log, "Mutate E[{}] -> E[{}]", parent_id, self.next_id).unwrap();
                sim
            }
            Err(e) => {
                writeln!(self.log, "Skip E[{}]: {:?}", self.next_id, e).unwrap();
                return;
            }
        };

        let abort = Arc::new(AtomicBool::new(false));
        let abort_clone = abort.clone();

        self.experiments.push(Experiment {
            id,
            snap,
            conclusion: None,
            steps: 0,
            fit: 0,
            abort,
            boring_streak: 0,
        });

        self.choir.spawn("advance").init(move |_| loop {
            // Check abort flag
            if abort_clone.load(Ordering::Relaxed) {
                let _ = sender.send(TaskStatus {
                    experiment_id: id,
                    step: sim.last_step(),
                    conclusion: Some(Conclusion::Extinct),
                    alive_ratio_variance: 0.0,
                });
                return;
            }

            let step = sim.last_step() + 1;
            match sim.advance() {
                Ok(_) if step % UPDATE_FREQUENCY == 0 => {
                    if sender
                        .send(TaskStatus {
                            experiment_id: id,
                            step,
                            conclusion: None,
                            alive_ratio_variance: sim.stats().alive_ratio_variance,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(_) => {}
                Err(conclusion) => {
                    let _ = sender.send(TaskStatus {
                        experiment_id: id,
                        step,
                        conclusion: Some(conclusion),
                        alive_ratio_variance: sim.stats().alive_ratio_variance,
                    });
                    return;
                }
            }
        });
    }

    /// Compute fitness score for a concluded experiment.
    fn compute_fitness(frozen_rules: bool, conclusion: &Conclusion) -> usize {
        match conclusion {
            Conclusion::Extinct | Conclusion::Saturate => 0,
            Conclusion::Done(ref state, ref snap) => {
                if frozen_rules {
                    let mut dummy_rng =
                        <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(0);
                    let analysis_score = if let Ok(grid) = snap.data.parse(&mut dummy_rng) {
                        let (_, summary) = analysis::analyze_grid(&grid);
                        let mut score = 0usize;
                        score += summary.unique_patterns.min(20) * 2;
                        score += summary.spaceships.len() * 30;
                        for &p in &summary.oscillators {
                            score += if p > 2 { p.min(20) } else { 1 };
                        }
                        score += summary.composability_score();
                        score
                    } else {
                        0
                    };

                    let base = 20usize;
                    let var_score = (state.alive_ratio_variance * 2000.0).min(20.0) as usize;
                    let late_score = (state.stabilized_step as f32 / 100.0).min(20.0) as usize;
                    let birth_score = (state.birth_rate_average * 5000.0).min(20.0) as usize;
                    let spatial_score =
                        (state.spatial_variance_average * 5000.0).min(20.0) as usize;
                    let narrative_score = state.narrative.richness().min(100) / 5;
                    let transient_ship_score = state.transient_spaceships.min(10) * 5;
                    let high_period_score = if state.max_oscillator_period > 2 {
                        state.max_oscillator_period.min(20) * 2
                    } else {
                        0
                    };
                    base + var_score
                        + late_score
                        + analysis_score
                        + birth_score
                        + spatial_score
                        + narrative_score
                        + transient_ship_score
                        + high_period_score
                } else {
                    let base = 100 - (60.0 * state.alive_ratio_average) as usize;
                    let birth_bonus = (state.birth_rate_average * 3000.0).min(15.0) as usize;
                    let spatial_bonus =
                        (state.spatial_variance_average * 3000.0).min(15.0) as usize;
                    let narrative_bonus = state.narrative.richness().min(100) / 7;
                    base + birth_bonus + spatial_bonus + narrative_bonus
                }
            }
            Conclusion::Crash => 0,
        }
    }

    /// Select a parent snap from the MAP-Elites archive.
    /// Uniform selection across occupied cells = diversity pressure.
    fn select_parent(&mut self) -> Option<(Snap, usize)> {
        if self.elite_map.is_empty() {
            return None;
        }
        let keys: Vec<BehaviorCell> = self.elite_map.keys().cloned().collect();
        let idx = self.rng.gen_range(0..keys.len());
        let entry = &self.elite_map[&keys[idx]];
        Some((entry.snap.clone(), entry.id))
    }

    pub fn update(&mut self) {
        while let Ok(status) = self.receiver.try_recv() {
            let exp_idx = self
                .experiments
                .iter()
                .position(|exp| exp.id == status.experiment_id)
                .unwrap();
            assert!(self.experiments[exp_idx].conclusion.is_none());
            self.experiments[exp_idx].steps = status.step;

            // Early discard: check if experiment is boring
            if status.conclusion.is_none()
                && status.step >= EARLY_DISCARD_WARMUP
                && status.step % EARLY_DISCARD_INTERVAL < UPDATE_FREQUENCY
            {
                if status.alive_ratio_variance < BORING_VARIANCE_THRESHOLD {
                    self.experiments[exp_idx].boring_streak += 1;
                    if self.experiments[exp_idx].boring_streak >= BORING_STREAK_LIMIT {
                        writeln!(
                            self.log,
                            "Early discard E[{}] at step {} (boring: var={:.6})",
                            status.experiment_id, status.step, status.alive_ratio_variance
                        )
                        .unwrap();
                        self.experiments[exp_idx]
                            .abort
                            .store(true, Ordering::Relaxed);
                        self.early_discards += 1;
                    }
                } else {
                    self.experiments[exp_idx].boring_streak = 0;
                }
            }

            if let Some(conclusion) = status.conclusion {
                let exp_id = self.experiments[exp_idx].id;
                writeln!(
                    self.log,
                    "Conclude E[{}] as {} at step {}",
                    exp_id, conclusion, status.step
                )
                .unwrap();

                let fit = Self::compute_fitness(self.config.frozen_rules, &conclusion);
                self.experiments[exp_idx].fit = fit;

                // MAP-Elites: insert into behavior map if better than current occupant
                if let Conclusion::Done(ref state, ref snap) = conclusion {
                    let cell = BehaviorCell::from_stats(state);
                    let dominated = self
                        .elite_map
                        .get(&cell)
                        .map_or(true, |existing| fit > existing.fit);
                    if dominated {
                        writeln!(
                            self.log,
                            "  MAP[d={},i={}] E[{}] fit={} (new elite)",
                            cell.density, cell.interest, exp_id, fit
                        )
                        .unwrap();
                        let name = format!("e{}-{}.ron", exp_id, status.step);
                        let file = fs::File::create(self.active_dir.join(name)).unwrap();
                        ron::ser::to_writer_pretty(
                            file,
                            snap,
                            ron::ser::PrettyConfig::default(),
                        )
                        .unwrap();
                        self.elite_map.insert(
                            cell,
                            EliteEntry {
                                snap: self.experiments[exp_idx].snap.clone(),
                                fit,
                                id: exp_id,
                            },
                        );
                    }
                }

                self.total_concluded += 1;
                self.experiments[exp_idx].conclusion = Some(conclusion);
            }
        }

        // Prune concluded experiments — only keep active ones + a small recent buffer.
        // The MAP-Elites archive is the real memory, not the experiment list.
        self.experiments
            .retain(|ex| ex.conclusion.is_none() || ex.fit > 0);
        if self.experiments.len() > self.config.max_archive + self.config.max_active {
            // Keep active experiments and the best concluded ones
            let active_count = self.experiments.iter().filter(|e| e.conclusion.is_none()).count();
            let max_concluded = self.config.max_archive;
            let mut concluded: Vec<usize> = self
                .experiments
                .iter()
                .enumerate()
                .filter(|(_, e)| e.conclusion.is_some())
                .map(|(i, _)| i)
                .collect();
            if concluded.len() > max_concluded {
                concluded.sort_by_key(|&i| std::cmp::Reverse(self.experiments[i].fit));
                let to_remove: std::collections::HashSet<usize> =
                    concluded[max_concluded..].iter().cloned().collect();
                let mut idx = 0;
                self.experiments.retain(|_| {
                    let keep = !to_remove.contains(&idx);
                    idx += 1;
                    keep
                });
            }
            let _ = active_count; // suppress unused warning
        }

        // Count active experiments
        let current_active = self
            .experiments
            .iter()
            .filter(|ex| ex.conclusion.is_none())
            .count();

        // Batch spawning: fill all available slots
        let slots_to_fill = self.config.max_active.saturating_sub(current_active);
        for _ in 0..slots_to_fill {
            // Every 10th experiment: random injection (not from any parent)
            let use_random = self.total_concluded > 0 && self.rng.gen_range(0..10) == 0;
            if use_random || self.elite_map.is_empty() {
                // Fresh random soup — explores new regions of behavior space
                let snap = self.experiments.first().map(|e| e.snap.clone());
                if let Some(mut snap) = snap {
                    self.create_soup(&mut snap);
                    self.add_experiment(snap, 0);
                }
            } else if let Some((mut snap, parent_id)) = self.select_parent() {
                self.mutate_snap(&mut snap);
                self.add_experiment(snap, parent_id);
            }
        }
    }

    fn mutate_probabilities(&mut self, probabilities: &mut ProbabilityTable) {
        let index = self.rng.gen_range(0..=self.config.max_probability_weight);
        let value = probabilities.entry(index).or_insert(0.0);
        let left = *value - self.config.probability_step;
        let right = *value + self.config.probability_step;
        *value = if left < 0.0 {
            right
        } else if right > 1.0 {
            left
        } else {
            [left, right][self.rng.gen_range(0..2)]
        };
    }

    fn mutate_snap(&mut self, snap: &mut Snap) {
        if self.config.frozen_rules {
            self.mutate_snap_frozen(snap);
            return;
        }
        // Apply 1-3 mutations for faster co-adaptation
        let num_mutations = match self.rng.gen_range(0..10) {
            0..=4 => 1,
            5..=7 => 2,
            _ => 3,
        };
        for _ in 0..num_mutations {
            match self.rng.gen_range(0..5) {
                0 => {
                    self.mutate_probabilities(&mut snap.rules.spawn);
                }
                1 => {
                    self.mutate_probabilities(&mut snap.rules.keep);
                }
                2 => {
                    let row_index = self.rng.gen_range(0..snap.rules.kernel.len());
                    let row = &mut snap.rules.kernel[row_index];
                    let candidates = row
                        .char_indices()
                        .filter(|(_, c)| c.is_numeric())
                        .collect::<Vec<_>>();
                    let (byte_offset, ch) = candidates[self.rng.gen_range(0..candidates.len())];
                    let other = if ch == '0' {
                        b'1'
                    } else if ch == '5' {
                        b'4'
                    } else {
                        [ch as u8 - 1, ch as u8 + 1][self.rng.gen_range(0..2)]
                    };
                    unsafe {
                        row.as_bytes_mut()[byte_offset] = other;
                    }
                }
                3 => {
                    // size change
                    let size_power = self.rng.gen_range(self.config.size_power.clone());
                    match snap.data {
                        Data::Random {
                            ref mut width,
                            ref mut height,
                            alive_ratio: _,
                        } => {
                            *width = 1 << size_power;
                            *height = 1 << size_power;
                        }
                        Data::Grid(_) => {
                            log::error!("Unable to change grid size");
                        }
                    }
                }
                _ => {
                    // boundary mode flip
                    snap.boundary = match snap.boundary {
                        BoundaryMode::Wrap => BoundaryMode::Dead,
                        BoundaryMode::Dead => BoundaryMode::Wrap,
                    };
                }
            }
        }
    }

    /// Mutation for frozen_rules mode: only mutate initial conditions.
    /// Balanced between exploration (new soups) and exploitation (local edits).
    fn mutate_snap_frozen(&mut self, snap: &mut Snap) {
        match self.rng.gen_range(0..10) {
            0..=2 => {
                // 30%: fresh random soup — pure exploration
                self.create_soup(snap);
            }
            3..=5 => {
                // 30%: focused flip — small perturbation near existing alive cells
                let count = self.rng.gen_range(1..=8);
                self.focused_flip(snap, count);
            }
            6..=7 => {
                // 20%: add a second small soup patch nearby — creates multi-center dynamics
                self.add_satellite_soup(snap);
            }
            _ => {
                // 20%: symmetric soup — bilateral or 4-fold symmetry
                // Many interesting patterns (pulsars, HWSS) have symmetry
                self.create_symmetric_soup(snap);
            }
        }
    }

    /// Create a small random "soup" (dense patch) centered in a larger grid.
    /// This gives gliders and other spaceships room to travel before wrapping.
    fn create_soup(&mut self, snap: &mut Snap) {
        use crate::grid::{Coordinates, Grid};

        let grid_power = self.rng.gen_range(self.config.size_power.clone());
        let grid_size = 1i32 << grid_power;
        // Mix of soup strategies:
        // - Standard census: 16×16 at 50% (what Catagolue uses)
        // - Methuselah sweet spot: 10-20 cells at 37.5%
        // - Dense small: 5-10 cells at 50% (good for finding small interesting patterns)
        let (soup_size, density): (i32, f32) = match self.rng.gen_range(0..10) {
            0..=3 => (16, 0.50),                                    // 40%: standard census
            4..=6 => (self.rng.gen_range(10..=20), self.rng.gen_range(0.30..0.45)), // 30%: methuselah
            7..=8 => (self.rng.gen_range(5..=10), 0.50),            // 20%: dense small
            _ => (self.rng.gen_range(20..=30), self.rng.gen_range(0.35..0.45)),  // 10%: large sparse
        };

        let mut grid = Grid::new(Coordinates {
            x: grid_size,
            y: grid_size,
        });

        let offset = (grid_size - soup_size) / 2;
        let cell_count = (soup_size as f32 * soup_size as f32 * density) as usize;
        for _ in 0..cell_count {
            let x = offset + self.rng.gen_range(0..soup_size);
            let y = offset + self.rng.gen_range(0..soup_size);
            grid.init(x, y);
        }

        snap.data = Data::unparse(&grid);
        snap.random_seed = self.rng.gen();
    }

    /// Add a second small soup patch near the existing alive cells.
    /// Creates multi-center initial conditions that can produce interesting collisions.
    fn add_satellite_soup(&mut self, snap: &mut Snap) {

        // Materialize first
        if let Data::Random { .. } = &snap.data {
            let mut sim_rng =
                <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(snap.random_seed);
            if let Ok(grid) = snap.data.parse(&mut sim_rng) {
                snap.data = Data::unparse(&grid);
            } else {
                self.create_soup(snap);
                return;
            }
        }

        if let Data::Grid(ref lines) = snap.data {
            let height = lines.len() as i32;
            let width = lines[0].len() as i32 * 4;

            // Find bounding box of existing alive cells
            let (mut minx, mut maxx, mut miny, mut maxy) = (width, 0i32, height, 0i32);
            for (y, line) in lines.iter().enumerate() {
                for (ci, ch) in line.chars().enumerate() {
                    let val = match ch {
                        '0' => 0,
                        '1'..='9' => ch as u8 - b'0',
                        'a'..='f' => 10 + ch as u8 - b'a',
                        _ => 0,
                    };
                    if val > 0 {
                        let x = ci as i32 * 4;
                        minx = minx.min(x);
                        maxx = maxx.max(x + 3);
                        miny = miny.min(y as i32);
                        maxy = maxy.max(y as i32);
                    }
                }
            }

            if minx >= maxx {
                self.create_soup(snap);
                return;
            }

            // Place a small (3-8 cell) satellite patch offset from the existing soup
            let satellite_size = self.rng.gen_range(3..=8);
            let offset_dist = self.rng.gen_range(5..=20);
            let angle = self.rng.gen_range(0..4);
            let (dx, dy) = match angle {
                0 => (offset_dist, 0),
                1 => (-offset_dist, 0),
                2 => (0, offset_dist),
                _ => (0, -offset_dist),
            };
            let cx = ((minx + maxx) / 2 + dx).clamp(satellite_size, width - satellite_size);
            let cy = ((miny + maxy) / 2 + dy).clamp(satellite_size, height - satellite_size);

            // Parse, add cells, unparse
            let mut sim_rng =
                <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(snap.random_seed);
            if let Ok(mut grid) = snap.data.parse(&mut sim_rng) {
                let count = (satellite_size as f32 * satellite_size as f32 * 0.5) as usize;
                for _ in 0..count {
                    let x = cx + self.rng.gen_range(-satellite_size..=satellite_size);
                    let y = cy + self.rng.gen_range(-satellite_size..=satellite_size);
                    if x >= 0 && x < width && y >= 0 && y < height {
                        grid.init(x, y);
                    }
                }
                snap.data = Data::unparse(&grid);
            }
        }
        snap.random_seed = self.rng.gen();
    }

    /// Create a soup with bilateral or 4-fold symmetry.
    /// Many interesting GoL patterns (pulsars, spaceships) have symmetry.
    fn create_symmetric_soup(&mut self, snap: &mut Snap) {
        use crate::grid::{Coordinates, Grid};

        let grid_power = self.rng.gen_range(self.config.size_power.clone());
        let grid_size = 1i32 << grid_power;
        let soup_half = self.rng.gen_range(4..=12);
        let density: f32 = self.rng.gen_range(0.30..0.50);
        let four_fold = self.rng.gen_range(0..3) == 0; // 33% chance of 4-fold symmetry

        let mut grid = Grid::new(Coordinates {
            x: grid_size,
            y: grid_size,
        });

        let cx = grid_size / 2;
        let cy = grid_size / 2;
        let cell_count = (soup_half as f32 * soup_half as f32 * density) as usize;

        for _ in 0..cell_count {
            let dx = self.rng.gen_range(0..soup_half);
            let dy = self.rng.gen_range(0..soup_half);
            // Place cell and its mirror(s)
            grid.init(cx + dx, cy + dy);
            grid.init(cx - dx - 1, cy + dy); // horizontal mirror
            if four_fold {
                grid.init(cx + dx, cy - dy - 1); // vertical mirror
                grid.init(cx - dx - 1, cy - dy - 1); // both
            }
        }

        snap.data = Data::unparse(&grid);
        snap.random_seed = self.rng.gen();
    }

    /// Flip cells near existing alive cells (structure-preserving mutation).
    /// Unlike `materialize_and_flip`, this targets the neighborhood of the soup.
    fn focused_flip(&mut self, snap: &mut Snap, count: usize) {
        // Materialize Random → Grid if needed
        if let Data::Random { .. } = &snap.data {
            let mut sim_rng =
                <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(snap.random_seed);
            if let Ok(grid) = snap.data.parse(&mut sim_rng) {
                snap.data = Data::unparse(&grid);
            } else {
                snap.random_seed = self.rng.gen();
                return;
            }
        }

        if let Data::Grid(ref mut lines) = snap.data {
            let height = lines.len();
            if height == 0 {
                return;
            }
            let chars_per_line = lines[0].len();
            if chars_per_line == 0 {
                return;
            }
            let width = chars_per_line * 4;

            // Find bounding box of alive cells, then flip within an expanded box
            let (mut minx, mut maxx, mut miny, mut maxy) = (width, 0usize, height, 0usize);
            for (y, line) in lines.iter().enumerate() {
                for (ci, ch) in line.chars().enumerate() {
                    let val = match ch {
                        '0' => 0,
                        '1'..='9' => ch as u8 - b'0',
                        'a'..='f' => 10 + ch as u8 - b'a',
                        _ => 0,
                    };
                    if val > 0 {
                        minx = minx.min(ci * 4);
                        maxx = maxx.max(ci * 4 + 3);
                        miny = miny.min(y);
                        maxy = maxy.max(y);
                    }
                }
            }

            if minx > maxx {
                return; // empty grid, nothing to flip near
            }

            // Expand bounding box by 3 cells in each direction (neighborhood)
            let pad = 3;
            let x0 = minx.saturating_sub(pad);
            let x1 = (maxx + pad).min(width - 1);
            let y0 = miny.saturating_sub(pad);
            let y1 = (maxy + pad).min(height - 1);

            for _ in 0..count {
                let x = self.rng.gen_range(x0..=x1);
                let y = self.rng.gen_range(y0..=y1);
                let char_idx = x / 4;
                let bit_idx = (x % 4) as u32;
                let ch = lines[y].as_bytes()[char_idx];
                let val = match ch {
                    b'0'..=b'9' => ch - b'0',
                    b'a'..=b'f' => 10 + ch - b'a',
                    b'A'..=b'F' => 10 + ch - b'A',
                    _ => continue,
                };
                let new_val = val ^ (1 << bit_idx);
                let new_ch = if new_val < 10 {
                    b'0' + new_val
                } else {
                    b'a' + new_val - 10
                };
                unsafe {
                    lines[y].as_bytes_mut()[char_idx] = new_ch;
                }
            }
        }
        snap.random_seed = self.rng.gen();
    }

}
