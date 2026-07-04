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
///    precursor to the actual detritus death of M-γ-3.
/// 2. **Consumption.** Building structure costs energy: wherever mass accumulates
///    (`ΔA > 0` after transport), energy is drawn down proportionally.
/// 3. **Maintenance.** Merely staying organized costs energy proportional to local
///    mass, every step.
///
/// Energy is replenished by localized renewable **sources** (`add_source`) and
/// spreads by **diffusion**, so exploitable gradients form. Energy is *not*
/// conserved (it is a flow, sourced and dissipated); **mass remains conserved**
/// across the matter channels exactly as before — gating only reshapes flow, and
/// death/recycling (which would move mass into a detritus channel) is deferred to
/// M-γ-3. The point: patterns whose localized behavior lets them find and hold
/// energy persist; others starve. Selection becomes a consequence of the economy,
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

/// A precomputed kernel tap: an integer offset and its normalized weight.
#[derive(Clone, Copy)]
struct Tap {
    dx: i32,
    dy: i32,
    w: f32,
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

    #[inline]
    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
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
                    let mut u = growth(acc, mu, sigma);
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
        for c in 0..channels {
            let base = c * cells;
            for v in self.scratch.iter_mut() {
                *v = 0.0;
            }
            for y in 0..h {
                for x in 0..w {
                    let m = self.a[base + self.idx(x, y)];
                    if m <= 0.0 {
                        continue;
                    }
                    // Sobel gradients of affinity (this channel) and total mass.
                    let (gux, guy) = sobel(&self.potential[base..base + cells], w, h, x, y);
                    let (gax, gay) = sobel(&self.total, w, h, x, y);
                    // Anti-crowding ramp: engage mass regulation as A_Σ → θ_A.
                    let a_sigma = self.total[self.idx(x, y)];
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
                    self.scratch[y0 * w + x0] += m * (1.0 - wx) * (1.0 - wy);
                    self.scratch[y0 * w + x1] += m * wx * (1.0 - wy);
                    self.scratch[y1 * w + x0] += m * (1.0 - wx) * wy;
                    self.scratch[y1 * w + x1] += m * wx * wy;
                }
            }
            self.a[base..base + cells].copy_from_slice(&self.scratch);
        }

        // 5. Energy economy (M-γ-2), if enabled: spend on growth + maintenance,
        //    inject from sources, diffuse. `self.total` still holds pre-transport
        //    A_Σ, so ΔA is recoverable against the just-updated matter.
        self.update_energy();
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
}
