use seeker::emergence::{rule_transect_cfg, rule_slice_2d, find_critical_rules, TransectConfig};
use seeker::rules;

fn run_transect(
    name: &str,
    spawn_a: &[f32; 9],
    keep_a: &[f32; 9],
    spawn_b: &[f32; 9],
    keep_b: &[f32; 9],
    cfg: &TransectConfig,
) {
    println!("\n=== {name} ===");
    let points = rule_transect_cfg(spawn_a, keep_a, spawn_b, keep_b, 21, 42, cfg);
    println!("  t    | Spread | Crit | Complex | Entropy | AutoCorr | Alive  | MeanField");
    println!("-------|--------|------|---------|---------|----------|--------|----------");
    for p in &points {
        println!(
            "  {:.2}  | {:.3}  | {:3}  |  {:5.1} |  {:.3}  |  {:.3}   | {:.4} |  {:.3}",
            p.t,
            p.derrida.spreading_rate,
            p.derrida.criticality_score(),
            p.complexity.complexity,
            p.complexity.entropy,
            p.complexity.autocorrelation,
            p.alive_ratio,
            p.mean_field
        );
    }
}

fn run_2d_slice(
    name: &str,
    base_spawn: &[f32; 9],
    base_keep: &[f32; 9],
    x_index: usize,
    y_index: usize,
    x_label: &str,
    y_label: &str,
    cfg: &TransectConfig,
) {
    let res = 11;
    println!("\n=== 2D Slice: {name} ===");
    println!("X axis: {x_label} (index {x_index}), Y axis: {y_label} (index {y_index})");
    let points = rule_slice_2d(base_spawn, base_keep, x_index, y_index, res, 42, cfg);

    // Print Derrida heatmap
    println!("\nDerrida spreading rate:");
    print!("       ");
    for xi in 0..res {
        print!(" {:.1} ", xi as f32 / (res - 1) as f32);
    }
    println!();
    for yi in 0..res {
        let y_val = yi as f32 / (res - 1) as f32;
        print!("  {:.1}  |", y_val);
        for xi in 0..res {
            let p = &points[yi * res + xi];
            if p.derrida.spreading_rate == 0.0 {
                print!("  -  ");
            } else if p.derrida.criticality_score() >= 90 {
                print!(" *{:.1}", p.derrida.spreading_rate);
            } else {
                print!("  {:.1}", p.derrida.spreading_rate);
            }
        }
        println!();
    }

    // Print complexity heatmap
    println!("\nComplexity score:");
    print!("       ");
    for xi in 0..res {
        print!(" {:.1} ", xi as f32 / (res - 1) as f32);
    }
    println!();
    for yi in 0..res {
        let y_val = yi as f32 / (res - 1) as f32;
        print!("  {:.1}  |", y_val);
        for xi in 0..res {
            let p = &points[yi * res + xi];
            if p.alive_ratio == 0.0 {
                print!("  -  ");
            } else {
                print!(" {:4.1}", p.complexity.complexity);
            }
        }
        println!();
    }
}

fn main() {
    let cfg = TransectConfig {
        grid_size: 64,
        sim_steps: 1000,
        num_seeds: 4,
        derrida_steps: 50,
    };

    let (gol_s, gol_k) = rules::b3s23();
    let (hl_s, hl_k) = rules::b36s23();
    let (seeds_s, seeds_k) = rules::b2s();
    let (dn_s, dn_k) = rules::b3678s34678();

    // --- Phase E.1: Probabilistic transects (smooth gradients) ---
    println!("########## 1D TRANSECTS (multi-seed averaged) ##########");
    run_transect("GoL (B3/S23) → HighLife (B36/S23)", &gol_s, &gol_k, &hl_s, &hl_k, &cfg);
    run_transect("GoL (B3/S23) → Seeds (B2/S)", &gol_s, &gol_k, &seeds_s, &seeds_k, &cfg);
    run_transect("GoL (B3/S23) → Day & Night (B3678/S34678)", &gol_s, &gol_k, &dn_s, &dn_k, &cfg);

    // --- Phase E.2: 2D slices ---
    println!("\n\n########## 2D SLICES ##########");
    // Vary spawn[2] × spawn[3] with GoL keep (S23)
    run_2d_slice(
        "spawn[2] × spawn[3] (GoL keep S23)",
        &[0.0; 9], &gol_k, 2, 3, "spawn[2]", "spawn[3]", &cfg,
    );
    // Vary keep[2] × keep[3] with GoL spawn (B3)
    run_2d_slice(
        "keep[2] × keep[3] (GoL spawn B3)",
        &gol_s, &[0.0; 9], 11, 12, "keep[2]", "keep[3]", &cfg,
    );

    // --- Phase E.5: Critical surface search ---
    println!("\n\n########## CRITICAL SURFACE SEARCH ##########");
    println!("Sampling 200 random viable rules...");
    let critical = find_critical_rules(200, 42, &cfg);
    println!("Found {} rules with measurable Derrida signal", critical.len());
    println!("\nTop 20 most critical rules:");
    println!("  # | Crit | Spread | Cmplx | Alive | MF    | Spawn                          | Keep");
    println!("----|------|--------|-------|-------|-------|--------------------------------|-----");
    for (i, r) in critical.iter().take(20).enumerate() {
        let spawn_str: String = r.spawn.iter()
            .enumerate()
            .filter(|(_, &v)| v > 0.0)
            .map(|(k, v)| format!("{}:{:.2}", k, v))
            .collect::<Vec<_>>()
            .join(" ");
        let keep_str: String = r.keep.iter()
            .enumerate()
            .filter(|(_, &v)| v > 0.0)
            .map(|(k, v)| format!("{}:{:.2}", k, v))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            " {:2} | {:3}  | {:.3}  | {:5.1} | {:.3} | {:.3} | {:30} | {}",
            i + 1, r.criticality_score, r.spreading_rate,
            r.complexity, r.alive_ratio, r.mean_field,
            spawn_str, keep_str,
        );
    }
}
