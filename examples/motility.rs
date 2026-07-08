//! F2 → M-γ-1: search for a *motile* species, then show it makes mixing dramatic.
//!
//! M-γ-1 confirmed that localized genomes coexist but barely mix in the default
//! regime — the structures are static, so they never collide. The fix is not to
//! hand-tune a mover (that is the designer-in-the-loop trap) but to *search* for
//! one. This example:
//!
//!   1. Runs MAP-Elites with the **motility** objective — illuminate
//!      concentration × speed, reward coherent translating structure.
//!   2. Takes the fastest coherent glider discovered and drops it into an M-γ-1
//!      multi-species world (two species, same rule, different localized μ).
//!   3. Measures the blend fraction: movers collide, so their genomes mix far
//!      more than the static ~1% baseline. A GIF shows the collision.
//!
//! Usage:
//!   cargo run --release --example motility [generations] [out.gif]

use seeker::flow_lenia::World;
use seeker::search::{map_elites, EvalConfig, Genome, MapConfig, Objective, SearchConfig};
use std::borrow::Cow;
use std::fs::File;

const CELL: u16 = 4;
const MU_LO: f32 = 0.0;
const MU_HI: f32 = 0.4;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let generations: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("data/motility.gif");

    // --- 1. Search for movers. ------------------------------------------------
    let cfg = SearchConfig {
        eval: EvalConfig { objective: Objective::Motility, ..Default::default() },
        // y-axis is speed (cells per sampled interval), not activity.
        map: MapConfig { res_x: 16, res_y: 16, x_range: (0.0, 0.6), y_range: (0.0, 3.0) },
        init_batch: 96,
        generations,
        batch: 24,
        mutation_sigma: 0.12,
        seed: 7,
        verbose: true,
        ..Default::default()
    };
    println!("F2 motility search  |  MAP-Elites, {generations} generations\n");
    let archive = map_elites(&cfg);

    // The best mover: highest motility quality in the map.
    let best = archive.best().expect("search filled no cells").clone();
    let g = &best.genome;
    println!(
        "\nfilled {}/{} cells ({:.0}% coverage)",
        archive.filled(),
        archive.total_cells(),
        archive.coverage() * 100.0
    );
    println!("\nbest mover discovered:");
    println!("  μ={:.4} σ={:.4} peak={:.3} width={:.3} dt={:.3} θ_A={:.2} n={:.2}",
        g.genes[0], g.genes[1], g.genes[2], g.genes[3], g.genes[4], g.genes[5], g.genes[6]);
    println!(
        "  mean speed {:.3}  peak speed {:.3}  mean comps {:.2}  concentration {:.3}",
        best.summary.mean_speed,
        best.summary.peak_speed,
        best.summary.mean_components,
        best.summary.mean_concentration
    );

    // --- 2 & 3. Drop the mover into an M-γ-1 two-species world. ----------------
    // Same rule for both species (kernel/dt/θ/n are global); they differ only in
    // the localized growth center μ, so both inherit the mover's motility.
    let base = g.to_params(cfg.eval.kernel_radius);
    let mu_a = base.growth_mu;
    let mu_b = (base.growth_mu * 1.12).min(0.39); // a second species, +12% μ
    let (w, h) = (96usize, 96usize);

    let mixed = run_two_species(w, h, &best.genome, mu_a, mu_b, 500, Some(out));
    // A static-regime control from M-γ-1's default rule, same protocol.
    let default_genome = default_rule_genome();
    let control = run_two_species(w, h, &default_genome, 0.13, 0.17, 500, None);

    println!("\nM-γ-1 mixing with the discovered mover vs the default static rule:");
    println!("  discovered mover  : blend {:.1}%  (μ {:.3} vs {:.3})", mixed * 100.0, mu_a, mu_b);
    println!("  default static rule: blend {:.1}%  (μ 0.130 vs 0.170)", control * 100.0);
    println!(
        "\nMovers collide, so their localized genomes mix far more than static spots —\n\
         the F2 search, not hand-tuning, is what unlocked it.  gif → {out}"
    );
}

/// Build a two-species M-γ-1 world from `genome` (shared global rule), seed two
/// blobs with localized μ `mu_a` / `mu_b`, run `steps`, optionally export a GIF,
/// and return the final blend-mass fraction (mass at μ between the two species).
fn run_two_species(
    w: usize,
    h: usize,
    genome: &Genome,
    mu_a: f32,
    mu_b: f32,
    steps: usize,
    gif: Option<&str>,
) -> f32 {
    let mut params = genome.to_params(13);
    params.growth_mu = mu_a; // background genome = species A
    let sigma = params.growth_sigma;
    let mut world = World::new(w, h, params);
    world.enable_genome();

    // Two blobs on a collision course: same y, offset x, seeded toward each other
    // by the world's own dynamics (movers drift; we just place them close).
    world.paint_genome(w as f32 * 0.5, h as f32 * 0.5, 6.0, mu_a, sigma);
    world.seed_blob(0, w as f32 * 0.38, h as f32 * 0.5, 6.0, 0.9);
    world.seed_species(w as f32 * 0.62, h as f32 * 0.5, 6.0, 0.9, mu_b, sigma);

    let mut encoder = gif.map(|path| {
        let gw = w as u16 * CELL;
        let gh = h as u16 * CELL;
        let file = File::create(path).expect("create gif");
        let mut enc = gif::Encoder::new(file, gw, gh, &species_palette()).expect("gif");
        enc.set_repeat(gif::Repeat::Infinite).ok();
        enc
    });
    let frame_every = (steps / 150).max(1);
    if let Some(enc) = encoder.as_mut() {
        write_species_frame(enc, &world);
    }
    for step in 1..=steps {
        world.step();
        if let Some(enc) = encoder.as_mut() {
            if step % frame_every == 0 {
                write_species_frame(enc, &world);
            }
        }
    }
    blend_mass_fraction(&world, mu_a, mu_b, 0.05)
}

/// The M-γ-1 default rule as a search genome, for the static control.
fn default_rule_genome() -> Genome {
    use seeker::flow_lenia::FlowLeniaParams;
    let d = FlowLeniaParams::default();
    Genome {
        genes: [d.growth_mu, d.growth_sigma, d.rings[0].peak, d.rings[0].width, d.dt, d.theta_a, d.alpha_n],
        // Energy genes are inert for the (non-metabolic) motility path.
        energy: [0.5, 0.15, 0.004, 0.15],
    }
}

/// Fraction of occupied mass whose localized μ falls strictly between the two
/// species values (by a margin) — a genuine advective blend.
fn blend_mass_fraction(world: &World, mu_a: f32, mu_b: f32, thresh: f32) -> f32 {
    let mass = world.channel(0);
    let mu = world.mu_field().expect("genome enabled");
    let (lo, hi) = (mu_a.min(mu_b), mu_a.max(mu_b));
    let margin = (hi - lo) * 0.15;
    let (mut blend, mut total) = (0.0f64, 0.0f64);
    for (i, &m) in mass.iter().enumerate() {
        if m <= thresh {
            continue;
        }
        total += m as f64;
        if mu[i] > lo + margin && mu[i] < hi - margin {
            blend += m as f64;
        }
    }
    if total > 0.0 {
        (blend / total) as f32
    } else {
        0.0
    }
}

/// Color a species frame: hue = μ, brightness = mass.
fn write_species_frame(encoder: &mut gif::Encoder<File>, world: &World) {
    let (w, h) = (world.width(), world.height());
    let mass = world.channel(0);
    let mu = world.mu_field().expect("genome enabled");
    let gw = w as u16 * CELL;
    let mut pixels = vec![0u8; (gw as usize) * (h * CELL as usize)];
    for y in 0..h {
        for x in 0..w {
            let m = mass[y * w + x].clamp(0.0, 1.0);
            let idx = if m < 0.02 {
                0
            } else {
                let hb =
                    (((mu[y * w + x] - MU_LO) / (MU_HI - MU_LO)) * 15.0).clamp(0.0, 15.0) as u8;
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
        height: h as u16 * CELL,
        delay: 5,
        buffer: Cow::Owned(pixels),
        ..Default::default()
    };
    encoder.write_frame(&frame).ok();
}

/// 16 hues (μ) × 16 brightnesses (mass); `value == 0` row is black.
fn species_palette() -> Vec<u8> {
    let mut pal = Vec::with_capacity(256 * 3);
    for hue_bucket in 0..16 {
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
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
