//! F2 outer-loop search: MAP-Elites illumination of Flow-Lenia rule space.
//!
//! Runs the search, prints the filled behavior map (concentration × activity),
//! and reports the elite rules in notable regions — the rules the search
//! discovered, not ones we hand-tuned.
//!
//! Usage:
//!   cargo run --release --example search [generations] [init_batch] [batch]

use seeker::search::{map_elites, Evaluated, MapElites, SearchConfig};

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
