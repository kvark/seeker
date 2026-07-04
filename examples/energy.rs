//! M-γ-2 demonstration: the energy economy as an *intrinsic* selective pressure.
//!
//! Runs two worlds from the **identical** seed and physics, differing only in the
//! economy: one is fed by renewable energy sources, the other is starved. The
//! measurement harness (F1) folds each run into a behavior fingerprint, so the
//! claim — "energy competition changes *which* patterns persist" — is a number,
//! not a vibe. A two-panel GIF (matter | energy) is exported for the fed world.
//!
//! Usage:
//!   cargo run --release --example energy [steps] [out.gif]
//!
//! The energy layer is toggleable by construction; this example is the A/B the
//! discipline note in CLAUDE.md asks every experiment to run against pure
//! Flow-Lenia.

use rand::SeedableRng;
use seeker::flow_lenia::{EnergyParams, FlowLeniaParams, World};
use seeker::harness::{measure_run, RunSummary};
use std::borrow::Cow;
use std::fs::File;

const CELL: u16 = 3; // upscale factor for the GIF

/// Seed the same matter into any world so the two runs differ only in energy.
fn seed(world: &mut World) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(20240704);
    // A scatter of blobs across the world — some will fall near sources, some in
    // the energy desert. No hand-placement of "winners": the economy decides.
    world.seed_blob(0, 64.0, 64.0, 8.0, 0.95);
    world.seed_blob(0, 32.0, 40.0, 6.0, 0.9);
    world.seed_blob(0, 96.0, 88.0, 6.0, 0.9);
    world.seed_random_patch(&mut rng, 0, 48.0, 96.0, 10.0, 0.6);
}

/// Place renewable sources: a bright vent at the center, a dimmer one off-corner.
/// Matter that finds and holds these persists; the rest starves.
fn add_sources(world: &mut World) {
    world.add_source(64.0, 64.0, 16.0, 0.5);
    world.add_source(96.0, 88.0, 12.0, 0.35);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let steps: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("data/energy.gif");
    let (w, h) = (128usize, 128usize);

    let energy_params = EnergyParams::default();

    // --- Fed world: energy economy on, charged, with renewable sources. --------
    let mut fed = World::new(w, h, FlowLeniaParams::default());
    fed.enable_energy(energy_params.clone());
    fed.charge_energy(energy_params.capacity * 0.5);
    add_sources(&mut fed);
    seed(&mut fed);

    // --- Starved world: energy economy on, but no charge and no sources. -------
    let mut starved = World::new(w, h, FlowLeniaParams::default());
    starved.enable_energy(energy_params.clone());
    seed(&mut starved);

    // --- Baseline: pure Flow-Lenia (economy off) — the A/B control. ------------
    let mut pure = World::new(w, h, FlowLeniaParams::default());
    seed(&mut pure);

    println!("Flow-Lenia M-γ-2  |  {w}×{h}, {steps} steps  |  energy economy A/B\n");

    // The GIF is exported from the fed world so we can watch matter track energy.
    // measure_run drives the world internally, so render first, then measure a
    // fresh identically-seeded copy for the fingerprint.
    export_gif(out, w, h, steps, &energy_params);

    let (pure_sum, _) = measure_run(&mut pure, steps, 20, 0.05, 8.0);
    let (fed_sum, _) = measure_run(&mut fed, steps, 20, 0.05, 8.0);
    let (starved_sum, _) = measure_run(&mut starved, steps, 20, 0.05, 8.0);

    println!("behavior fingerprints (F1 harness):\n");
    print_header();
    print_row("pure (economy off)", &pure_sum);
    print_row("starved (no food) ", &starved_sum);
    print_row("fed (sources on)  ", &fed_sum);

    let fed_energy = fed.total_energy().unwrap_or(0.0);
    let starved_energy = starved.total_energy().unwrap_or(0.0);
    println!(
        "\nfinal stored energy: fed = {fed_energy:.0}  |  starved = {starved_energy:.0}  |  gif → {out}"
    );
    println!(
        "\nread: the fed world should hold more/*steadier* structure (concentration,\n\
         final components) than the starved one, with the economy — not a fitness\n\
         function — deciding which blobs survive."
    );
}

fn print_header() {
    println!(
        "  run                | mass drift | mean conc | mean comps | final comps | mean act | peak speed"
    );
    println!(
        "  -------------------|------------|-----------|------------|-------------|----------|-----------"
    );
}

fn print_row(label: &str, s: &RunSummary) {
    println!(
        "  {label} | {:10.2e} | {:9.4} | {:10.2} | {:11} | {:8.4} | {:10.3}",
        s.mass_drift, s.mean_concentration, s.mean_components, s.final_components, s.mean_activity, s.peak_speed
    );
}

/// Drive a fed world forward, writing a side-by-side (matter | energy) GIF.
fn export_gif(out: &str, w: usize, h: usize, steps: usize, ep: &EnergyParams) {
    let mut world = World::new(w, h, FlowLeniaParams::default());
    world.enable_energy(ep.clone());
    world.charge_energy(ep.capacity * 0.5);
    add_sources(&mut world);
    seed(&mut world);

    let panel_w = w as u16 * CELL;
    let gh = h as u16 * CELL;
    let gap: u16 = CELL * 2;
    let gw = panel_w * 2 + gap;
    let palette = build_palette();
    let file = File::create(out).expect("create gif");
    let mut encoder = gif::Encoder::new(file, gw, gh, &palette).expect("gif encoder");
    encoder.set_repeat(gif::Repeat::Infinite).ok();

    let frame_every = (steps / 150).max(1);
    write_two_panel(&mut encoder, &world, gw, gh, gap, ep.capacity);
    for step in 1..=steps {
        world.step();
        if step % frame_every == 0 {
            write_two_panel(&mut encoder, &world, gw, gh, gap, ep.capacity);
        }
    }
}

/// Left panel: matter (channel 0). Right panel: energy, normalized to capacity.
fn write_two_panel(
    encoder: &mut gif::Encoder<File>,
    world: &World,
    gw: u16,
    gh: u16,
    gap: u16,
    cap: f32,
) {
    let (w, h) = (world.width(), world.height());
    let matter = world.channel(0);
    let energy = world.energy_field().expect("energy enabled");
    let panel_w = w as u16 * CELL;
    let mut pixels = vec![0u8; gw as usize * gh as usize];
    let mut blit = |field: &[f32], scale: f32, x_off: u16| {
        for y in 0..h {
            for x in 0..w {
                let v = (field[y * w + x] * scale).clamp(0.0, 1.0);
                let idx = (v * 255.0) as u8;
                for dy in 0..CELL {
                    for dx in 0..CELL {
                        let px = x_off + x as u16 * CELL + dx;
                        let py = y as u16 * CELL + dy;
                        pixels[py as usize * gw as usize + px as usize] = idx;
                    }
                }
            }
        }
    };
    blit(matter, 1.0, 0);
    blit(energy, 1.0 / cap.max(1e-6), panel_w + gap);
    let frame = gif::Frame {
        width: gw,
        height: gh,
        delay: 6,
        buffer: Cow::Owned(pixels),
        ..Default::default()
    };
    encoder.write_frame(&frame).ok();
}

/// Same inferno-style palette as the flow_lenia example: 0 → black, 1 → pale.
fn build_palette() -> Vec<u8> {
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
