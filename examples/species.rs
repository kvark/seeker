//! M-γ-1 demonstration: parameter localization → multi-species coexistence.
//!
//! The growth genome `(μ, σ)` is localized into a per-cell field that advects
//! *with* the mass (same reintegration-tracking transport). We seed several
//! species — same substrate, different growth center μ — in one world and watch
//! them coexist, compete, and mix. Where mass from two species merges, the new
//! genome is their mass-weighted average, so blended μ values appear at the
//! interfaces; that low-pass mixing is also the homogenization risk (CLAUDE.md
//! risk #3), so the run tracks the mass-weighted μ variance over time.
//!
//! Usage:
//!   cargo run --release --example species [steps] [out.gif]
//!
//! The GIF encodes **species by hue** (μ → color) and **mass by brightness**, so
//! distinct species read as distinct colors and blends as intermediate hues.

use rand::SeedableRng;
use seeker::flow_lenia::{FlowLeniaParams, World};
use std::borrow::Cow;
use std::fs::File;

const CELL: u16 = 3;
// Display range for μ → hue. Seeded species live inside this band.
const MU_LO: f32 = 0.10;
const MU_HI: f32 = 0.20;

/// The three species genomes, one per vertical band: (label, μ, σ). Same σ,
/// different growth center μ.
const SPECIES: &[(&str, f32, f32)] = &[
    ("violet μ=0.125", 0.125, 0.017),
    ("teal   μ=0.150", 0.150, 0.017),
    ("amber  μ=0.175", 0.175, 0.017),
];

/// The distinct seeded μ values, for the blend metric.
const SEEDED_MU: &[f32] = &[0.125, 0.150, 0.175];

/// A **genome-region soup**: three genome territories — a violet disc (left) and
/// an amber disc (right) over a teal background (the default μ) — flooded with a
/// shared matter soup. Flow-Lenia condenses the soup into localized spots; spots
/// well inside a territory keep that species' genome (coexistence), while spots
/// near a boundary pull mass from two territories and carry the mass-weighted
/// blend (rule mixing). One scene, both phenomena.
fn build_world(w: usize, h: usize) -> World {
    let mut world = World::new(w, h, FlowLeniaParams::default());
    world.enable_genome();

    // Background genome is the world default (teal, μ=0.150). Paint the violet
    // and amber territories as broad discs left and right.
    let (wf, hf) = (w as f32, h as f32);
    world.paint_genome(wf * 0.28, hf * 0.5, wf * 0.24, SPECIES[0].1, SPECIES[0].2); // violet
    world.paint_genome(wf * 0.72, hf * 0.5, wf * 0.24, SPECIES[2].1, SPECIES[2].2); // amber

    // Flood a broad matter soup so every territory has raw material to condense.
    let mut rng = rand::rngs::StdRng::seed_from_u64(20240705);
    world.seed_random_patch(&mut rng, 0, wf * 0.5, hf * 0.5, wf * 0.42, 0.55);
    world
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let steps: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("data/species.gif");
    let (w, h) = (128usize, 128usize);

    let mut world = build_world(w, h);
    let initial_mass = world.total_mass();

    println!("Flow-Lenia M-γ-1  |  {w}×{h}, {steps} steps  |  parameter localization\n");
    for &(label, mu, _) in SPECIES {
        println!("  genome territory: {label}  (μ={mu})");
    }
    let (m0, v0) = world.mu_stats().unwrap();
    println!("\n  step | mass drift | occupied | μ mean  | μ var     | μ range        | blend%");
    println!("  -----|------------|----------|---------|-----------|----------------|-------");

    let palette = build_palette();
    let gw = w as u16 * CELL;
    let gh = h as u16 * CELL;
    let file = File::create(out).expect("create gif");
    let mut encoder = gif::Encoder::new(file, gw, gh, &palette).expect("gif encoder");
    encoder.set_repeat(gif::Repeat::Infinite).ok();
    let frame_every = (steps / 150).max(1);

    write_frame(&mut encoder, &world, gw, gh);
    report(0, &world, initial_mass, m0, v0);

    for step in 1..=steps {
        world.step();
        if step % frame_every == 0 {
            write_frame(&mut encoder, &world, gw, gh);
        }
        if step % (steps / 12).max(1) == 0 || step == steps {
            let (mean, var) = world.mu_stats().unwrap();
            report(step, &world, initial_mass, mean, var);
        }
    }

    let (mean, var) = world.mu_stats().unwrap();
    let blend = blend_mass_fraction(&world, 0.05, 0.006) * 100.0;
    println!(
        "\nμ variance {v0:.2e} → {var:.2e}  (mean {mean:.4}).  Variance holding well above\n\
         zero = species coexist; collapse toward zero = the gene pool homogenized.\n\
         blend fraction rose to {blend:.1}% of mass — intermediate μ in *neither* seed,\n\
         produced only where mass from two genomes merged: that is rule mixing.\n\
         (Modest here because the default regime makes static, non-migrating spots;\n\
         strong mixing wants motile species — an F2 tuning target, not hand-tuning.)\n\
         gif → {out}  (hue = species/μ, brightness = mass)"
    );
}

fn report(step: usize, world: &World, initial_mass: f64, mean: f32, var: f32) {
    let mass = world.total_mass();
    let drift = (mass - initial_mass).abs() / initial_mass;
    let occ = world.occupied_fraction(0.05);
    let (lo, hi) = occupied_mu_range(world, 0.05);
    let blend = blend_mass_fraction(world, 0.05, 0.006) * 100.0;
    println!(
        "  {step:4} | {drift:10.2e} | {occ:8.3} | {mean:7.4} | {var:9.2e} | [{lo:.3}, {hi:.3}] | {blend:5.1}"
    );
}

/// Fraction of occupied *mass* whose localized μ is not within `tol` of any
/// seeded species value — i.e., a genuine advective blend, produced only where
/// mass from different species merged. Zero at seed time (every cell is exactly
/// one species); rising = rule mixing.
fn blend_mass_fraction(world: &World, thresh: f32, tol: f32) -> f32 {
    let mass = world.channel(0);
    let mu = world.mu_field().expect("genome enabled");
    let (mut blend, mut total) = (0.0f64, 0.0f64);
    for (i, &m) in mass.iter().enumerate() {
        if m <= thresh {
            continue;
        }
        total += m as f64;
        let pure = SEEDED_MU.iter().any(|&s| (mu[i] - s).abs() <= tol);
        if !pure {
            blend += m as f64;
        }
    }
    if total > 0.0 {
        (blend / total) as f32
    } else {
        0.0
    }
}

/// Min and max localized μ over cells carrying meaningful mass — the spread that
/// tells us whether multiple species are still present.
fn occupied_mu_range(world: &World, thresh: f32) -> (f32, f32) {
    let mass = world.channel(0);
    let mu = world.mu_field().expect("genome enabled");
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for (i, &m) in mass.iter().enumerate() {
        if m > thresh {
            lo = lo.min(mu[i]);
            hi = hi.max(mu[i]);
        }
    }
    if lo.is_finite() {
        (lo, hi)
    } else {
        (0.0, 0.0)
    }
}

/// Color each cell by species (μ → hue) and mass (→ brightness).
fn write_frame(encoder: &mut gif::Encoder<File>, world: &World, gw: u16, gh: u16) {
    let (w, h) = (world.width(), world.height());
    let mass = world.channel(0);
    let mu = world.mu_field().expect("genome enabled");
    let mut pixels = vec![0u8; gw as usize * gh as usize];
    for y in 0..h {
        for x in 0..w {
            let m = mass[y * w + x].clamp(0.0, 1.0);
            let idx = if m < 0.02 {
                0
            } else {
                let hb = (((mu[y * w + x] - MU_LO) / (MU_HI - MU_LO)) * 15.0).clamp(0.0, 15.0) as u8;
                let vb = (m * 15.0).clamp(1.0, 15.0) as u8;
                hb * 16 + vb
            };
            for dy in 0..CELL {
                for dx in 0..CELL {
                    let px = x as u16 * CELL + dx;
                    let py = y as u16 * CELL + dy;
                    pixels[py as usize * gw as usize + px as usize] = idx;
                }
            }
        }
    }
    let frame = gif::Frame {
        width: gw,
        height: gh,
        delay: 6,
        buffer: Cow::Owned(pixels),
        ..Default::default()
    };
    encoder.write_frame(&frame).ok();
}

/// A 16×16 indexed palette: 16 hues (μ) × 16 brightnesses (mass). Index
/// `hue*16 + value`; the `value == 0` row is black (empty cells).
fn build_palette() -> Vec<u8> {
    let mut pal = Vec::with_capacity(256 * 3);
    for hue_bucket in 0..16 {
        // Map hue bucket to [0, 0.72]: violet(0) → teal → amber(0.72-ish).
        let hue = hue_bucket as f32 / 15.0 * 0.72;
        for val_bucket in 0..16 {
            let val = val_bucket as f32 / 15.0;
            let (r, g, b) = hsv_to_rgb(hue, 0.85, val);
            pal.push(r);
            pal.push(g);
            pal.push(b);
        }
    }
    pal
}

/// HSV → RGB (all in `[0, 1]` in, `u8` out). `h` wraps at 1.0.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    (
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8,
    )
}
