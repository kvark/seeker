//! Flow-Lenia substrate (M-γ-0): continuous, mass-conserving cellular dynamics.
//!
//! This retires the discrete B3/S23 lineage. State is a continuous, multi-channel
//! concentration field `A_i(x) ∈ [0, 1]`. One step:
//!
//! 1. Convolve a radial (Lenia) kernel with each channel → neighborhood potential.
//! 2. Map the potential through a Gaussian growth function → an **affinity** field
//!    `U_i ∈ [-1, 1]`. Unlike classic Lenia, growth is *not added* to the state;
//!    it only shapes where matter wants to flow.
//! 3. Assemble a flow vector `F_i = (1-α)∇U_i − α∇A_Σ`, where `A_Σ` is total local
//!    mass and `α(x)` ramps in the mass-regulation (anti-crowding) term as `A_Σ`
//!    approaches a critical mass `θ_A`. Gradients via Sobel.
//! 4. Transport matter along `F_i` with **reintegration tracking** (bilinear
//!    scatter): each cell's mass lands on a unit box centered at `p + dt·F`, split
//!    across the four overlapped cells. The split weights sum to 1, so **total mass
//!    is conserved exactly** — the defining invariant of this substrate, and the
//!    M-γ-0 gate (see `CLAUDE.md`).
//!
//! Because mass is only moved, never created, total mass is a constant of motion
//! fixed by the initial condition; structure arises from redistribution alone.
//!
//! This is a CPU reference. It plays the role `sim.rs` played for the discrete
//! engine: a correct, testable ground truth to validate a later `blade-graphics`
//! GPU port against.

use rand::Rng;

/// One Gaussian ring of a Lenia kernel, expressed in normalized-radius space
/// (distance from center divided by the kernel radius `R`, in `(0, 1]`).
#[derive(Clone, Copy, Debug)]
pub struct KernelRing {
    /// Ring center, as a fraction of the kernel radius.
    pub peak: f32,
    /// Gaussian width of the ring.
    pub width: f32,
    /// Relative amplitude of the ring.
    pub weight: f32,
}

/// Parameters of a single-species Flow-Lenia world.
#[derive(Clone, Debug)]
pub struct FlowLeniaParams {
    /// Number of concentration channels.
    pub channels: usize,
    /// Kernel radius `R`, in cells.
    pub kernel_radius: usize,
    /// Rings composing the (shared) radial kernel.
    pub rings: Vec<KernelRing>,
    /// Growth-function center `μ`.
    pub growth_mu: f32,
    /// Growth-function width `σ`.
    pub growth_sigma: f32,
    /// Integration timestep `dt` (also the advection scale).
    pub dt: f32,
    /// Critical mass `θ_A` for the `α` mass-regulation ramp.
    pub theta_a: f32,
    /// Exponent `n` of the `α` ramp; `n > 1` keeps affinity dominant over a
    /// wider mass range before regulation kicks in.
    pub alpha_n: f32,
    /// Clamp on advection displacement `|F|·dt`, in cells. Mass conservation
    /// holds regardless of this bound (bilinear splat always conserves); the
    /// clamp only keeps a single step's transport local for fidelity.
    pub max_flow: f32,
}

impl Default for FlowLeniaParams {
    /// A single-channel, Orbium-flavored configuration: a smooth localized
    /// pattern-forming regime. Not exactly tuned to any cataloged creature —
    /// exact SLP reproduction is a calibration task, not a default.
    fn default() -> Self {
        Self {
            channels: 1,
            kernel_radius: 13,
            rings: vec![KernelRing { peak: 0.5, width: 0.15, weight: 1.0 }],
            growth_mu: 0.15,
            growth_sigma: 0.017,
            dt: 0.1,
            theta_a: 3.0,
            alpha_n: 2.0,
            max_flow: 1.0,
        }
    }
}

/// Parameters of the **energy economy** (M-γ-2) — the M-γ research contribution.
///
/// Off by default. When enabled (`World::enable_energy`), a scalar energy field
/// `E(x)` ("food") is layered on top of the mass-conserving substrate and coupled
/// to the dynamics three ways:
///
/// 1. **Gate.** Matter's organizing affinity (the Lenia growth field, whose
///    gradient drives transport) is scaled by a saturating function of local
///    energy, `g(E) = E/(E+K)`. Where energy is scarce the affinity flow vanishes
///    and only the ever-present anti-crowding term remains, so unfed matter loses
///    the pull that holds it together and disperses — death-by-dispersal, the
///    soft precursor to the explicit detritus death of M-γ-3 ([`DetritusParams`]).
/// 2. **Consumption.** Building structure costs energy: wherever mass accumulates
///    (`ΔA > 0` after transport), energy is drawn down proportionally.
/// 3. **Maintenance.** Merely staying organized costs energy proportional to local
///    mass, every step.
///
/// Energy is replenished by localized renewable **sources** (`add_source`) and
/// spreads by **diffusion**, so exploitable gradients form. Energy is *not*
/// conserved (it is a flow, sourced and dissipated); **mass remains conserved**
/// across the matter channels exactly as before — gating only reshapes flow.
/// Explicit death/recycling (matter moving into an inert detritus channel and
/// back) is the M-γ-3 layer ([`DetritusParams`], `enable_detritus`), off by
/// default here. The point: patterns whose localized behavior lets them find and
/// hold energy persist; others starve. Selection becomes a consequence of the economy,
/// not a fitness function we wrote.
#[derive(Clone, Debug)]
pub struct EnergyParams {
    /// Diffusion coefficient `D` for the energy Laplacian, per step. Spreads
    /// energy away from sources so gradients form. Keep `D ≤ 0.25` for stability
    /// of the explicit 5-point update. `0` = no diffusion.
    pub diffusion: f32,
    /// Per-cell storage cap. Sources stop injecting past this; diffusion and
    /// spending are clamped to `[0, capacity]`.
    pub capacity: f32,
    /// Half-saturation constant `K` of the growth gate `g(E) = E/(E+K)`. Growth
    /// affinity runs at half strength when local energy equals `K`; small `K`
    /// makes the economy permissive, large `K` makes energy the binding constraint.
    pub gate_half: f32,
    /// Energy spent per unit of matter *accumulated* at a cell in a step
    /// (`ΔE −= consume·max(ΔA, 0)`): the price of building structure.
    pub consume: f32,
    /// Energy drained per unit of local mass per step (`ΔE −= maintain·A_Σ`): the
    /// standing cost of staying organized.
    pub maintain: f32,
}

impl Default for EnergyParams {
    /// A v0 regime (multiplicative gate + renewable sources, the plan's default):
    /// energy is a real but not overwhelming constraint, gradients form, and a
    /// well-fed blob can pay its upkeep while an unfed one cannot.
    fn default() -> Self {
        Self {
            diffusion: 0.15,
            capacity: 4.0,
            gate_half: 0.5,
            consume: 0.15,
            maintain: 0.004,
        }
    }
}

/// Internal state of the energy economy: parameters, the live energy field, the
/// (static) per-cell source injection map, and a diffusion scratch buffer.
struct Energy {
    params: EnergyParams,
    /// Energy concentration `E(x)`, row-major `w×h`.
    field: Vec<f32>,
    /// Per-cell renewable injection rate `S(x)` added each step (pre-cap).
    source: Vec<f32>,
    /// Scratch for the out-of-place diffusion pass.
    scratch: Vec<f32>,
}

/// Parameters of the **closed-loop detritus cycle** (M-γ-3) — death and recycling
/// that keep the world turning over instead of freezing.
///
/// M-γ-2 starved matter to inertness ("death by dispersal") but nothing came back:
/// once the energy ran out the world just stopped. M-γ-3 closes the loop with an
/// inert **detritus** channel and two fluxes, both gated on the energy economy:
///
/// 1. **Death.** Where energy is scarce, live matter converts to detritus in place
///    at a rate scaled by starvation `s = 1 − g(E) = K/(E+K)` (the complement of
///    the growth gate — the same energy shortage that stops growth now also kills):
///    `Δdet = death_rate · s · A_live`. Detritus does **not** flow, convolve, or
///    grow — it is dead mass sitting where its owner died.
/// 2. **Recycling.** Detritus decomposes back into the live channel at
///    `recycle_matter · Det` per step, and each unit that decomposes **releases
///    energy** into `E` (`recycle_energy` per unit matter) — dead matter becomes
///    food, so the world is no longer solely fed by the external vent.
///
/// **Conservation.** Death and recycling only ever *move* matter between the live
/// channel and detritus, so `Σ A_live + Σ Det` is conserved exactly (the released
/// energy is a byproduct in the non-conserved `E` field, like a source). This is
/// the M-γ invariant extended to {live + detritus}, and a unit test pins it.
///
/// Single matter channel, and detritus carries no genome (M-γ-1) in v0 — returned
/// matter adopts whatever genome sits at its cell. Off unless `enable_detritus`.
#[derive(Clone, Debug)]
pub struct DetritusParams {
    /// Max fraction of local live mass that dies to detritus per step, at full
    /// starvation (`E → 0`). Scaled down by available energy via `s = K/(E+K)`.
    pub death_rate: f32,
    /// Fraction of local detritus that decomposes back into the live channel per
    /// step. Small values make detritus a slow-release matter reservoir.
    pub recycle_matter: f32,
    /// Energy released into `E` per unit of matter decomposed (the "food" a
    /// decomposing corpse yields). `0` = matter recycles but releases no energy.
    pub recycle_energy: f32,
}

impl Default for DetritusParams {
    /// A v0 regime: starved matter dies over ~tens of steps, detritus is a slow
    /// reservoir that trickles matter back and releases food as it rots — fast
    /// enough to reseed regrowth, slow enough to leave a visible detritus pool.
    fn default() -> Self {
        Self { death_rate: 0.05, recycle_matter: 0.01, recycle_energy: 0.5 }
    }
}

/// Internal state of the detritus cycle: parameters and the inert detritus field.
struct Detritus {
    params: DetritusParams,
    /// Dead mass `Det(x)`, row-major `w×h`. Inert: never flows, grows, or convolves.
    field: Vec<f32>,
}

/// A precomputed kernel tap: an integer offset and its normalized weight.
#[derive(Clone, Copy)]
struct Tap {
    dx: i32,
    dy: i32,
    w: f32,
}

/// Localized parameters (M-γ-1) — the "genome" carried by the matter itself.
///
/// When enabled (`World::enable_genome`), the growth center `μ` and width `σ`
/// become per-cell fields that **advect with the mass** through the same
/// reintegration-tracking transport: each cell's growth mapping uses its *local*
/// `(μ, σ)`, and when transported mass lands on a cell the new parameters are the
/// **mass-weighted average** of what arrived. Heterogeneous rules therefore
/// coexist, compete, and mix in one field — this is what makes the substrate
/// multi-species.
///
/// Single matter channel only for v0 (see `enable_genome`): the genome rides
/// channel 0's flow. Multi-channel localization needs per-channel genomes (mass
/// in different channels flows differently) — deferred.
///
/// **Watch the variance (risk #3).** Mass-weighted averaging on merge is a
/// low-pass filter on the gene pool; left unchecked it homogenizes species into
/// their blend. `mu_stats` exposes the mass-weighted mean/variance so the collapse
/// is measurable rather than assumed.
struct Genome {
    /// Per-cell growth center `μ`, row-major `w×h`.
    mu: Vec<f32>,
    /// Per-cell growth width `σ`.
    sigma: Vec<f32>,
    /// Advection accumulators: mass-weighted sums scattered during transport,
    /// divided by the new per-cell mass to recover the averaged parameter.
    mu_acc: Vec<f32>,
    sigma_acc: Vec<f32>,
}

/// A continuous, mass-conserving Flow-Lenia world.
pub struct World {
    w: usize,
    h: usize,
    params: FlowLeniaParams,
    /// Channel-major concentration: `a[c * w * h + y * w + x]`.
    a: Vec<f32>,
    /// Shared radial kernel, as normalized taps (weights sum to 1).
    kernel: Vec<Tap>,
    // Scratch buffers reused across steps to avoid per-step allocation.
    potential: Vec<f32>, // per-channel affinity U_i
    total: Vec<f32>,     // A_Σ (pre-transport, this step)
    scratch: Vec<f32>,   // reintegration target for one channel
    /// Optional energy economy (M-γ-2). `None` = pure Flow-Lenia (M-γ-0/1).
    energy: Option<Energy>,
    /// Optional closed-loop detritus cycle (M-γ-3). `None` = no death/recycling.
    detritus: Option<Detritus>,
    /// Optional localized parameters (M-γ-1). `None` = single global rule.
    genome: Option<Genome>,
}

impl World {
    /// Create an empty world of `w × h` cells with the given parameters.
    pub fn new(w: usize, h: usize, params: FlowLeniaParams) -> Self {
        assert!(w > 0 && h > 0 && params.channels > 0);
        let kernel = build_kernel(&params);
        let cells = w * h;
        World {
            a: vec![0.0; cells * params.channels],
            potential: vec![0.0; cells * params.channels],
            total: vec![0.0; cells],
            scratch: vec![0.0; cells],
            energy: None,
            detritus: None,
            genome: None,
            kernel,
            w,
            h,
            params,
        }
    }

    pub fn width(&self) -> usize {
        self.w
    }

    pub fn height(&self) -> usize {
        self.h
    }

    pub fn params(&self) -> &FlowLeniaParams {
        &self.params
    }

    /// Enable the energy economy (M-γ-2) with the given parameters. The energy
    /// field starts empty; add sources with [`add_source`](Self::add_source) and
    /// optionally an initial charge with [`charge_energy`](Self::charge_energy).
    /// Idempotent-ish: re-enabling replaces the parameters but resets the field.
    pub fn enable_energy(&mut self, params: EnergyParams) {
        let cells = self.w * self.h;
        self.energy = Some(Energy {
            params,
            field: vec![0.0; cells],
            source: vec![0.0; cells],
            scratch: vec![0.0; cells],
        });
    }

    /// Whether the energy economy is active. When `false`, this is pure
    /// Flow-Lenia — the A/B baseline every experiment can compare against.
    pub fn energy_enabled(&self) -> bool {
        self.energy.is_some()
    }

    /// Read-only view of the energy field, or `None` if the economy is disabled.
    /// Substrate-agnostic — feed straight into the measurement harness.
    pub fn energy_field(&self) -> Option<&[f32]> {
        self.energy.as_ref().map(|e| e.field.as_slice())
    }

    /// Total energy currently stored across the world (not conserved; sourced and
    /// dissipated). `None` if the economy is disabled.
    pub fn total_energy(&self) -> Option<f64> {
        self.energy
            .as_ref()
            .map(|e| e.field.iter().map(|&v| v as f64).sum())
    }

    /// Add a localized renewable energy source: a smooth Gaussian bump of
    /// per-step injection rate centered at `(cx, cy)`, peak `rate`, width
    /// `radius`. Accumulates with existing sources. No-op if energy is disabled.
    pub fn add_source(&mut self, cx: f32, cy: f32, radius: f32, rate: f32) {
        let (w, h) = (self.w, self.h);
        let Some(energy) = self.energy.as_mut() else { return };
        let inv = 1.0 / (2.0 * radius * radius);
        for y in 0..h {
            for x in 0..w {
                let dx = torus_delta(x as f32, cx, w as f32);
                let dy = torus_delta(y as f32, cy, h as f32);
                let g = rate * (-(dx * dx + dy * dy) * inv).exp();
                energy.source[y * w + x] += g;
            }
        }
    }

    /// Set a uniform initial energy charge across the field (clamped to capacity).
    /// No-op if energy is disabled.
    pub fn charge_energy(&mut self, level: f32) {
        if let Some(energy) = self.energy.as_mut() {
            let cap = energy.params.capacity;
            for v in energy.field.iter_mut() {
                *v = level.clamp(0.0, cap);
            }
        }
    }

    /// Enable the **closed-loop detritus cycle** (M-γ-3): starved matter dies into
    /// an inert detritus channel that decomposes back into the live channel and
    /// releases energy as it rots. See [`DetritusParams`]. Requires the energy
    /// economy (death is triggered by energy scarcity). Re-enabling resets the
    /// detritus field.
    ///
    /// # Panics
    /// If the energy economy is disabled, or the world has more than one matter
    /// channel (v0 is single-channel, matching the genome and metabolism layers).
    pub fn enable_detritus(&mut self, params: DetritusParams) {
        assert!(
            self.energy.is_some(),
            "detritus recycling (M-γ-3) needs the energy economy (death is energy-gated)"
        );
        assert_eq!(
            self.params.channels, 1,
            "detritus recycling (M-γ-3) is single matter channel in v0"
        );
        self.detritus = Some(Detritus { params, field: vec![0.0; self.w * self.h] });
    }

    /// Whether the detritus cycle (M-γ-3) is active.
    pub fn detritus_enabled(&self) -> bool {
        self.detritus.is_some()
    }

    /// Read-only view of the inert detritus field, or `None` if M-γ-3 is disabled.
    pub fn detritus_field(&self) -> Option<&[f32]> {
        self.detritus.as_ref().map(|d| d.field.as_slice())
    }

    /// Total dead mass currently held as detritus, or `None` if M-γ-3 is disabled.
    /// The conserved M-γ-3 quantity is `total_mass() + total_detritus()`.
    pub fn total_detritus(&self) -> Option<f64> {
        self.detritus
            .as_ref()
            .map(|d| d.field.iter().map(|&v| v as f64).sum())
    }

    /// Enable **localized parameters** (M-γ-1): the growth genome `(μ, σ)` becomes
    /// a per-cell field that advects with the mass. The field is initialized to
    /// the world's global `growth_mu`/`growth_sigma`; paint distinct species with
    /// [`paint_genome`](Self::paint_genome) or [`seed_species`](Self::seed_species).
    ///
    /// # Panics
    /// If the world has more than one matter channel — v0 rides channel 0's flow;
    /// per-channel genomes are deferred.
    pub fn enable_genome(&mut self) {
        assert_eq!(
            self.params.channels, 1,
            "localized parameters (M-γ-1) are single-channel in v0"
        );
        let cells = self.w * self.h;
        self.genome = Some(Genome {
            mu: vec![self.params.growth_mu; cells],
            sigma: vec![self.params.growth_sigma; cells],
            mu_acc: vec![0.0; cells],
            sigma_acc: vec![0.0; cells],
        });
    }

    /// Whether localized parameters are active.
    pub fn genome_enabled(&self) -> bool {
        self.genome.is_some()
    }

    /// Read-only view of the per-cell growth-center field `μ(x)`, or `None`.
    pub fn mu_field(&self) -> Option<&[f32]> {
        self.genome.as_ref().map(|g| g.mu.as_slice())
    }

    /// Read-only view of the per-cell growth-width field `σ(x)`, or `None`.
    pub fn sigma_field(&self) -> Option<&[f32]> {
        self.genome.as_ref().map(|g| g.sigma.as_slice())
    }

    /// Paint a species' genome: hard-set the local growth `(μ, σ)` for every cell
    /// within `radius` of `(cx, cy)`. Seed matter with the same footprint so the
    /// genome has mass to ride. No-op if the genome is disabled.
    pub fn paint_genome(&mut self, cx: f32, cy: f32, radius: f32, mu: f32, sigma: f32) {
        let (w, h) = (self.w, self.h);
        let Some(g) = self.genome.as_mut() else { return };
        for y in 0..h {
            for x in 0..w {
                let dx = torus_delta(x as f32, cx, w as f32);
                let dy = torus_delta(y as f32, cy, h as f32);
                if dx * dx + dy * dy <= radius * radius {
                    let idx = y * w + x;
                    g.mu[idx] = mu;
                    g.sigma[idx] = sigma;
                }
            }
        }
    }

    /// Convenience for the multi-species demo: paint a species genome *and* seed a
    /// matching Gaussian blob of matter on channel 0 in one call.
    pub fn seed_species(&mut self, cx: f32, cy: f32, radius: f32, amp: f32, mu: f32, sigma: f32) {
        self.paint_genome(cx, cy, radius * 1.5, mu, sigma);
        self.seed_blob(0, cx, cy, radius, amp);
    }

    /// Mass-weighted mean and variance of the growth center `μ` over the occupied
    /// matter — the M-γ-1 diversity signal. Variance well above zero means
    /// multiple rules coexist; variance collapsing toward zero is the gene pool
    /// homogenizing (risk #3). Returns `(mean, variance)`, or `None` if the genome
    /// is disabled. `(0, 0)` when there is no mass.
    pub fn mu_stats(&self) -> Option<(f32, f32)> {
        let g = self.genome.as_ref()?;
        let cells = self.w * self.h;
        let mut m = 0.0f64;
        let mut mean = 0.0f64;
        for i in 0..cells {
            let a = self.a[i] as f64;
            m += a;
            mean += a * g.mu[i] as f64;
        }
        if m <= 0.0 {
            return Some((0.0, 0.0));
        }
        mean /= m;
        let mut var = 0.0f64;
        for i in 0..cells {
            let a = self.a[i] as f64;
            let d = g.mu[i] as f64 - mean;
            var += a * d * d;
        }
        var /= m;
        Some((mean as f32, var as f32))
    }

    /// Read-only view of one channel's concentration field.
    pub fn channel(&self, c: usize) -> &[f32] {
        let cells = self.w * self.h;
        &self.a[c * cells..(c + 1) * cells]
    }

    /// Mutable access to one channel's concentration field (for seeding).
    pub fn channel_mut(&mut self, c: usize) -> &mut [f32] {
        let cells = self.w * self.h;
        &mut self.a[c * cells..(c + 1) * cells]
    }

    /// Total mass summed over every channel and cell — the conserved quantity.
    pub fn total_mass(&self) -> f64 {
        self.a.iter().map(|&v| v as f64).sum()
    }

    /// Per-cell total concentration (summed over channels), row-major `w×h`.
    /// This is the field the measurement harness reduces over.
    pub fn mass_field(&self) -> Vec<f32> {
        let cells = self.w * self.h;
        let mut out = vec![0.0f32; cells];
        for c in 0..self.params.channels {
            let base = c * cells;
            for (i, v) in out.iter_mut().enumerate() {
                *v += self.a[base + i];
            }
        }
        out
    }

    /// Fraction of cells whose total concentration exceeds `thresh`.
    pub fn occupied_fraction(&self, thresh: f32) -> f32 {
        let cells = self.w * self.h;
        let mut count = 0usize;
        for i in 0..cells {
            let mut s = 0.0;
            for c in 0..self.params.channels {
                s += self.a[c * cells + i];
            }
            if s > thresh {
                count += 1;
            }
        }
        count as f32 / cells as f32
    }

    /// Variance of total concentration across cells. High variance = localized,
    /// structured matter; near-zero = uniform (dissipated) or empty.
    pub fn mass_variance(&self) -> f32 {
        let cells = self.w * self.h;
        let mut vals = vec![0.0f32; cells];
        for c in 0..self.params.channels {
            let base = c * cells;
            for (i, v) in vals.iter_mut().enumerate() {
                *v += self.a[base + i];
            }
        }
        let mean = vals.iter().sum::<f32>() / cells as f32;
        let var = vals.iter().map(|&v| (v - mean) * (v - mean)).sum::<f32>() / cells as f32;
        var
    }

    /// Mass-weighted center of mass of total concentration, in cell units.
    /// Uses a toroidal (circular-mean) reduction so wrapping blobs report a
    /// sensible centroid. Returns `None` if the world is empty.
    pub fn center_of_mass(&self) -> Option<(f32, f32)> {
        let cells = self.w * self.h;
        let (mut sx_c, mut sx_s, mut sy_c, mut sy_s, mut m) = (0.0f64, 0.0, 0.0, 0.0, 0.0f64);
        let tau = std::f64::consts::TAU;
        for y in 0..self.h {
            for x in 0..self.w {
                let mut v = 0.0f32;
                for c in 0..self.params.channels {
                    v += self.a[c * cells + y * self.w + x];
                }
                let v = v as f64;
                if v <= 0.0 {
                    continue;
                }
                let ax = tau * x as f64 / self.w as f64;
                let ay = tau * y as f64 / self.h as f64;
                sx_c += v * ax.cos();
                sx_s += v * ax.sin();
                sy_c += v * ay.cos();
                sy_s += v * ay.sin();
                m += v;
            }
        }
        if m <= 0.0 {
            return None;
        }
        let cx = (sx_s.atan2(sx_c).rem_euclid(tau)) / tau * self.w as f64;
        let cy = (sy_s.atan2(sy_c).rem_euclid(tau)) / tau * self.h as f64;
        Some((cx as f32, cy as f32))
    }

    /// Seed a smooth Gaussian blob of matter centered at `(cx, cy)` on channel
    /// `c`, with peak amplitude `amp` and standard deviation `radius`.
    pub fn seed_blob(&mut self, c: usize, cx: f32, cy: f32, radius: f32, amp: f32) {
        let (w, h) = (self.w, self.h);
        let inv = 1.0 / (2.0 * radius * radius);
        let field = self.channel_mut(c);
        for y in 0..h {
            for x in 0..w {
                // Nearest toroidal offset to the center.
                let dx = torus_delta(x as f32, cx, w as f32);
                let dy = torus_delta(y as f32, cy, h as f32);
                let g = amp * (-(dx * dx + dy * dy) * inv).exp();
                let idx = y * w + x;
                field[idx] = (field[idx] + g).min(1.0);
            }
        }
    }

    /// Seed a random circular patch of noise on channel `c`: every cell within
    /// `radius` of the center gets a uniform `[0, amp]` value.
    pub fn seed_random_patch<R: Rng>(
        &mut self,
        rng: &mut R,
        c: usize,
        cx: f32,
        cy: f32,
        radius: f32,
        amp: f32,
    ) {
        let (w, h) = (self.w, self.h);
        let field = self.channel_mut(c);
        for y in 0..h {
            for x in 0..w {
                let dx = torus_delta(x as f32, cx, w as f32);
                let dy = torus_delta(y as f32, cy, h as f32);
                if dx * dx + dy * dy <= radius * radius {
                    let idx = y * w + x;
                    field[idx] = (field[idx] + rng.gen::<f32>() * amp).min(1.0);
                }
            }
        }
    }

    /// Advance the world by one timestep. Total mass is invariant.
    pub fn step(&mut self) {
        let (w, h, cells) = (self.w, self.h, self.w * self.h);
        let channels = self.params.channels;

        // 1 & 2. Potential (kernel * A) → affinity U_i via growth mapping.
        //        Also accumulate total mass A_Σ. If the energy economy is on, the
        //        positive part of affinity is gated by local energy g(E)=E/(E+K):
        //        starved matter loses the pull that concentrates it into structure.
        let (mu, sigma) = (self.params.growth_mu, self.params.growth_sigma);
        // Move the genome out so its per-cell fields can be read here (growth) and
        // its advection accumulators written below (transport) without borrowing
        // all of `self`. Restored before the function returns.
        let mut genome = self.genome.take();
        for v in self.total.iter_mut() {
            *v = 0.0;
        }
        for c in 0..channels {
            let base = c * cells;
            for y in 0..h {
                for x in 0..w {
                    let mut acc = 0.0f32;
                    for tap in &self.kernel {
                        let sx = wrap(x as i32 + tap.dx, w);
                        let sy = wrap(y as i32 + tap.dy, h);
                        acc += self.a[base + sy * w + sx] * tap.w;
                    }
                    let idx = y * w + x;
                    // Localized growth (M-γ-1): matter here maps through its own
                    // genome's (μ, σ) if the genome is on, else the global rule.
                    let (gmu, gsig) = match &genome {
                        Some(g) => (g.mu[idx], g.sigma[idx]),
                        None => (mu, sigma),
                    };
                    let mut u = growth(acc, gmu, gsig);
                    if let Some(e) = &self.energy {
                        // Throttle the organizing affinity by local energy. Its
                        // gradient is what drives transport, so scaling it down
                        // starves matter of the flow that concentrates it — and
                        // the anti-crowding term (always on) then disperses what
                        // energy can no longer hold together.
                        let ev = e.field[idx];
                        u *= ev / (ev + e.params.gate_half);
                    }
                    self.potential[base + idx] = u;
                    self.total[idx] += self.a[base + idx];
                }
            }
        }

        // 3 & 4. Per channel: flow from ∇U_i and ∇A_Σ, then transport.
        let dt = self.params.dt;
        let theta = self.params.theta_a;
        let n = self.params.alpha_n;
        let max_flow = self.params.max_flow;
        // Zero the genome advection accumulators for this step.
        if let Some(g) = genome.as_mut() {
            for v in g.mu_acc.iter_mut() {
                *v = 0.0;
            }
            for v in g.sigma_acc.iter_mut() {
                *v = 0.0;
            }
        }
        for c in 0..channels {
            let base = c * cells;
            for v in self.scratch.iter_mut() {
                *v = 0.0;
            }
            for y in 0..h {
                for x in 0..w {
                    let src = y * w + x;
                    let m = self.a[base + src];
                    if m <= 0.0 {
                        continue;
                    }
                    // Sobel gradients of affinity (this channel) and total mass.
                    let (gux, guy) = sobel(&self.potential[base..base + cells], w, h, x, y);
                    let (gax, gay) = sobel(&self.total, w, h, x, y);
                    // Anti-crowding ramp: engage mass regulation as A_Σ → θ_A.
                    let a_sigma = self.total[src];
                    let alpha = ((a_sigma / theta).powf(n)).clamp(0.0, 1.0);
                    let fx = (1.0 - alpha) * gux - alpha * gax;
                    let fy = (1.0 - alpha) * guy - alpha * gay;
                    // Displacement in cells, clamped for advection fidelity.
                    let (mut dx, mut dy) = (fx * dt, fy * dt);
                    let mag = (dx * dx + dy * dy).sqrt();
                    if mag > max_flow {
                        let s = max_flow / mag;
                        dx *= s;
                        dy *= s;
                    }
                    // Bilinear (reintegration-tracking) scatter of mass `m` onto
                    // the unit box centered at (x+dx, y+dy). Weights sum to 1.
                    let tx = x as f32 + dx;
                    let ty = y as f32 + dy;
                    let x0f = tx.floor();
                    let y0f = ty.floor();
                    let wx = tx - x0f;
                    let wy = ty - y0f;
                    let x0 = wrap(x0f as i32, w);
                    let x1 = wrap(x0f as i32 + 1, w);
                    let y0 = wrap(y0f as i32, h);
                    let y1 = wrap(y0f as i32 + 1, h);
                    let (d00, d01, d10, d11) =
                        (y0 * w + x0, y0 * w + x1, y1 * w + x0, y1 * w + x1);
                    let (m00, m01, m10, m11) = (
                        m * (1.0 - wx) * (1.0 - wy),
                        m * wx * (1.0 - wy),
                        m * (1.0 - wx) * wy,
                        m * wx * wy,
                    );
                    self.scratch[d00] += m00;
                    self.scratch[d01] += m01;
                    self.scratch[d10] += m10;
                    self.scratch[d11] += m11;
                    // Advect the genome (M-γ-1): the mass leaving `src` carries its
                    // (μ, σ) to the same four cells, weighted by the moved mass.
                    // Resolved into a mass-weighted average after the channel loop.
                    if let Some(g) = genome.as_mut() {
                        let (gmu, gsig) = (g.mu[src], g.sigma[src]);
                        g.mu_acc[d00] += m00 * gmu;
                        g.mu_acc[d01] += m01 * gmu;
                        g.mu_acc[d10] += m10 * gmu;
                        g.mu_acc[d11] += m11 * gmu;
                        g.sigma_acc[d00] += m00 * gsig;
                        g.sigma_acc[d01] += m01 * gsig;
                        g.sigma_acc[d10] += m10 * gsig;
                        g.sigma_acc[d11] += m11 * gsig;
                    }
                }
            }
            self.a[base..base + cells].copy_from_slice(&self.scratch);
        }

        // Resolve the advected genome: each cell's new (μ, σ) is the mass-weighted
        // average of what arrived. `self.scratch` holds channel 0's post-transport
        // mass (single-channel invariant of the genome). Cells that received no
        // mass keep their prior genome — irrelevant until matter returns.
        if let Some(g) = genome.as_mut() {
            for i in 0..cells {
                let m = self.scratch[i];
                if m > 1e-9 {
                    g.mu[i] = g.mu_acc[i] / m;
                    g.sigma[i] = g.sigma_acc[i] / m;
                }
            }
        }
        self.genome = genome;

        // 5. Energy economy (M-γ-2), if enabled: spend on growth + maintenance,
        //    inject from sources, diffuse. `self.total` still holds pre-transport
        //    A_Σ, so ΔA is recoverable against the just-updated matter.
        self.update_energy();

        // 6. Detritus cycle (M-γ-3), if enabled: kill starved matter into detritus
        //    and decompose detritus back into the live channel + energy. Runs after
        //    the energy update so death reads this step's post-injection energy.
        self.update_detritus();
    }

    /// Update the energy field one step: consumption (`ΔA > 0`), maintenance
    /// (`∝ A_Σ`), renewable source injection, then diffusion. Mass is untouched —
    /// this only spends and spreads energy. No-op if the economy is disabled.
    fn update_energy(&mut self) {
        if self.energy.is_none() {
            return;
        }
        let (w, h, cells) = (self.w, self.h, self.w * self.h);
        let channels = self.params.channels;

        // Pointwise spend + source, reading pre-transport A_Σ from `self.total`
        // and post-transport A_Σ from `self.a`. Kept as a separate borrow scope so
        // the diffusion pass below can borrow the energy field freely.
        {
            let energy = self.energy.as_mut().unwrap();
            let (consume, maintain, cap) =
                (energy.params.consume, energy.params.maintain, energy.params.capacity);
            for i in 0..cells {
                let mut a_new = 0.0f32;
                for c in 0..channels {
                    a_new += self.a[c * cells + i];
                }
                let delta = a_new - self.total[i]; // ΔA_Σ at this cell
                let mut e = energy.field[i];
                e -= consume * delta.max(0.0); // building structure costs energy
                e -= maintain * a_new; // upkeep costs energy
                e += energy.source[i]; // renewable injection
                energy.field[i] = e.clamp(0.0, cap);
            }
        }

        // Diffusion: explicit 5-point Laplacian, toroidal, out-of-place.
        let energy = self.energy.as_mut().unwrap();
        let d = energy.params.diffusion;
        if d > 0.0 {
            let cap = energy.params.capacity;
            let f = &energy.field;
            let s = &mut energy.scratch;
            for y in 0..h {
                let ym = wrap(y as i32 - 1, h);
                let yp = wrap(y as i32 + 1, h);
                for x in 0..w {
                    let xm = wrap(x as i32 - 1, w);
                    let xp = wrap(x as i32 + 1, w);
                    let c = f[y * w + x];
                    let lap = f[y * w + xm] + f[y * w + xp] + f[ym * w + x] + f[yp * w + x]
                        - 4.0 * c;
                    s[y * w + x] = (c + d * lap).clamp(0.0, cap);
                }
            }
            std::mem::swap(&mut energy.field, &mut energy.scratch);
        }
    }

    /// Update the detritus cycle one step (M-γ-3): starved live matter dies into
    /// detritus, detritus decomposes back into the live channel and releases energy.
    /// Matter only *moves* between the live channel and detritus, so
    /// `Σ A_live + Σ Det` is conserved exactly. No-op unless M-γ-3 is enabled.
    fn update_detritus(&mut self) {
        // Both the detritus field and the energy field are needed; `enable_detritus`
        // guarantees energy is present. These are disjoint fields of `self`, so the
        // live channel `self.a` can be borrowed alongside them.
        let (Some(det), Some(energy)) = (self.detritus.as_mut(), self.energy.as_mut()) else {
            return;
        };
        let cells = self.w * self.h;
        let k = energy.params.gate_half;
        let cap = energy.params.capacity;
        let DetritusParams { death_rate, recycle_matter, recycle_energy } = det.params;
        for i in 0..cells {
            // Death: the same energy shortage that closes the growth gate now kills.
            // s = 1 − g(E) = K/(E+K) → 1 as E → 0, 0 when energy is plentiful.
            let e = energy.field[i];
            let starve = k / (e + k);
            let dead = death_rate * starve * self.a[i];
            self.a[i] -= dead;
            let pool = det.field[i] + dead;
            // Recycle: decomposition returns matter to the live channel (conserving
            // {live + detritus}) and releases energy as a byproduct (a source term).
            let back = recycle_matter * pool;
            det.field[i] = pool - back;
            self.a[i] += back;
            energy.field[i] = (energy.field[i] + recycle_energy * back).min(cap);
        }
    }
}

/// Lenia growth: a bell curve on the neighborhood potential, mapped to `[-1, 1]`.
#[inline]
fn growth(u: f32, mu: f32, sigma: f32) -> f32 {
    let d = (u - mu) / sigma;
    2.0 * (-0.5 * d * d).exp() - 1.0
}

/// Toroidal signed delta from `to` to `from` (nearest wrap), in `[-size/2, size/2)`.
#[inline]
fn torus_delta(from: f32, to: f32, size: f32) -> f32 {
    let mut d = from - to;
    if d > size * 0.5 {
        d -= size;
    } else if d < -size * 0.5 {
        d += size;
    }
    d
}

/// Wrap an integer coordinate into `[0, n)`.
#[inline]
fn wrap(v: i32, n: usize) -> usize {
    let n = n as i32;
    (((v % n) + n) % n) as usize
}

/// 3×3 Sobel gradient of `field` at `(x, y)` with toroidal boundaries.
#[inline]
fn sobel(field: &[f32], w: usize, h: usize, x: usize, y: usize) -> (f32, f32) {
    let xm = wrap(x as i32 - 1, w);
    let xp = wrap(x as i32 + 1, w);
    let ym = wrap(y as i32 - 1, h);
    let yp = wrap(y as i32 + 1, h);
    let at = |xx: usize, yy: usize| field[yy * w + xx];
    let tl = at(xm, ym);
    let tc = at(x, ym);
    let tr = at(xp, ym);
    let ml = at(xm, y);
    let mr = at(xp, y);
    let bl = at(xm, yp);
    let bc = at(x, yp);
    let br = at(xp, yp);
    let gx = (tr + 2.0 * mr + br - tl - 2.0 * ml - bl) / 8.0;
    let gy = (bl + 2.0 * bc + br - tl - 2.0 * tc - tr) / 8.0;
    (gx, gy)
}

/// Build the shared radial kernel as normalized taps within `kernel_radius`.
fn build_kernel(params: &FlowLeniaParams) -> Vec<Tap> {
    let r = params.kernel_radius as i32;
    let rf = params.kernel_radius as f32;
    let mut taps = Vec::new();
    let mut sum = 0.0f32;
    for dy in -r..=r {
        for dx in -r..=r {
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            let n = dist / rf; // normalized radius
            if n > 1.0 || n <= 0.0 {
                continue;
            }
            let mut val = 0.0f32;
            for ring in &params.rings {
                let d = (n - ring.peak) / ring.width;
                val += ring.weight * (-0.5 * d * d).exp();
            }
            if val > 1e-6 {
                taps.push(Tap { dx, dy, w: val });
                sum += val;
            }
        }
    }
    // Normalize so a uniform unit field convolves to exactly 1.
    if sum > 0.0 {
        for tap in &mut taps {
            tap.w /= sum;
        }
    }
    taps
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn test_params() -> FlowLeniaParams {
        FlowLeniaParams::default()
    }

    #[test]
    fn kernel_is_normalized() {
        let k = build_kernel(&test_params());
        assert!(!k.is_empty(), "kernel should have taps");
        let sum: f32 = k.iter().map(|t| t.w).sum();
        assert!((sum - 1.0).abs() < 1e-4, "kernel weights sum to 1, got {sum}");
    }

    #[test]
    fn empty_world_stays_empty() {
        let mut world = World::new(48, 48, test_params());
        for _ in 0..20 {
            world.step();
        }
        assert_eq!(world.total_mass(), 0.0);
    }

    #[test]
    fn mass_is_conserved_over_many_steps() {
        // The defining M-γ-0 invariant: transport moves mass, never creates it.
        let mut world = World::new(64, 64, test_params());
        world.seed_blob(0, 32.0, 32.0, 6.0, 0.9);
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        world.seed_random_patch(&mut rng, 0, 20.0, 40.0, 8.0, 0.5);

        let initial = world.total_mass();
        assert!(initial > 0.0);
        for step in 0..200 {
            world.step();
            let m = world.total_mass();
            let drift = (m - initial).abs() / initial;
            assert!(
                drift < 1e-4,
                "mass drifted by {drift} at step {step} (initial {initial}, now {m})"
            );
        }
    }

    #[test]
    fn mass_conserved_multichannel() {
        let mut params = test_params();
        params.channels = 3;
        let mut world = World::new(48, 48, params);
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        for c in 0..3 {
            world.seed_random_patch(&mut rng, c, 24.0, 24.0, 10.0, 0.7);
        }
        let initial = world.total_mass();
        for _ in 0..100 {
            world.step();
        }
        let drift = (world.total_mass() - initial).abs() / initial;
        assert!(drift < 1e-4, "multichannel mass drifted by {drift}");
    }

    #[test]
    fn seeded_structure_does_not_dissipate_to_uniform() {
        // A seeded blob should remain spatially structured, not smear into a
        // flat soup. (Localization is the qualitative M-γ-0 signal; mass
        // conservation above is the quantitative gate.)
        let mut world = World::new(64, 64, test_params());
        world.seed_blob(0, 32.0, 32.0, 5.0, 0.95);
        let v0 = world.mass_variance();
        for _ in 0..100 {
            world.step();
        }
        let v1 = world.mass_variance();
        assert!(v1 > v0 * 0.1, "structure collapsed: variance {v0} → {v1}");
    }

    #[test]
    fn center_of_mass_tracks_a_blob() {
        let mut world = World::new(64, 64, test_params());
        world.seed_blob(0, 20.0, 44.0, 4.0, 0.9);
        let (cx, cy) = world.center_of_mass().unwrap();
        assert!((cx - 20.0).abs() < 1.5, "cx off: {cx}");
        assert!((cy - 44.0).abs() < 1.5, "cy off: {cy}");
    }

    #[test]
    fn energy_disabled_by_default() {
        let world = World::new(16, 16, test_params());
        assert!(!world.energy_enabled());
        assert!(world.energy_field().is_none());
        assert!(world.total_energy().is_none());
    }

    #[test]
    fn mass_conserved_with_energy_on() {
        // The energy layer gates and spends *energy*; it must never touch the
        // mass invariant. Death/recycling (which would move mass) is M-γ-3.
        let mut world = World::new(64, 64, test_params());
        world.enable_energy(EnergyParams::default());
        world.charge_energy(2.0);
        world.add_source(32.0, 32.0, 10.0, 0.3);
        world.seed_blob(0, 32.0, 32.0, 6.0, 0.9);
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        world.seed_random_patch(&mut rng, 0, 20.0, 40.0, 8.0, 0.5);

        let initial = world.total_mass();
        assert!(initial > 0.0);
        for step in 0..200 {
            world.step();
            let drift = (world.total_mass() - initial).abs() / initial;
            assert!(drift < 1e-4, "mass drifted by {drift} at step {step} with energy on");
        }
    }

    #[test]
    fn source_replenishes_and_upkeep_drains_energy() {
        // A world with matter but no source bleeds energy (maintenance +
        // consumption); an otherwise identical world with a source refills it.
        let build = |with_source: bool| {
            let mut w = World::new(48, 48, test_params());
            w.enable_energy(EnergyParams::default());
            w.charge_energy(1.0);
            w.seed_blob(0, 24.0, 24.0, 6.0, 0.9);
            if with_source {
                w.add_source(24.0, 24.0, 10.0, 0.5);
            }
            w
        };
        let mut drained = build(false);
        let mut fed = build(true);
        let e0 = drained.total_energy().unwrap();
        for _ in 0..60 {
            drained.step();
            fed.step();
        }
        let e_drained = drained.total_energy().unwrap();
        let e_fed = fed.total_energy().unwrap();
        assert!(e_drained < e0, "unsourced energy should fall: {e0} → {e_drained}");
        assert!(e_fed > e_drained, "sourced world should hold more energy: {e_fed} vs {e_drained}");
    }

    #[test]
    fn energy_gating_selects_which_structure_persists() {
        // Same seed, same physics — only the economy differs. Fed matter (sitting
        // on a renewable source) keeps its organizing affinity and stays a live,
        // moving, multi-blob structure; starved matter (energy ≈ 0 → gate ≈ 0)
        // loses the affinity flow, goes inert, and its structure collapses.
        //
        // This is the M-γ-2 claim, measured through the F1 harness: energy
        // competition changes *which* patterns persist — activity and motility,
        // not just a static snapshot — with no fitness function in sight.
        use crate::harness::measure_run;

        let seed = |w: &mut World| {
            let mut rng = rand::rngs::StdRng::seed_from_u64(99);
            w.seed_blob(0, 48.0, 48.0, 8.0, 0.95);
            w.seed_blob(0, 30.0, 64.0, 6.0, 0.9);
            w.seed_random_patch(&mut rng, 0, 70.0, 30.0, 8.0, 0.5);
        };

        let mut fed = World::new(96, 96, test_params());
        fed.enable_energy(EnergyParams::default());
        fed.charge_energy(2.0);
        fed.add_source(48.0, 48.0, 16.0, 0.5);
        seed(&mut fed);

        let mut starved = World::new(96, 96, test_params());
        starved.enable_energy(EnergyParams::default());
        // No charge, no source: the gate stays near zero everywhere.
        seed(&mut starved);

        let (fed_sum, _) = measure_run(&mut fed, 200, 20, 0.05, 8.0);
        let (starved_sum, _) = measure_run(&mut starved, 200, 20, 0.05, 8.0);

        // Mass is untouched by the economy in both.
        assert!(fed_sum.mass_drift < 1e-4 && starved_sum.mass_drift < 1e-4);
        // Fed stays dynamic; starved freezes (the gap is ~10× — assert a safe 3×).
        assert!(
            fed_sum.mean_activity > starved_sum.mean_activity * 3.0,
            "fed should stay dynamic: activity {} vs starved {}",
            fed_sum.mean_activity,
            starved_sum.mean_activity
        );
        // Fed keeps more distinct structures alive at the end.
        assert!(
            fed_sum.final_components > starved_sum.final_components,
            "fed should keep more structure: {} vs {}",
            fed_sum.final_components,
            starved_sum.final_components
        );
    }

    // ---- M-γ-3: closed-loop detritus recycling ---------------------------

    #[test]
    fn detritus_disabled_by_default() {
        let mut world = World::new(16, 16, test_params());
        assert!(!world.detritus_enabled());
        assert!(world.detritus_field().is_none());
        assert!(world.total_detritus().is_none());
        // Enabling requires the energy economy.
        world.enable_energy(EnergyParams::default());
        world.enable_detritus(DetritusParams::default());
        assert!(world.detritus_enabled());
        assert_eq!(world.total_detritus(), Some(0.0));
    }

    #[test]
    #[should_panic(expected = "needs the energy economy")]
    fn detritus_requires_energy() {
        let mut world = World::new(16, 16, test_params());
        world.enable_detritus(DetritusParams::default());
    }

    #[test]
    fn matter_conserved_across_live_and_detritus() {
        // The M-γ invariant extended to M-γ-3: death and recycling only *move*
        // matter between the live channel and detritus, so their sum is conserved
        // exactly — even as a starved world sheds most of its live mass to detritus.
        let mut world = World::new(64, 64, test_params());
        world.enable_energy(EnergyParams::default());
        world.enable_detritus(DetritusParams::default());
        // Starved (no source, no charge) so death is vigorous; recycling churns.
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        world.seed_blob(0, 32.0, 32.0, 8.0, 0.95);
        world.seed_random_patch(&mut rng, 0, 20.0, 44.0, 8.0, 0.6);

        let initial = world.total_mass() + world.total_detritus().unwrap();
        assert!(initial > 0.0);
        let mut saw_detritus = false;
        for step in 0..200 {
            world.step();
            let total = world.total_mass() + world.total_detritus().unwrap();
            let drift = (total - initial).abs() / initial;
            assert!(drift < 1e-4, "live+detritus drifted by {drift} at step {step}");
            if world.total_detritus().unwrap() > initial * 0.05 {
                saw_detritus = true;
            }
        }
        // The starved world really did die into a detritus pool (not a no-op).
        assert!(saw_detritus, "expected starvation to build a detritus pool");
    }

    #[test]
    fn fed_matter_stays_alive_starved_matter_becomes_detritus() {
        // The M-γ-3 payoff: with death+recycling on, a fed world keeps its mass in
        // the *live* channel, while a starved world converts most of it to detritus.
        let build = |fed: bool| {
            let mut w = World::new(80, 80, test_params());
            w.enable_energy(EnergyParams::default());
            w.enable_detritus(DetritusParams::default());
            if fed {
                w.charge_energy(2.0);
                w.add_source(40.0, 40.0, 14.0, 0.5);
            }
            w.seed_blob(0, 40.0, 40.0, 9.0, 0.95);
            w
        };
        let mut fed = build(true);
        let mut starved = build(false);
        for _ in 0..250 {
            fed.step();
            starved.step();
        }
        // Fraction of each world's matter that is dead (detritus).
        let dead_frac = |w: &World| {
            let det = w.total_detritus().unwrap();
            det / (w.total_mass() + det)
        };
        let (fed_dead, starved_dead) = (dead_frac(&fed), dead_frac(&starved));
        assert!(
            starved_dead > fed_dead + 0.3,
            "starved world should be far more detritus: starved {starved_dead:.2} vs fed {fed_dead:.2}"
        );
        // Recycling releases energy: the starved world's energy is not strictly zero
        // even though it has no external source (dead matter fed it).
        assert!(
            starved.total_energy().unwrap() > 0.0,
            "detritus decomposition should release energy into the starved world"
        );
    }

    // ---- M-γ-1: parameter localization -----------------------------------

    #[test]
    fn genome_disabled_by_default() {
        let world = World::new(16, 16, test_params());
        assert!(!world.genome_enabled());
        assert!(world.mu_field().is_none());
        assert!(world.mu_stats().is_none());
    }

    #[test]
    #[should_panic(expected = "single-channel")]
    fn genome_rejects_multichannel() {
        let mut params = test_params();
        params.channels = 2;
        let mut world = World::new(16, 16, params);
        world.enable_genome();
    }

    #[test]
    fn mass_conserved_with_genome() {
        // Advecting the genome must not perturb the mass invariant: matter is
        // still only moved by the same bilinear splat.
        let mut world = World::new(64, 64, test_params());
        world.enable_genome();
        world.seed_species(28.0, 32.0, 6.0, 0.9, 0.13, 0.017);
        world.seed_species(40.0, 34.0, 6.0, 0.9, 0.17, 0.017);
        let initial = world.total_mass();
        for step in 0..200 {
            world.step();
            let drift = (world.total_mass() - initial).abs() / initial;
            assert!(drift < 1e-4, "mass drifted by {drift} at step {step} with genome on");
        }
    }

    #[test]
    fn uniform_genome_is_preserved_under_advection() {
        // A world with one genome everywhere must keep exactly that genome: the
        // mass-weighted average of identical parameters is that parameter, so
        // advection injects no spurious drift or default values.
        let mut world = World::new(64, 64, test_params());
        world.enable_genome();
        world.paint_genome(32.0, 32.0, 200.0, 0.16, 0.019); // whole world
        world.seed_blob(0, 32.0, 32.0, 7.0, 0.95);
        for _ in 0..100 {
            world.step();
        }
        let (mean, var) = world.mu_stats().unwrap();
        assert!((mean - 0.16).abs() < 1e-3, "μ mean drifted to {mean}");
        assert!(var < 1e-6, "uniform genome grew variance {var}");
    }

    #[test]
    fn species_coexist_without_homogenizing() {
        // Three separated species with distinct μ persist together: the
        // mass-weighted μ variance stays near its seeded value rather than
        // collapsing toward a single blended rule (risk #3).
        let mut world = World::new(96, 96, test_params());
        world.enable_genome();
        world.seed_species(28.0, 32.0, 7.0, 0.95, 0.13, 0.017);
        world.seed_species(66.0, 34.0, 7.0, 0.95, 0.15, 0.017);
        world.seed_species(48.0, 68.0, 7.0, 0.95, 0.17, 0.017);
        let (_, v0) = world.mu_stats().unwrap();
        assert!(v0 > 0.0);
        for _ in 0..200 {
            world.step();
        }
        let (_, v1) = world.mu_stats().unwrap();
        assert!(
            v1 > v0 * 0.5,
            "gene pool homogenized: μ variance {v0} → {v1}"
        );
        // All three species still present: μ spread across the occupied field.
        let (lo, hi) = occupied_mu_span(&world, 0.05);
        assert!(hi - lo > 0.03, "species collapsed to one rule: μ span [{lo}, {hi}]");
    }

    #[test]
    fn genomes_mix_where_matter_merges() {
        // Rule mixing: where mass from two genomes lands on the same cell, the
        // new genome is their mass-weighted average, so intermediate μ values —
        // present in *neither* seed — appear. Background genome is 0.13 so any
        // intermediate μ is a genuine advective blend, not a leftover default.
        let mut world = World::new(64, 64, test_params());
        world.enable_genome();
        world.paint_genome(32.0, 32.0, 200.0, 0.13, 0.017); // whole world = 0.13
        world.paint_genome(40.0, 32.0, 16.0, 0.17, 0.017); // right lobe = 0.17
        let mut rng = rand::rngs::StdRng::seed_from_u64(4);
        world.seed_random_patch(&mut rng, 0, 32.0, 32.0, 22.0, 0.6);

        let blended_before = count_blended(&world, 0.05, 0.13, 0.17, 0.01);
        assert_eq!(blended_before, 0, "seeds should be pure before any step");
        for _ in 0..80 {
            world.step();
        }
        let blended_after = count_blended(&world, 0.05, 0.13, 0.17, 0.01);
        assert!(
            blended_after > 0,
            "no rule mixing: expected cells with intermediate μ, found none"
        );
    }

    /// Min/max localized μ over cells carrying meaningful mass.
    fn occupied_mu_span(world: &World, thresh: f32) -> (f32, f32) {
        let mass = world.channel(0);
        let mu = world.mu_field().unwrap();
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for (i, &m) in mass.iter().enumerate() {
            if m > thresh {
                lo = lo.min(mu[i]);
                hi = hi.max(mu[i]);
            }
        }
        (lo, hi)
    }

    /// Count occupied cells whose μ is strictly between `a` and `b` by at least
    /// `margin` on both sides — a genuine blend of the two seeded genomes.
    fn count_blended(world: &World, thresh: f32, a: f32, b: f32, margin: f32) -> usize {
        let mass = world.channel(0);
        let mu = world.mu_field().unwrap();
        let (lo, hi) = (a.min(b) + margin, a.max(b) - margin);
        mass.iter()
            .zip(mu)
            .filter(|(&m, &u)| m > thresh && u > lo && u < hi)
            .count()
    }
}
