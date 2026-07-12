//! F2 → M-γ-3: search for a self-sustaining *ecosystem*.
//!
//! The metabolic search (M-γ-2) found organisms that live off a healthy vent.
//! This closes the M-γ-3 loop: every genome now also carries **detritus-cycle**
//! genes (death rate, matter recycling, energy release), and the `Ecosystem`
//! objective evaluates each candidate in a deliberately **scarce** world — a
//! small charge and a weak vent that alone cannot feed the seeded biomass. An
//! organism can only persist by recycling its own dead, so the search co-tunes
//! rule + economy + recycling into a regime that thrives on turnover.
//!
//! The payoff check: the discovered genome is run in that scarce world with
//! recycling ON vs OFF (the M-γ-3 A/B). If recycling is doing the work, the loop
//! sustains living structure the un-recycled control cannot — selection the
//! search found, not one we imposed. Three-panel GIF (matter | detritus | energy).
//!
//! Usage:
//!   cargo run --release --example ecosystem [generations] [out.gif]

use rand::SeedableRng;
use seeker::flow_lenia::{DetritusParams, EnergyParams, FlowLeniaParams, World};
use seeker::harness::field_stats;
use seeker::search::{map_elites, EvalConfig, MapConfig, Objective, SearchConfig};
use std::borrow::Cow;
use std::fs::File;

const CELL: u16 = 3;
const W: usize = 96;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let generations: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(24);
    let out = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("data/ecosystem.gif");

    // --- Search rule + economy + recycling for a scarcity-proof ecosystem. -----
    let cfg = SearchConfig {
        eval: EvalConfig {
            objective: Objective::Ecosystem,
            ..Default::default()
        },
        map: MapConfig::default(),
        init_batch: 96,
        generations,
        batch: 24,
        mutation_sigma: 0.12,
        seed: 13,
        verbose: true,
        ..Default::default()
    };
    println!("F2 ecosystem search  |  MAP-Elites over rule + energy + detritus genes, {generations} generations\n");
    let archive = map_elites(&cfg);
    let best = archive.best().expect("search filled no cells").clone();
    let g = &best.genome;
    let ep = g.to_energy_params();
    let dp = g.to_detritus_params();
    println!(
        "\nfilled {}/{} cells ({:.0}% coverage)",
        archive.filled(),
        archive.total_cells(),
        archive.coverage() * 100.0
    );
    println!("\nbest ecosystem discovered:");
    println!(
        "  rule:     μ={:.4} σ={:.4} peak={:.3} width={:.3} dt={:.3} θ_A={:.2} n={:.2}",
        g.genes[0], g.genes[1], g.genes[2], g.genes[3], g.genes[4], g.genes[5], g.genes[6]
    );
    println!(
        "  economy:  gate_half={:.3} consume={:.3} maintain={:.4} diffusion={:.3}",
        ep.gate_half, ep.consume, ep.maintain, ep.diffusion
    );
    println!(
        "  detritus: death={:.4} recycle_matter={:.4} recycle_energy={:.3}",
        dp.death_rate, dp.recycle_matter, dp.recycle_energy
    );

    // --- A/B: the discovered genome, scarce world, recycling on vs off. ---------
    let base = g.to_params(cfg.eval.kernel_radius);
    let no_recycle = run(&base, &ep, None, 500, None);
    let recycling = run(&base, &ep, Some(&dp), 500, Some(out));

    println!("\ndiscovered ecosystem in the scarce world, recycling off vs on:\n");
    println!("  world              | late conc | late activity | final live | final detritus | final energy | matter drift");
    println!("  -------------------|-----------|---------------|------------|----------------|--------------|-------------");
    print_row("recycling off (M-γ-2)", &no_recycle);
    print_row("recycling on  (M-γ-3)", &recycling);

    // Honest, data-driven read of what the search actually found — no dressing up.
    let act_ratio = recycling.late_activity / no_recycle.late_activity.max(1e-9);
    let conc_ratio = recycling.late_conc / no_recycle.late_conc.max(1e-9);
    let e_max = EnergyParams::default().capacity as f64 * (W * W) as f64;
    let saturated = recycling.final_energy > 0.95 * e_max;
    // Did the search lean on recycling, or escape scarcity by barely dying?
    let death_floor = 0.01; // Bounds::default detritus lower bound
    let minimized_death = dp.death_rate <= death_floor * 1.2;

    println!("\nHonest read of the result:");
    println!(
        "  recycling on vs off: activity ×{act_ratio:.2}, concentration ×{conc_ratio:.2}; \
         detritus pool {:.0}; matter conserved to {:.1e}.",
        recycling.final_detritus, recycling.matter_drift
    );
    if minimized_death || saturated {
        println!(
            "  But the search drove death to its floor ({:.4} vs bound {death_floor}) and the \
             render world's\n  energy {} — so at this grid/horizon the world is not actually \
             scarce within the eval, and\n  recycling is not yet load-bearing. This is Risk #2 \
             (\"the energy layer may do nothing\")\n  made concrete: the ~48²/200-step eval is too \
             short for energy to deplete, so the search\n  escapes scarcity rather than exploiting \
             the loop. Genuine recycling-dependent selection\n  needs real scarcity — finite/\
             decaying sources, longer horizons, denser worlds, or the\n  thermodynamic closure \
             that gives recycling a true cost — i.e. the GPU-scale search.",
            dp.death_rate,
            if saturated { "saturates" } else { "stays high" }
        );
    } else {
        println!(
            "  The search kept a real death+recycle loop (death {:.4}) and recycling lifts the \
             living\n  structure over the un-recycled control — a scarcity-proof ecosystem the \
             search found.",
            dp.death_rate
        );
    }
    println!(
        "\nMechanism note: decomposition energy is not yet debited against build cost, so the \
         loop\nis idealized (thermodynamic-closure open question). The conserved, tested invariant \
         is\n*matter*. gif → {out}  (matter | detritus | energy)"
    );
}

struct Report {
    final_live: f64,
    final_detritus: f64,
    final_energy: f64,
    late_conc: f64,
    late_activity: f64,
    matter_drift: f64,
}

fn print_row(label: &str, r: &Report) {
    println!(
        "  {label} | {:9.3} | {:13.2e} | {:10.2} | {:14.2} | {:12.2} | {:11.1e}",
        r.late_conc,
        r.late_activity,
        r.final_live,
        r.final_detritus,
        r.final_energy,
        r.matter_drift
    );
}

/// Run the discovered genome in the scarce ecosystem world (matching the search's
/// eval setup, scaled to the render grid). `dp = Some` enables recycling (M-γ-3);
/// `None` is the un-recycled M-γ-2 control. Optional matter|detritus|energy GIF.
fn run(
    base: &FlowLeniaParams,
    ep: &EnergyParams,
    dp: Option<&DetritusParams>,
    steps: usize,
    gif: Option<&str>,
) -> Report {
    let mut world = World::new(W, W, base.clone());
    world.enable_energy(ep.clone());
    let c = W as f32 * 0.5;
    // Scarce world: quarter charge + weak vent (same recipe the search evaluated).
    world.charge_energy(ep.capacity * 0.25);
    world.add_source(c, c, W as f32 / 4.0, 0.2);
    if let Some(dp) = dp {
        world.enable_detritus(dp.clone());
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(20240703);
    world.seed_random_patch(&mut rng, 0, c, c, W as f32 / 3.0, 0.6);

    let cap = ep.capacity;
    let initial_matter = world.total_mass() + world.total_detritus().unwrap_or(0.0);

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
            late_conc += field_stats(&combined_matter(&world), 0.05).concentration as f64;
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
