//! F2 outer-loop search: MAP-Elites illumination of Flow-Lenia rule space.
//!
//! Runs the search, prints the filled behavior map (concentration × activity),
//! and reports the elite rules in notable regions — the rules the search
//! discovered, not ones we hand-tuned.
//!
//! After the search, the highest-quality discovered rule is re-run and exported
//! as data/evolved-best.gif — illuminate the space, then watch the winner.
//!
//! Usage:
//!   cargo run --release --example search [generations] [init_batch] [batch]

use rand::{rngs::StdRng, SeedableRng};
use seeker::flow_lenia::World;
use seeker::search::{map_elites, EvalConfig, Evaluated, MapElites, SearchConfig};
use std::borrow::Cow;
use std::fs::File;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let generations = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let init_batch = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
    let batch = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

    let cfg = SearchConfig {
        generations,
        init_batch,
        batch,
        verbose: true,
        ..Default::default()
    };

    println!(
        "MAP-Elites over Flow-Lenia rule space\n  grid {}²  steps {}  |  init {}  gens {}  batch {}  = {} evaluations",
        cfg.eval.grid_size,
        cfg.eval.steps,
        cfg.init_batch,
        cfg.generations,
        cfg.batch,
        cfg.init_batch + cfg.generations * cfg.batch,
    );

    let archive = map_elites(&cfg);

    print_map(&archive);

    println!(
        "\nCoverage: {}/{} cells ({:.0}%)",
        archive.filled(),
        archive.total_cells(),
        archive.coverage() * 100.0
    );

    // Notable elites.
    let alive: Vec<&Evaluated> = archive
        .occupied()
        .filter(|e| e.quality > 0.25) // above the "dead" persistence floor
        .collect();

    if let Some(best) = archive.best() {
        print_elite("highest quality", best);
    }
    if let Some(e) = alive
        .iter()
        .max_by(|a, b| a.summary.mean_activity.partial_cmp(&b.summary.mean_activity).unwrap())
    {
        print_elite("most dynamic (alive)", e);
    }
    if let Some(e) = alive
        .iter()
        .max_by(|a, b| a.summary.mean_concentration.partial_cmp(&b.summary.mean_concentration).unwrap())
    {
        print_elite("most concentrated (alive)", e);
    }
    if let Some(e) = alive
        .iter()
        .max_by(|a, b| a.summary.peak_speed.partial_cmp(&b.summary.peak_speed).unwrap())
    {
        print_elite("most motile (alive)", e);
    }

    // Watch the winner: re-run the best rule and export a GIF.
    if let Some(best) = archive.best() {
        let out = "data/evolved-best.gif";
        render_best(best, &cfg.eval, out);
        println!("\nBest discovered rule → {out}");
    }
}

/// Re-run a discovered rule from the same fixed soup and write an animated GIF
/// (upscaled), so the search result is watchable, not just a row of numbers.
fn render_best(best: &Evaluated, eval: &EvalConfig, path: &str) {
    const CELL: u16 = 6;
    let g = eval.grid_size;
    let mut world = World::new(g, g, best.genome.to_params(eval.kernel_radius));
    let mut rng = StdRng::seed_from_u64(eval.seed);
    let c = g as f32 * 0.5;
    world.seed_random_patch(&mut rng, 0, c, c, g as f32 / 3.0, 0.6);

    let gw = g as u16 * CELL;
    let palette = inferno_palette();
    let file = File::create(path).expect("create gif");
    let mut enc = gif::Encoder::new(file, gw, gw, &palette).expect("gif encoder");
    enc.set_repeat(gif::Repeat::Infinite).ok();

    let frame_every = (eval.steps / 150).max(1);
    write_frame(&mut enc, &world, gw, CELL);
    for step in 1..=eval.steps {
        world.step();
        if step % frame_every == 0 {
            write_frame(&mut enc, &world, gw, CELL);
        }
    }
}

fn write_frame(enc: &mut gif::Encoder<File>, world: &World, gw: u16, cell: u16) {
    let field = world.channel(0);
    let w = world.width();
    let h = world.height();
    let mut pixels = vec![0u8; gw as usize * gw as usize];
    for y in 0..h {
        for x in 0..w {
            let idx = (field[y * w + x].clamp(0.0, 1.0) * 255.0) as u8;
            for dy in 0..cell {
                for dx in 0..cell {
                    let px = x as u16 * cell + dx;
                    let py = y as u16 * cell + dy;
                    pixels[py as usize * gw as usize + px as usize] = idx;
                }
            }
        }
    }
    let frame = gif::Frame {
        width: gw,
        height: gw,
        delay: 6,
        buffer: Cow::Owned(pixels),
        ..Default::default()
    };
    enc.write_frame(&frame).ok();
}

fn inferno_palette() -> Vec<u8> {
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

/// ASCII heatmap of the archive: rows = activity (top = high), cols =
/// concentration (right = high). Character encodes elite quality.
fn print_map(a: &MapElites) {
    let cfg = a.config();
    let glyph = |q: f32| -> char {
        if q < 0.25 {
            '.' // present but "dead" (below persistence floor)
        } else if q < 0.5 {
            ':'
        } else if q < 0.8 {
            '+'
        } else if q < 1.0 {
            '*'
        } else {
            '#'
        }
    };
    println!("\nBehavior map  (rows: activity ↑,  cols: concentration →)");
    println!("  legend: ' ' empty  '.' dead  ':' '+' '*' '#' rising liveness\n");
    for iy in (0..cfg.res_y).rev() {
        print!("  ");
        for ix in 0..cfg.res_x {
            match a.cell(ix, iy) {
                Some(e) => print!("{}", glyph(e.quality)),
                None => print!(" "),
            }
        }
        println!();
    }
    println!(
        "  concentration {:.2}→{:.2}   activity {:.3}→{:.3}",
        cfg.x_range.0, cfg.x_range.1, cfg.y_range.0, cfg.y_range.1
    );
}

fn print_elite(label: &str, e: &Evaluated) {
    let g = &e.genome.genes;
    println!("\n── {label} ──");
    println!(
        "  quality {:.3}  |  concentration {:.3}  activity {:.5}  components {:.1}  peak-speed {:.2}",
        e.quality, e.summary.mean_concentration, e.summary.mean_activity, e.summary.mean_components, e.summary.peak_speed
    );
    println!(
        "  rule: μ={:.3} σ={:.4} ring(peak={:.2},width={:.2}) dt={:.3} θ_A={:.2} n={:.2}",
        g[0], g[1], g[2], g[3], g[4], g[5], g[6]
    );
}
