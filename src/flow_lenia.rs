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
    total: Vec<f32>,     // A_Σ
    scratch: Vec<f32>,   // reintegration target for one channel
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
        //        Also accumulate total mass A_Σ.
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
                    self.potential[base + idx] = growth(acc, self.params.growth_mu, self.params.growth_sigma);
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
}
