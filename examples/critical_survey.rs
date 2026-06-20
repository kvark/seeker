use seeker::emergence::{find_critical_rules, rule_slice_2d, rule_transect_cfg, TransectConfig};
use seeker::rules;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("all");

    let cfg = TransectConfig {
        grid_size: 64,
        sim_steps: 1000,
        num_seeds: 4,
        derrida_steps: 50,
    };
    let cfg_hires = TransectConfig {
        grid_size: 96,
        sim_steps: 2000,
        num_seeds: 8,
        derrida_steps: 80,
    };

    match mode {
        "search" | "all" => {
            println!("########## LARGE CRITICAL SURFACE SEARCH ##########");
            println!("Sampling 1000 random viable rules (8-seed averaging, 96×96 grids)...");
            let critical = find_critical_rules(1000, 42, &cfg_hires);
            println!("Found {} rules with measurable Derrida signal\n", critical.len());

            println!("Top 30 most critical rules:");
            println!("  # | Crit | Spread | Cmplx | Alive | MF    | Spawn                          | Keep");
            println!("----|------|--------|-------|-------|-------|--------------------------------|-----");
            for (i, r) in critical.iter().take(30).enumerate() {
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

            // Classify by spreading rate ranges
            let ordered = critical.iter().filter(|r| r.spreading_rate < 0.9).count();
            let critical_count = critical.iter().filter(|r| r.spreading_rate >= 0.9 && r.spreading_rate <= 1.1).count();
            let chaotic = critical.iter().filter(|r| r.spreading_rate > 1.1).count();
            println!("\nPhase distribution:");
            println!("  Ordered (λ < 0.9):     {}", ordered);
            println!("  Critical (0.9 ≤ λ ≤ 1.1): {}", critical_count);
            println!("  Chaotic (λ > 1.1):     {}", chaotic);

            // Complexity distribution for critical rules
            let crit_rules: Vec<_> = critical.iter().filter(|r| r.criticality_score >= 80).collect();
            if !crit_rules.is_empty() {
                let avg_cx: f32 = crit_rules.iter().map(|r| r.complexity).sum::<f32>() / crit_rules.len() as f32;
                let max_cx = crit_rules.iter().map(|r| r.complexity).fold(0.0f32, f32::max);
                println!("\nAmong {} highly critical rules (score ≥ 80):", crit_rules.len());
                println!("  Average complexity: {:.2}", avg_cx);
                println!("  Max complexity:     {:.2}", max_cx);
            }

            if mode == "search" { return; }
        }
        _ => {}
    }

    match mode {
        "slice" | "all" => {
            println!("\n\n########## FINE 2D SLICE: spawn[2] × spawn[3] ##########");
            let (_, gol_k) = rules::b3s23();
            let res = 21;
            let points = rule_slice_2d(&[0.0; 9], &gol_k, 2, 3, res, 42, &cfg);

            println!("\nDerrida spreading rate (21×21, GoL keep):");
            print!("        ");
            for xi in (0..res).step_by(2) {
                print!(" {:.2}", xi as f32 / (res - 1) as f32);
            }
            println!();
            for yi in 0..res {
                let y_val = yi as f32 / (res - 1) as f32;
                print!("  {:.2}  |", y_val);
                for xi in 0..res {
                    let p = &points[yi * res + xi];
                    if p.derrida.spreading_rate == 0.0 {
                        print!(" -- ");
                    } else {
                        let c = p.derrida.criticality_score();
                        if c >= 90 { print!("*"); } else { print!(" "); }
                        print!("{:.1}", p.derrida.spreading_rate);
                    }
                }
                println!();
            }

            println!("\nComplexity score (21×21):");
            print!("        ");
            for xi in (0..res).step_by(2) {
                print!(" {:.2}", xi as f32 / (res - 1) as f32);
            }
            println!();
            for yi in 0..res {
                let y_val = yi as f32 / (res - 1) as f32;
                print!("  {:.2}  |", y_val);
                for xi in 0..res {
                    let p = &points[yi * res + xi];
                    if p.alive_ratio < 0.001 {
                        print!("  -  ");
                    } else {
                        print!(" {:4.1}", p.complexity.complexity);
                    }
                }
                println!();
            }

            // Also do spawn[3] × spawn[6] (GoL → HighLife axis)
            println!("\n\n########## 2D SLICE: spawn[3] × spawn[6] (B3→B36 axis) ##########");
            let points2 = rule_slice_2d(&[0.0; 9], &gol_k, 3, 6, res, 42, &cfg);

            println!("\nDerrida spreading rate:");
            print!("s6\\s3   ");
            for xi in (0..res).step_by(2) {
                print!(" {:.2}", xi as f32 / (res - 1) as f32);
            }
            println!();
            for yi in 0..res {
                let y_val = yi as f32 / (res - 1) as f32;
                print!("  {:.2}  |", y_val);
                for xi in 0..res {
                    let p = &points2[yi * res + xi];
                    if p.derrida.spreading_rate == 0.0 {
                        print!(" -- ");
                    } else {
                        let c = p.derrida.criticality_score();
                        if c >= 90 { print!("*"); } else { print!(" "); }
                        print!("{:.1}", p.derrida.spreading_rate);
                    }
                }
                println!();
            }

            if mode == "slice" { return; }
        }
        _ => {}
    }

    match mode {
        "transect" | "all" => {
            // High-resolution transect around the GoL→HighLife boundary
            println!("\n\n########## HI-RES TRANSECT: GoL → HighLife (41 points) ##########");
            let (gol_s, gol_k) = rules::b3s23();
            let (hl_s, hl_k) = rules::b36s23();
            let points = rule_transect_cfg(&gol_s, &gol_k, &hl_s, &hl_k, 41, 42, &cfg_hires);
            println!("  t    | Spread | Crit | Complex | Entropy | AutoCorr | Alive  | MeanField");
            println!("-------|--------|------|---------|---------|----------|--------|----------");
            for p in &points {
                println!(
                    "  {:.3} | {:.3}  | {:3}  |  {:5.1} |  {:.3}  |  {:.3}   | {:.4} |  {:.3}",
                    p.t, p.derrida.spreading_rate,
                    p.derrida.criticality_score(), p.complexity.complexity,
                    p.complexity.entropy, p.complexity.autocorrelation,
                    p.alive_ratio, p.mean_field
                );
            }
        }
        _ => {}
    }
}
