use crate::analysis;
use crate::grid::BoundaryMode;
use crate::sim::{Conclusion, Data, Probability, ProbabilityTable, Simulation, Snap, Weight};
use rand::{rngs::ThreadRng, Rng as _};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash as _, Hasher},
    io::Write as _,
    mem,
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
    /// Recent conclusion signature hashes for novelty tracking.
    novelty_hashes: Vec<u64>,
    /// Number of early discards so far.
    pub early_discards: usize,
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
            novelty_hashes: Vec::new(),
            early_discards: 0,
        }
    }

    pub fn experiments(&self) -> &[Experiment] {
        &self.experiments
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

    pub fn update(&mut self) {
        while let Ok(status) = self.receiver.try_recv() {
            let max_fit = self
                .experiments
                .iter()
                .map(|exp| exp.fit)
                .max()
                .unwrap_or_default();

            let experiment = self
                .experiments
                .iter_mut()
                .find(|exp| exp.id == status.experiment_id)
                .unwrap();
            assert!(experiment.conclusion.is_none());
            experiment.steps = status.step;

            // Early discard: check if experiment is boring
            if status.conclusion.is_none()
                && status.step >= EARLY_DISCARD_WARMUP
                && status.step % EARLY_DISCARD_INTERVAL < UPDATE_FREQUENCY
            {
                if status.alive_ratio_variance < BORING_VARIANCE_THRESHOLD {
                    experiment.boring_streak += 1;
                    if experiment.boring_streak >= BORING_STREAK_LIMIT {
                        writeln!(
                            self.log,
                            "Early discard E[{}] at step {} (boring: var={:.6})",
                            status.experiment_id, status.step, status.alive_ratio_variance
                        )
                        .unwrap();
                        experiment.abort.store(true, Ordering::Relaxed);
                        self.early_discards += 1;
                    }
                } else {
                    experiment.boring_streak = 0;
                }
            }

            if let Some(conclusion) = status.conclusion {
                writeln!(
                    self.log,
                    "Conclude E[{}] as {} at step {}",
                    status.experiment_id, conclusion, status.step
                )
                .unwrap();

                experiment.fit = match conclusion {
                    Conclusion::Extinct | Conclusion::Saturate => {
                        mem::size_of::<usize>() * 8 - status.step.leading_zeros() as usize
                    }
                    Conclusion::Done(ref state, ref snap) => {
                        let fit = if self.config.frozen_rules {
                            // GoL fitness: analyze final grid patterns directly
                            let mut dummy_rng =
                                <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(0);
                            let analysis_score =
                                if let Ok(grid) = snap.data.parse(&mut dummy_rng) {
                                    let (_, summary) = analysis::analyze_grid(&grid);
                                    let mut score = 0usize;
                                    // Reward unique pattern diversity
                                    score += summary.unique_patterns.min(20) * 2;
                                    // Big reward for spaceships (gliders!)
                                    score += summary.spaceships.len() * 30;
                                    // Reward higher-period oscillators
                                    for &p in &summary.oscillators {
                                        score += if p > 2 { p.min(20) } else { 1 };
                                    }
                                    // Composability: independent classified components
                                    score += summary.composability_score();
                                    score
                                } else {
                                    0
                                };

                            let base = 20usize;
                            let var_score =
                                (state.alive_ratio_variance * 2000.0).min(20.0) as usize;
                            // Late stabilization = interesting transients
                            let late_score =
                                (state.stabilized_step as f32 / 100.0).min(20.0) as usize;
                            // Sustained birth rate = ongoing production (guns, puffers)
                            let birth_score =
                                (state.birth_rate_average * 5000.0).min(20.0) as usize;
                            // Spatial structure = localized dynamics (guns, factories)
                            let spatial_score =
                                (state.spatial_variance_average * 5000.0).min(20.0) as usize;
                            // Narrative richness = structural events during simulation
                            let narrative_score =
                                state.narrative.richness().min(100) / 5;
                            base + var_score + late_score + analysis_score + birth_score + spatial_score + narrative_score
                        } else {
                            // Reward sustained birth rate for non-frozen mode too
                            let base = 100 - (60.0 * state.alive_ratio_average) as usize;
                            let birth_bonus =
                                (state.birth_rate_average * 3000.0).min(15.0) as usize;
                            let spatial_bonus =
                                (state.spatial_variance_average * 3000.0).min(15.0) as usize;
                            let narrative_bonus =
                                state.narrative.richness().min(100) / 7;
                            base + birth_bonus + spatial_bonus + narrative_bonus
                        };
                        // Novelty penalty: hash the conclusion signature and
                        // penalize experiments that look like recent ones.
                        let sig_hash = {
                            let mut h = DefaultHasher::new();
                            // Quantize key stats into buckets for similarity detection
                            let alive_bucket = (state.alive_ratio_average * 100.0) as u32;
                            let var_bucket = (state.alive_ratio_variance * 10000.0) as u32;
                            let period_bucket = state.period;
                            alive_bucket.hash(&mut h);
                            var_bucket.hash(&mut h);
                            period_bucket.hash(&mut h);
                            h.finish()
                        };
                        let duplicates = self
                            .novelty_hashes
                            .iter()
                            .filter(|&&h| h == sig_hash)
                            .count();
                        // Each duplicate halves the novelty bonus (up to 30 points penalty)
                        let novelty_penalty = (duplicates * 10).min(30);
                        self.novelty_hashes.push(sig_hash);
                        // Keep novelty window bounded
                        const NOVELTY_WINDOW: usize = 100;
                        if self.novelty_hashes.len() > NOVELTY_WINDOW {
                            self.novelty_hashes
                                .drain(0..self.novelty_hashes.len() - NOVELTY_WINDOW);
                        }

                        let fit = fit.saturating_sub(novelty_penalty);
                        if fit > max_fit {
                            let name = format!("e{}-{}.ron", experiment.id, status.step);
                            let file = fs::File::create(self.active_dir.join(name)).unwrap();
                            ron::ser::to_writer_pretty(
                                file,
                                snap,
                                ron::ser::PrettyConfig::default(),
                            )
                            .unwrap();
                        }
                        fit
                    }
                    Conclusion::Crash => 0,
                };
                experiment.conclusion = Some(conclusion);
            }
        }

        // Prune concluded experiments to keep only the best `max_archive`.
        // Active (in-flight) experiments are always retained.
        let concluded_count = self
            .experiments
            .iter()
            .filter(|ex| ex.conclusion.is_some())
            .count();
        if concluded_count > self.config.max_archive {
            // Find the fitness threshold: sort concluded fitnesses descending,
            // keep the top max_archive.
            let mut concluded_fits: Vec<usize> = self
                .experiments
                .iter()
                .filter(|ex| ex.conclusion.is_some())
                .map(|ex| ex.fit)
                .collect();
            concluded_fits.sort_unstable_by(|a, b| b.cmp(a));
            let min_fit = concluded_fits[self.config.max_archive - 1];
            let mut kept = 0;
            self.experiments.retain(|ex| {
                if ex.conclusion.is_none() {
                    return true; // always keep active
                }
                if ex.fit >= min_fit && kept < self.config.max_archive {
                    kept += 1;
                    true
                } else {
                    false
                }
            });
        }

        let mut total_fit = 0;
        let mut current_active = 0;
        for ex in self.experiments.iter() {
            if ex.conclusion.is_some() {
                total_fit += ex.fit
            } else {
                current_active += 1;
            }
        }

        if current_active < self.config.max_active {
            let parent = if total_fit != 0 {
                let mut cutoff = self.rng.gen_range(0..total_fit);
                self.experiments
                    .iter_mut()
                    .find(|ex| {
                        if ex.conclusion.is_some() {
                            if cutoff < ex.fit {
                                true
                            } else {
                                cutoff -= ex.fit;
                                false
                            }
                        } else {
                            false
                        }
                    })
                    .unwrap()
            } else {
                let index = self.rng.gen_range(0..self.experiments.len());
                &mut self.experiments[index]
            };

            let mut snap = parent.snap.clone();
            let parent_id = parent.id;
            self.mutate_snap(&mut snap);
            self.add_experiment(snap, parent_id);
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
    /// Uses "soup in a box" approach — small dense random patch in a larger grid.
    /// Applies 1-3 mutations (Poisson-ish) for faster co-adaptation.
    fn mutate_snap_frozen(&mut self, snap: &mut Snap) {
        // Number of mutations: 1 (50%), 2 (30%), 3 (20%)
        let num_mutations = match self.rng.gen_range(0..10) {
            0..=4 => 1,
            5..=7 => 2,
            _ => 3,
        };
        for _ in 0..num_mutations {
            match self.rng.gen_range(0..10) {
                0..=5 => {
                    // 60%: create a new random soup (small patch in larger grid)
                    self.create_soup(snap);
                }
                6..=8 => {
                    // 30%: flip random cells in existing grid (exploitation)
                    let count = self.rng.gen_range(1..=5);
                    self.materialize_and_flip(snap, count);
                }
                _ => {
                    // 10%: just change the random seed (same pattern, different RNG trajectory)
                    snap.random_seed = self.rng.gen();
                }
            }
        }
    }

    /// Create a small random "soup" (dense patch) centered in a larger grid.
    /// This gives gliders and other spaceships room to travel before wrapping.
    fn create_soup(&mut self, snap: &mut Snap) {
        use crate::grid::{Coordinates, Grid};

        let grid_power = self.rng.gen_range(self.config.size_power.clone());
        let grid_size = 1i32 << grid_power;
        // Soup size: 8-20 cells on a side
        let soup_size = self.rng.gen_range(8..=20);
        let density: f32 = self.rng.gen_range(0.3..0.5);

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

    /// Convert Data::Random to Data::Grid (materializing the actual cells),
    /// then flip `count` random cells.
    fn materialize_and_flip(&mut self, snap: &mut Snap, count: usize) {
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

        // Flip random cells in the hex-encoded grid
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

            for _ in 0..count {
                let x = self.rng.gen_range(0..width);
                let y = self.rng.gen_range(0..height);
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
                // Safety: we're replacing one ASCII byte with another ASCII byte
                unsafe {
                    lines[y].as_bytes_mut()[char_idx] = new_ch;
                }
            }
        }
    }
}
