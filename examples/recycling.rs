//! M-γ-3 demonstration: closed-loop detritus recycling — a world that *recycles
//! instead of freezing*.
//!
//! The setup is a **closed world**: an initial energy charge and no external vent.
//! That isolates recycling as the only thing that can keep the world going, so the
//! A/B is clean:
//!
//! - **M-γ-2 (recycling off).** The charge is spent on growth + upkeep, the gate
//!   closes, matter disperses and goes inert. The world *freezes* — activity → 0.
//! - **M-γ-3 (recycling on).** Starved matter dies into an inert detritus channel;
//!   detritus decomposes back into the live channel and releases energy as it rots.
//!   Dead matter becomes food, so regrowth reignites — a *sustained, non-collapsing*
//!   turnover of the same conserved matter. `Σ live + Σ detritus` is constant.
//!
//! A three-panel GIF (matter | detritus | energy) is exported for the M-γ-3 world.
//!
//! Usage:
//!   cargo run --release --example recycling [steps] [out.gif]

use rand::SeedableRng;
use seeker::flow_lenia::{DetritusParams, EnergyParams, FlowLeniaParams, World};
use seeker::harness::field_stats;
use std::borrow::Cow;
use std::fs::File;

const CELL: u16 = 3;
const W: usize = 96;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let steps: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("data/recycling.gif");
    // Starved regime: a small initial charge and a weak vent that alone cannot
    // support the seeded biomass. Tunable so the regime is explicit, not hidden.
    let charge: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.3);
    let source: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.0);

    let vent = if source > 0.0 { format!("weak vent {source}") } else { "no vent".to_string() };
    println!("M-γ-3 detritus recycling  |  closed world (charge {charge}, {vent}), {steps} steps\n");

    // A/B: identical closed world, recycling off vs on. The only external energy is
    // the initial charge — so whatever keeps the M-γ-3 world going is its own dead.
    let depleting = run(false, steps, charge, source, None);
    let cycling = run(true, steps, charge, source, Some(out));
    let e0 = charge as f64 * (W * W) as f64;

    println!("closed world, recycling off (M-γ-2) vs on (M-γ-3):\n");
    println!("  world              | late conc | late activity | final live | final detritus | final energy | matter drift");
    println!("  -------------------|-----------|---------------|------------|----------------|--------------|-------------");
    print_row("recycling off (M-γ-2)", &depleting);
    print_row("recycling on  (M-γ-3)", &cycling);

    let act_uplift = cycling.late_activity / depleting.late_activity.max(1e-9);
    println!(
        "\nInitial energy budget: {e0:.0} (charge × cells), no external source. Recycling off,\n\
         the economy only runs down ({e0:.0} → {:.0}) and death is irreversible dispersal — no\n\
         detritus. Recycling on, dying matter becomes a concentrated detritus pool ({:.0}) that\n\
         decomposes back and *regenerates energy* ({e0:.0} → {:.0}, net positive): the world feeds\n\
         on its own dead and keeps cycling (late activity ~{:.1}× higher). Matter is conserved\n\
         across live+detritus to {:.1e}.\n\n\
         Honest caveat: decomposition here releases energy with no accounting against what was\n\
         spent to build that matter, so a closed world is net energy-positive — the idealized\n\
         'dead matter is food' mechanism, not yet thermodynamically closed (an M-γ-3 open\n\
         question). The conserved, testable invariant is *matter*, not energy.\n\
         gif → {out}  (matter | detritus | energy)",
        depleting.final_energy, cycling.final_detritus, cycling.final_energy, act_uplift, cycling.matter_drift
    );
}

/// What we record about a run.
struct Report {
    final_live: f64,
    final_detritus: f64,
    final_energy: f64,
    /// Mean concentration of *all* matter (live + detritus) over the last third —
    /// structure retention. Mass is conserved, so total mass is not the signal;
    /// whether that mass stays organized or bleeds into a flat film is.
    late_conc: f64,
    /// Mean per-step activity of the live field over the last third — still alive?
    late_activity: f64,
    /// Max relative drift of (live + detritus) over the run — the M-γ-3 invariant.
    matter_drift: f64,
}

fn print_row(label: &str, r: &Report) {
    println!(
        "  {label} | {:9.3} | {:13.2e} | {:10.2} | {:14.2} | {:12.2} | {:11.1e}",
        r.late_conc, r.late_activity, r.final_live, r.final_detritus, r.final_energy, r.matter_drift
    );
}

/// Run one starved world. `recycle` toggles M-γ-3. When `gif` is set, export a
/// matter|detritus|energy panel every few steps.
fn run(recycle: bool, steps: usize, charge: f32, source: f32, gif: Option<&str>) -> Report {
    let mut world = World::new(W, W, FlowLeniaParams::default());
    world.enable_energy(EnergyParams::default());
    if recycle {
        world.enable_detritus(DetritusParams::default());
    }
    world.charge_energy(charge);
    if source > 0.0 {
        world.add_source(W as f32 * 0.5, W as f32 * 0.5, 12.0, source);
    }
    seed(&mut world);

    let cap = EnergyParams::default().capacity;
    let initial_matter = world.total_mass() + world.total_detritus().unwrap_or(0.0);

    // GIF geometry: three panels side by side.
    let panel = W as u16 * CELL;
    let gap = CELL * 2;
    let total_w = panel * 3 + gap * 2;
    let mut encoder = gif.map(|path| {
        let file = File::create(path).expect("create gif");
        let mut e = gif::Encoder::new(file, total_w, W as u16 * CELL, &palette()).expect("gif");
        e.set_repeat(gif::Repeat::Infinite).ok();
        e
    });
    let frame_every = (steps / 150).max(1);

    let mut prev = world.mass_field();
    let mut late_activity = 0.0f64;
    let mut late_conc = 0.0f64;
    let mut late_count = 0usize;
    let mut matter_drift = 0.0f64;
    let late_start = steps * 2 / 3;

    for step in 1..=steps {
        world.step();
        // Activity = mean absolute change of the live field this step.
        let cur = world.mass_field();
        let act: f64 = cur
            .iter()
            .zip(&prev)
            .map(|(a, b)| (*a - *b).abs() as f64)
            .sum::<f64>()
            / cur.len() as f64;
        prev = cur;
        let total = world.total_mass() + world.total_detritus().unwrap_or(0.0);
        matter_drift = matter_drift.max((total - initial_matter).abs() / initial_matter);
        if step >= late_start {
            late_activity += act;
            // Concentration of *all* matter: fold detritus back in so a world that
            // parked its mass as (concentrated) detritus is credited for structure.
            let combined = combined_matter(&world);
            late_conc += field_stats(&combined, 0.05).concentration as f64;
            late_count += 1;
        }

        if let Some(enc) = encoder.as_mut() {
            if step % frame_every == 0 {
                three_panel(enc, &world, total_w, gap, cap);
            }
        }
    }

    let n = late_count.max(1) as f64;
    Report {
        final_live: world.total_mass(),
        final_detritus: world.total_detritus().unwrap_or(0.0),
        final_energy: world.total_energy().unwrap_or(0.0),
        late_conc: late_conc / n,
        late_activity: late_activity / n,
        matter_drift,
    }
}

/// Live matter + detritus, cell by cell — the full conserved matter field.
fn combined_matter(world: &World) -> Vec<f32> {
    let mut f = world.mass_field();
    if let Some(det) = world.detritus_field() {
        for (a, d) in f.iter_mut().zip(det) {
            *a += *d;
        }
    }
    f
}

/// The same scatter of matter for both runs, so they differ only in recycling.
fn seed(world: &mut World) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(20240705);
    world.seed_blob(0, 48.0, 48.0, 9.0, 0.95);
    world.seed_blob(0, 28.0, 64.0, 6.0, 0.9);
    world.seed_blob(0, 68.0, 32.0, 6.0, 0.9);
    world.seed_random_patch(&mut rng, 0, 60.0, 72.0, 10.0, 0.6);
}

/// matter | detritus | energy, left to right.
fn three_panel(encoder: &mut gif::Encoder<File>, world: &World, gw: u16, gap: u16, cap: f32) {
    let (w, h) = (world.width(), world.height());
    let panel = w as u16 * CELL;
    let matter = world.channel(0);
    let detritus = world.detritus_field();
    let energy = world.energy_field().expect("energy on");
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
    // Detritus is faint; brighten it so the dead pool is visible.
    if let Some(det) = detritus {
        blit(det, 3.0, panel + gap);
    }
    blit(energy, 1.0 / cap.max(1e-6), (panel + gap) * 2);
    let frame = gif::Frame {
        width: gw,
        height: h as u16 * CELL,
        delay: 6,
        buffer: Cow::Owned(px),
        ..Default::default()
    };
    encoder.write_frame(&frame).ok();
}

/// Inferno-style palette shared with the other examples.
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
