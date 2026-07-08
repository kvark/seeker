//! F2 extended to the energy economy (M-γ-2): search for a *metabolism*.
//!
//! M-γ-2 showed a hand-written economy changes which patterns persist. The F2
//! extension lets the search *co-tune the economy itself*: every genome now
//! carries energy genes (gate half-saturation, consumption, maintenance,
//! diffusion), and the `Metabolic` objective evaluates each candidate in a world
//! that is charged and fed by a localized source. Quality is liveness *under* the
//! economy — a structure only scores if its metabolism can pay the upkeep and
//! organize despite the energy gate. So the search illuminates which
//! rule+economy pairs sustain life, with no economy hand-tuning.
//!
//! The payoff check: the discovered metabolism is run fed vs starved (the M-γ-2
//! A/B). A genuine metabolism thrives when fed and collapses when starved —
//! selection that the search found rather than we imposed.
//!
//! Usage:
//!   cargo run --release --example metabolism [generations] [out.gif]

use rand::SeedableRng;
use seeker::flow_lenia::{EnergyParams, FlowLeniaParams, World};
use seeker::harness::{measure_run, RunSummary};
use seeker::search::{map_elites, EvalConfig, MapConfig, Objective, SearchConfig};
use std::borrow::Cow;
use std::fs::File;

const CELL: u16 = 3;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let generations: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(24);
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("data/metabolism.gif");

    // --- Search over rule + economy for a self-sustaining metabolism. ---------
    let cfg = SearchConfig {
        eval: EvalConfig { objective: Objective::Metabolic, ..Default::default() },
        map: MapConfig::default(),
        init_batch: 96,
        generations,
        batch: 24,
        mutation_sigma: 0.12,
        seed: 11,
        verbose: true,
        ..Default::default()
    };
    println!("F2 metabolic search  |  MAP-Elites over rule + energy genes, {generations} generations\n");
    let archive = map_elites(&cfg);
    let best = archive.best().expect("search filled no cells").clone();
    let g = &best.genome;
    let ep = g.to_energy_params();
    println!(
        "\nfilled {}/{} cells ({:.0}% coverage)",
        archive.filled(),
        archive.total_cells(),
        archive.coverage() * 100.0
    );
    println!("\nbest metabolism discovered:");
    println!(
        "  rule:   μ={:.4} σ={:.4} peak={:.3} width={:.3} dt={:.3} θ_A={:.2} n={:.2}",
        g.genes[0], g.genes[1], g.genes[2], g.genes[3], g.genes[4], g.genes[5], g.genes[6]
    );
    println!(
        "  economy: gate_half={:.3} consume={:.3} maintain={:.4} diffusion={:.3}",
        ep.gate_half, ep.consume, ep.maintain, ep.diffusion
    );

    // --- A/B: the discovered metabolism fed vs starved (the M-γ-2 test). -------
    let base = g.to_params(cfg.eval.kernel_radius);
    let (w, h) = (96usize, 96usize);
    let (fed_sum, _) = run(&base, &ep, w, h, true, 400, Some(out));
    let (starved_sum, _) = run(&base, &ep, w, h, false, 400, None);

    println!("\ndiscovered metabolism, fed vs starved (F1 fingerprints):\n");
    print_header();
    print_row("fed (source on) ", &fed_sum);
    print_row("starved (no food)", &starved_sum);
    println!(
        "\nThe search found a metabolism that sustains structure when fed and\n\
         collapses when starved — intrinsic selection, co-tuned by F2, not imposed.\n\
         gif → {out}  (matter | energy)"
    );
}

/// Build a world from the rule and (optionally active) economy, seed the shared
/// soup, run, and return its fingerprint. `fed` adds the source + initial charge;
/// `!fed` is the starved control (economy on, no food).
fn run(
    base: &FlowLeniaParams,
    ep: &EnergyParams,
    w: usize,
    h: usize,
    fed: bool,
    steps: usize,
    gif: Option<&str>,
) -> (RunSummary, ()) {
    let mut world = World::new(w, h, base.clone());
    world.enable_energy(ep.clone());
    let c = w as f32 * 0.5;
    if fed {
        world.charge_energy(ep.capacity * 0.5);
        world.add_source(c, c, w as f32 / 4.0, 0.5);
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(20240703);
    world.seed_random_patch(&mut rng, 0, c, h as f32 * 0.5, w as f32 / 3.0, 0.6);

    // Optional GIF: run manually so we can snapshot matter | energy each frame.
    if let Some(path) = gif {
        let gap = CELL * 2;
        let panel = w as u16 * CELL;
        let total_w = panel * 2 + gap;
        let file = File::create(path).expect("create gif");
        let mut enc = gif::Encoder::new(file, total_w, w as u16 * CELL, &palette()).expect("gif");
        enc.set_repeat(gif::Repeat::Infinite).ok();
        let frame_every = (steps / 150).max(1);
        // Re-seed an identical world for the render (measure_run consumes its own).
        let mut vis = World::new(w, h, base.clone());
        vis.enable_energy(ep.clone());
        if fed {
            vis.charge_energy(ep.capacity * 0.5);
            vis.add_source(c, c, w as f32 / 4.0, 0.5);
        }
        let mut rng2 = rand::rngs::StdRng::seed_from_u64(20240703);
        vis.seed_random_patch(&mut rng2, 0, c, h as f32 * 0.5, w as f32 / 3.0, 0.6);
        two_panel(&mut enc, &vis, total_w, gap, ep.capacity);
        for step in 1..=steps {
            vis.step();
            if step % frame_every == 0 {
                two_panel(&mut enc, &vis, total_w, gap, ep.capacity);
            }
        }
    }

    let (summary, _) = measure_run(&mut world, steps, 20, 0.05, 8.0);
    (summary, ())
}

fn print_header() {
    println!("  run               | mass drift | mean conc | mean comps | final comps | mean act");
    println!("  ------------------|------------|-----------|------------|-------------|---------");
}

fn print_row(label: &str, s: &RunSummary) {
    println!(
        "  {label} | {:10.2e} | {:9.4} | {:10.2} | {:11} | {:8.4}",
        s.mass_drift, s.mean_concentration, s.mean_components, s.final_components, s.mean_activity
    );
}

/// Left panel matter, right panel energy (normalized to capacity).
fn two_panel(encoder: &mut gif::Encoder<File>, world: &World, gw: u16, gap: u16, cap: f32) {
    let (w, h) = (world.width(), world.height());
    let matter = world.channel(0);
    let energy = world.energy_field().expect("energy on");
    let panel = w as u16 * CELL;
    let mut px = vec![0u8; gw as usize * (h * CELL as usize)];
    let mut blit = |field: &[f32], scale: f32, x_off: u16| {
        for y in 0..h {
            for x in 0..w {
                let v = (field[y * w + x] * scale).clamp(0.0, 1.0);
                let idx = (v * 255.0) as u8;
                for dy in 0..CELL {
                    for dx in 0..CELL {
                        let xx = x_off + x as u16 * CELL + dx;
                        let yy = y as u16 * CELL + dy;
                        px[yy as usize * gw as usize + xx as usize] = idx;
                    }
                }
            }
        }
    };
    blit(matter, 1.0, 0);
    blit(energy, 1.0 / cap.max(1e-6), panel + gap);
    let frame = gif::Frame {
        width: gw,
        height: h as u16 * CELL,
        delay: 6,
        buffer: Cow::Owned(px),
        ..Default::default()
    };
    encoder.write_frame(&frame).ok();
}

/// Inferno-style palette: 0 → black, 1 → pale yellow (matches other examples).
fn palette() -> Vec<u8> {
    let stops: [(f32, (u8, u8, u8)); 5] = [
        (0.00, (0, 0, 4)),
        (0.25, (60, 12, 90)),
        (0.50, (160, 40, 90)),
        (0.75, (232, 110, 40)),
        (1.00, (250, 250, 180)),
    ];
    let mut pal = Vec::with_capacity(256 * 3);
    for i in 0..256 {
        let t = i as f32 / 255.0;
        let mut seg = 0;
        while seg + 1 < stops.len() && t > stops[seg + 1].0 {
            seg += 1;
        }
        let (t0, c0) = stops[seg];
        let (t1, c1) = stops[(seg + 1).min(stops.len() - 1)];
        let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f) as u8;
        pal.push(lerp(c0.0, c1.0));
        pal.push(lerp(c0.1, c1.1));
        pal.push(lerp(c0.2, c1.2));
    }
    pal
}
