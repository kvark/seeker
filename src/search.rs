//! Outer-loop search (F2): MAP-Elites illumination over Flow-Lenia rule space.
//!
//! F1 turned a run into numbers; F2 uses those numbers to *find* rules instead
//! of hand-tuning them (the designer-in-the-loop trap M-γ exists to escape). We
//! run MAP-Elites: rather than optimize one objective, **illuminate** a 2D
//! behavior space — keep the highest-quality rule discovered in each behavior
//! cell — so the output is a map of *what regimes are reachable at all*, with a
//! concrete rule in every filled cell.
//!
//! - **Genome** — 7 continuous Flow-Lenia parameters (growth μ/σ, kernel ring
//!   peak/width, dt, critical mass θ_A, ramp exponent n). Kernel radius is fixed
//!   so evaluation cost is constant and comparable.
//! - **Behavior descriptors** (the map axes) come straight from the harness
//!   `RunSummary`: mean concentration (how localized matter becomes) × mean
//!   activity (dynamism — frozen vs. churning). These separate dead/uniform from
//!   still-life from lively-structured.
//! - **Quality** (elite tie-break within a cell) is a modest *liveness* score:
//!   structure persists, matter neither dissipates to uniform nor explodes to
//!   fill space, and the pattern keeps changing. This is an admitted written
//!   fitness — F2 is deliberately a search we drive; intrinsic selection is
//!   M-γ-2's job (the energy economy). The deliverable here is the filled map.
//!
//! Every genome is evaluated from the *same* fixed random soup, so differences
//! reflect the rule, not the seed.

use crate::flow_lenia::{FlowLeniaParams, KernelRing, World};
use crate::harness::{measure_run, RunSummary, Sample};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Number of searchable genes.
pub const N_GENES: usize = 7;
// Gene indices.
const MU: usize = 0;
const SIGMA: usize = 1;
const PEAK: usize = 2;
const WIDTH: usize = 3;
const DT: usize = 4;
const THETA: usize = 5;
const ALPHA: usize = 6;

/// A point in Flow-Lenia rule space.
#[derive(Clone, Debug, PartialEq)]
pub struct Genome {
    pub genes: [f32; N_GENES],
}

/// What the search rewards, and therefore which behavior it illuminates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Objective {
    /// Persistent, dynamic, non-degenerate structure. Map axis: activity.
    /// The default — a broad "what regimes exist at all" illumination.
    Liveness,
    /// Coherent, persistent, *moving* structure — gliders. Map axis: speed.
    /// This is what M-γ-1 needs: species that migrate and collide, so their
    /// localized genomes actually mix instead of sitting in static spots.
    Motility,
}

/// Per-gene search bounds.
#[derive(Clone, Debug)]
pub struct Bounds {
    pub lo: [f32; N_GENES],
    pub hi: [f32; N_GENES],
}

impl Default for Bounds {
    fn default() -> Self {
        //             μ      σ      peak   width  dt     θ_A    n
        let lo = [0.02f32, 0.003, 0.15, 0.03, 0.02, 0.5, 1.0];
        let hi = [0.40f32, 0.120, 0.95, 0.35, 0.30, 6.0, 4.0];
        Bounds { lo, hi }
    }
}

impl Genome {
    /// Uniformly random genome within bounds.
    pub fn random<R: Rng>(rng: &mut R, b: &Bounds) -> Self {
        let mut genes = [0.0f32; N_GENES];
        for (i, gene) in genes.iter_mut().enumerate() {
            *gene = rng.gen_range(b.lo[i]..=b.hi[i]);
        }
        Genome { genes }
    }

    /// Gaussian mutation: perturb each gene by `sigma × (hi−lo)`, clamped.
    pub fn mutate<R: Rng>(&self, rng: &mut R, b: &Bounds, sigma: f32) -> Self {
        let mut genes = self.genes;
        for (i, gene) in genes.iter_mut().enumerate() {
            let range = b.hi[i] - b.lo[i];
            *gene = (*gene + gaussian(rng) * sigma * range).clamp(b.lo[i], b.hi[i]);
        }
        Genome { genes }
    }

    /// Map the genome to substrate parameters (single channel, one ring).
    pub fn to_params(&self, kernel_radius: usize) -> FlowLeniaParams {
        let g = &self.genes;
        FlowLeniaParams {
            channels: 1,
            kernel_radius,
            rings: vec![KernelRing { peak: g[PEAK], width: g[WIDTH], weight: 1.0 }],
            growth_mu: g[MU],
            growth_sigma: g[SIGMA],
            dt: g[DT],
            theta_a: g[THETA],
            alpha_n: g[ALPHA],
            max_flow: 1.0,
        }
    }
}

/// How a single genome is evaluated.
#[derive(Clone, Debug)]
pub struct EvalConfig {
    pub grid_size: usize,
    pub kernel_radius: usize,
    pub steps: usize,
    pub sample_every: usize,
    pub threshold: f32,
    /// Seed for the fixed initial soup (shared by every genome).
    pub seed: u64,
    /// What to reward / illuminate.
    pub objective: Objective,
}

impl Default for EvalConfig {
    fn default() -> Self {
        // Sized for a tractable pure-CPU search. Scalar direct convolution at
        // radius 13 is the cost floor here — batching many worlds on the GPU is
        // F2's real throughput play (see docs/mgamma-plan.md); on CPU we keep the
        // grid and horizon modest and let parallel evaluation carry the load.
        EvalConfig {
            grid_size: 48,
            kernel_radius: 13,
            steps: 200,
            sample_every: 10,
            threshold: 0.05,
            seed: 20240703,
            objective: Objective::Liveness,
        }
    }
}

/// A genome plus everything measured about it.
#[derive(Clone, Debug)]
pub struct Evaluated {
    pub genome: Genome,
    pub summary: RunSummary,
    pub quality: f32,
    /// Behavior descriptor: (concentration, activity).
    pub bd: (f32, f32),
    /// Final-frame concentration and occupancy (for interpreting liveness).
    pub final_concentration: f32,
    pub final_occupied: f32,
}

/// Run one genome from the shared fixed soup and measure it.
pub fn evaluate(genome: &Genome, cfg: &EvalConfig) -> Evaluated {
    let params = genome.to_params(cfg.kernel_radius);
    let mut world = World::new(cfg.grid_size, cfg.grid_size, params);
    // Fixed initial condition: a central soup patch. Same for all genomes, so
    // behavior differences are attributable to the rule.
    let mut rng = StdRng::seed_from_u64(cfg.seed);
    let c = cfg.grid_size as f32 * 0.5;
    world.seed_random_patch(&mut rng, 0, c, c, cfg.grid_size as f32 / 3.0, 0.6);

    let (summary, samples) =
        measure_run(&mut world, cfg.steps, cfg.sample_every, cfg.threshold, 8.0);
    let (final_concentration, final_occupied) = samples
        .last()
        .map(|s: &Sample| (s.stats.concentration, s.stats.occupied_fraction))
        .unwrap_or((0.0, 0.0));
    // Quality and the map's y-descriptor both follow the objective; x stays
    // concentration (how localized matter is) for both, so the two maps are
    // directly comparable along their shared axis.
    let (quality, bd) = match cfg.objective {
        Objective::Liveness => (
            quality_liveness(&summary, final_concentration, final_occupied),
            (summary.mean_concentration, summary.mean_activity),
        ),
        Objective::Motility => (
            quality_motility(&summary, final_concentration, final_occupied),
            (summary.mean_concentration, summary.mean_speed),
        ),
    };
    Evaluated {
        genome: genome.clone(),
        summary,
        quality,
        bd,
        final_concentration,
        final_occupied,
    }
}

/// Liveness quality: reward persistent, dynamic, non-degenerate structure.
/// Dead (dissipated to uniform / collapsed) or exploded (fills the grid) → low.
fn quality_liveness(summary: &RunSummary, final_concentration: f32, final_occupied: f32) -> f32 {
    let alive = final_concentration > 0.08 && final_occupied > 0.003 && final_occupied < 0.9;
    let persistence = if alive { 1.0 } else { 0.05 };
    // Sustained change is good; the tanh keeps runaway chaos from dominating.
    let dynamism = (summary.mean_activity * 200.0).tanh();
    persistence * (0.2 + dynamism)
}

/// Motility quality: reward a *coherent, persistent, translating* structure — a
/// glider, not soup and not a static blob. Speed alone is untrustworthy (blob
/// matching can jump), so it is gated on the structure staying localized and
/// composed of few objects: real translation, not tracking noise.
fn quality_motility(summary: &RunSummary, final_concentration: f32, final_occupied: f32) -> f32 {
    let alive = final_concentration > 0.08 && final_occupied > 0.002 && final_occupied < 0.5;
    if !alive {
        return 0.02;
    }
    // Few coherent objects (a glider is ~1). Soup of many blobs is penalized.
    let coherence = if summary.mean_components <= 1.0 {
        1.0
    } else {
        (1.0 / summary.mean_components).max(0.1)
    };
    // Sustained center-of-mass drift, saturated so lucky spikes don't dominate.
    let speed = (summary.mean_speed * 2.0).tanh();
    0.05 + coherence * speed
}

/// Zero-mean, unit-variance Gaussian via Box–Muller.
fn gaussian<R: Rng>(rng: &mut R) -> f32 {
    let u1: f32 = rng.gen::<f32>().max(1e-7);
    let u2: f32 = rng.gen::<f32>();
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

/// Behavior-map geometry.
#[derive(Clone, Debug)]
pub struct MapConfig {
    pub res_x: usize,
    pub res_y: usize,
    /// Concentration axis range.
    pub x_range: (f32, f32),
    /// Activity axis range.
    pub y_range: (f32, f32),
}

impl Default for MapConfig {
    fn default() -> Self {
        MapConfig {
            res_x: 16,
            res_y: 16,
            x_range: (0.0, 0.6),
            y_range: (0.0, 0.02),
        }
    }
}

/// A MAP-Elites archive: one (best) elite per behavior cell.
pub struct MapElites {
    cfg: MapConfig,
    cells: Vec<Option<Evaluated>>,
}

impl MapElites {
    pub fn new(cfg: MapConfig) -> Self {
        let n = cfg.res_x * cfg.res_y;
        MapElites {
            cfg,
            cells: (0..n).map(|_| None).collect(),
        }
    }

    fn bin(v: f32, range: (f32, f32), res: usize) -> usize {
        let (lo, hi) = range;
        if hi <= lo {
            return 0;
        }
        let t = ((v - lo) / (hi - lo)).clamp(0.0, 0.999_999);
        (t * res as f32) as usize
    }

    fn cell_index(&self, bd: (f32, f32)) -> usize {
        let ix = Self::bin(bd.0, self.cfg.x_range, self.cfg.res_x);
        let iy = Self::bin(bd.1, self.cfg.y_range, self.cfg.res_y);
        iy * self.cfg.res_x + ix
    }

    /// Insert an evaluated genome, keeping it only if its cell is empty or it
    /// beats the incumbent's quality. Returns true if it was placed.
    pub fn insert(&mut self, e: Evaluated) -> bool {
        let idx = self.cell_index(e.bd);
        match &self.cells[idx] {
            Some(cur) if cur.quality >= e.quality => false,
            _ => {
                self.cells[idx] = Some(e);
                true
            }
        }
    }

    pub fn occupied(&self) -> impl Iterator<Item = &Evaluated> {
        self.cells.iter().filter_map(|c| c.as_ref())
    }

    pub fn coverage(&self) -> f32 {
        let filled = self.cells.iter().filter(|c| c.is_some()).count();
        filled as f32 / self.cells.len() as f32
    }

    pub fn filled(&self) -> usize {
        self.cells.iter().filter(|c| c.is_some()).count()
    }

    pub fn total_cells(&self) -> usize {
        self.cells.len()
    }

    /// Highest-quality elite in the whole map.
    pub fn best(&self) -> Option<&Evaluated> {
        self.occupied()
            .max_by(|a, b| a.quality.partial_cmp(&b.quality).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn config(&self) -> &MapConfig {
        &self.cfg
    }

    /// Row-major grid of occupancy for visualization; `cell(ix, iy)`.
    pub fn cell(&self, ix: usize, iy: usize) -> Option<&Evaluated> {
        self.cells[iy * self.cfg.res_x + ix].as_ref()
    }
}

/// Full MAP-Elites run configuration.
#[derive(Clone, Debug)]
pub struct SearchConfig {
    pub bounds: Bounds,
    pub eval: EvalConfig,
    pub map: MapConfig,
    /// Random genomes evaluated to seed the archive.
    pub init_batch: usize,
    /// Number of mutation generations.
    pub generations: usize,
    /// Mutants evaluated per generation.
    pub batch: usize,
    pub mutation_sigma: f32,
    pub seed: u64,
    /// Emit coverage progress to stderr.
    pub verbose: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            bounds: Bounds::default(),
            eval: EvalConfig::default(),
            map: MapConfig::default(),
            init_batch: 64,
            generations: 20,
            batch: 20,
            mutation_sigma: 0.12,
            seed: 1,
            verbose: false,
        }
    }
}

/// Evaluate a batch of genomes in parallel with scoped threads. Cost per genome
/// is essentially constant (grid × steps × kernel), so contiguous chunking is
/// balanced. Results are returned in input order.
fn parallel_eval(genomes: &[Genome], cfg: &EvalConfig) -> Vec<Evaluated> {
    let n = genomes.len();
    if n == 0 {
        return Vec::new();
    }
    let threads = std::thread::available_parallelism()
        .map(|x| x.get())
        .unwrap_or(4)
        .min(n);
    let chunk = n.div_ceil(threads);
    let mut out: Vec<Evaluated> = Vec::with_capacity(n);
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for t in 0..threads {
            let start = t * chunk;
            let end = ((t + 1) * chunk).min(n);
            if start >= end {
                break;
            }
            let slice = &genomes[start..end];
            handles.push(s.spawn(move || slice.iter().map(|g| evaluate(g, cfg)).collect::<Vec<_>>()));
        }
        for h in handles {
            out.extend(h.join().expect("eval thread panicked"));
        }
    });
    out
}

/// Run MAP-Elites illumination and return the filled archive.
pub fn map_elites(cfg: &SearchConfig) -> MapElites {
    let mut rng = StdRng::seed_from_u64(cfg.seed);
    let mut archive = MapElites::new(cfg.map.clone());

    // Initial random population.
    let init: Vec<Genome> = (0..cfg.init_batch)
        .map(|_| Genome::random(&mut rng, &cfg.bounds))
        .collect();
    for e in parallel_eval(&init, &cfg.eval) {
        archive.insert(e);
    }
    if cfg.verbose {
        eprintln!(
            "[map-elites] init: {}/{} cells filled",
            archive.filled(),
            archive.total_cells()
        );
    }

    // Mutation generations.
    for gen in 0..cfg.generations {
        // Sample parents from occupied cells.
        let parents: Vec<Genome> = archive.occupied().map(|e| e.genome.clone()).collect();
        if parents.is_empty() {
            break;
        }
        let children: Vec<Genome> = (0..cfg.batch)
            .map(|_| {
                let p = &parents[rng.gen_range(0..parents.len())];
                p.mutate(&mut rng, &cfg.bounds, cfg.mutation_sigma)
            })
            .collect();
        for e in parallel_eval(&children, &cfg.eval) {
            archive.insert(e);
        }
        if cfg.verbose && (gen + 1) % 10 == 0 {
            eprintln!(
                "[map-elites] gen {}/{}: {}/{} cells, best quality {:.3}",
                gen + 1,
                cfg.generations,
                archive.filled(),
                archive.total_cells(),
                archive.best().map(|e| e.quality).unwrap_or(0.0)
            );
        }
    }
    archive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genome_roundtrips_to_params() {
        let b = Bounds::default();
        let mut rng = StdRng::seed_from_u64(3);
        let g = Genome::random(&mut rng, &b);
        let p = g.to_params(13);
        assert_eq!(p.channels, 1);
        assert_eq!(p.kernel_radius, 13);
        assert_eq!(p.rings.len(), 1);
        assert!((p.growth_mu - g.genes[MU]).abs() < 1e-6);
        assert!((p.rings[0].peak - g.genes[PEAK]).abs() < 1e-6);
    }

    #[test]
    fn random_genome_respects_bounds() {
        let b = Bounds::default();
        let mut rng = StdRng::seed_from_u64(9);
        for _ in 0..200 {
            let g = Genome::random(&mut rng, &b);
            for i in 0..N_GENES {
                assert!(g.genes[i] >= b.lo[i] && g.genes[i] <= b.hi[i]);
            }
        }
    }

    #[test]
    fn mutation_stays_in_bounds() {
        let b = Bounds::default();
        let mut rng = StdRng::seed_from_u64(11);
        let g = Genome::random(&mut rng, &b);
        for _ in 0..500 {
            let m = g.mutate(&mut rng, &b, 0.5);
            for i in 0..N_GENES {
                assert!(m.genes[i] >= b.lo[i] && m.genes[i] <= b.hi[i], "gene {i} OOB");
            }
        }
    }

    #[test]
    fn evaluate_conserves_mass_and_produces_descriptor() {
        let cfg = EvalConfig {
            grid_size: 32,
            steps: 60,
            sample_every: 15,
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(5);
        let g = Genome::random(&mut rng, &Bounds::default());
        let e = evaluate(&g, &cfg);
        assert!(e.summary.mass_drift < 1e-4, "drift {}", e.summary.mass_drift);
        assert!(e.bd.0 >= 0.0 && e.bd.0 <= 1.0);
        assert!(e.bd.1 >= 0.0);
        assert!(e.quality >= 0.0);
    }

    #[test]
    fn map_binning_is_in_range() {
        let m = MapElites::new(MapConfig { res_x: 8, res_y: 8, x_range: (0.0, 1.0), y_range: (0.0, 1.0) });
        assert_eq!(MapElites::bin(-1.0, (0.0, 1.0), 8), 0);
        assert_eq!(MapElites::bin(2.0, (0.0, 1.0), 8), 7);
        assert_eq!(MapElites::bin(0.5, (0.0, 1.0), 8), 4);
        assert_eq!(m.total_cells(), 64);
    }

    #[test]
    fn insert_keeps_higher_quality() {
        let mut m = MapElites::new(MapConfig::default());
        let mk = |q: f32, bd: (f32, f32)| Evaluated {
            genome: Genome { genes: [0.0; N_GENES] },
            summary: RunSummary::default(),
            quality: q,
            bd,
            final_concentration: 0.0,
            final_occupied: 0.0,
        };
        assert!(m.insert(mk(0.5, (0.3, 0.01))));
        assert!(!m.insert(mk(0.4, (0.3, 0.01))), "lower quality, same cell");
        assert!(m.insert(mk(0.9, (0.3, 0.01))), "higher quality, same cell");
        assert_eq!(m.filled(), 1);
        assert!(m.insert(mk(0.1, (0.5, 0.015))), "different cell");
        assert_eq!(m.filled(), 2);
    }

    #[test]
    fn motility_quality_rewards_coherent_movement() {
        // A coherent glider (one blob, drifting) must score above an equally
        // persistent but stationary blob, and above a fast-but-fragmented soup.
        let mover = RunSummary { mean_speed: 1.0, mean_components: 1.0, ..RunSummary::default() };
        let still = RunSummary { mean_speed: 0.0, mean_components: 1.0, ..RunSummary::default() };
        let soup = RunSummary { mean_speed: 1.0, mean_components: 30.0, ..RunSummary::default() };
        // final_concentration/occupied chosen "alive" for all three.
        let q = |s: &RunSummary| quality_motility(s, 0.2, 0.05);
        assert!(q(&mover) > q(&still), "mover {} !> still {}", q(&mover), q(&still));
        assert!(q(&mover) > q(&soup), "mover {} !> soup {}", q(&mover), q(&soup));
    }

    #[test]
    fn motility_objective_uses_speed_descriptor() {
        // Under the motility objective the map's y-descriptor is mean_speed;
        // under liveness it is mean_activity. Same genome, different bd.y source.
        let mut rng = StdRng::seed_from_u64(5);
        let g = Genome::random(&mut rng, &Bounds::default());
        let base = EvalConfig { grid_size: 32, steps: 60, sample_every: 15, ..Default::default() };
        let live = evaluate(&g, &EvalConfig { objective: Objective::Liveness, ..base.clone() });
        let move_ = evaluate(&g, &EvalConfig { objective: Objective::Motility, ..base });
        assert_eq!(live.bd.1, live.summary.mean_activity);
        assert_eq!(move_.bd.1, move_.summary.mean_speed);
        // x-descriptor (concentration) is shared, so the maps are comparable.
        assert_eq!(live.bd.0, move_.bd.0);
    }

    #[test]
    fn motility_search_illuminates() {
        // The motility search runs and fills cells just like the liveness one.
        let cfg = SearchConfig {
            eval: EvalConfig {
                grid_size: 24,
                steps: 40,
                sample_every: 10,
                objective: Objective::Motility,
                ..Default::default()
            },
            map: MapConfig { res_x: 6, res_y: 6, x_range: (0.0, 0.6), y_range: (0.0, 3.0) },
            init_batch: 16,
            generations: 4,
            batch: 8,
            ..Default::default()
        };
        let archive = map_elites(&cfg);
        assert!(archive.filled() >= 1);
        assert!(archive.best().is_some());
    }

    #[test]
    fn map_elites_fills_cells() {
        // Small, fast search: just confirm the loop runs and illuminates.
        let cfg = SearchConfig {
            eval: EvalConfig { grid_size: 24, steps: 40, sample_every: 10, ..Default::default() },
            map: MapConfig { res_x: 6, res_y: 6, ..Default::default() },
            init_batch: 16,
            generations: 4,
            batch: 8,
            ..Default::default()
        };
        let archive = map_elites(&cfg);
        assert!(archive.filled() >= 1, "search should fill at least one cell");
        assert!(archive.best().is_some());
    }
}
