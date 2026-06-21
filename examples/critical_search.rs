use seeker::emergence::{
    binary_search_critical, cma_evolve_critical, gradient_descent_to_critical,
    trace_critical_manifold, TransectConfig,
};
use seeker::rules;

fn print_rule(label: &str, spawn: &[f32; 9], keep: &[f32; 9]) {
    let spawn_str: String = spawn
        .iter()
        .enumerate()
        .filter(|(_, &v)| v > 0.0)
        .map(|(k, v)| format!("{}:{:.2}", k, v))
        .collect::<Vec<_>>()
        .join(" ");
    let keep_str: String = keep
        .iter()
        .enumerate()
        .filter(|(_, &v)| v > 0.0)
        .map(|(k, v)| format!("{}:{:.2}", k, v))
        .collect::<Vec<_>>()
        .join(" ");
    println!("  {}: spawn=[{}] keep=[{}]", label, spawn_str, keep_str);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("all");

    let cfg = TransectConfig {
        grid_size: 64,
        sim_steps: 1000,
        num_seeds: 4,
        derrida_steps: 50,
    };

    match mode {
        "gradient" | "all" => {
            println!("########## GRADIENT DESCENT TO CRITICAL ##########\n");

            // Start from a slightly chaotic rule (HighLife-ish)
            let (hl_s, hl_k) = rules::b36s23();
            println!("Starting from HighLife (B36/S23):");
            print_rule("start", &hl_s, &hl_k);

            let trajectory = gradient_descent_to_critical(&hl_s, &hl_k, 15, 42, &cfg);
            println!("\n  Step | Spread | Cmplx | Alive");
            println!("  -----|--------|-------|------");
            for (i, r) in trajectory.iter().enumerate() {
                println!(
                    "  {:3}  | {:.3}  | {:5.1} | {:.3}",
                    i, r.spreading_rate, r.complexity, r.alive_ratio
                );
            }
            if let Some(last) = trajectory.last() {
                println!("\nFinal rule:");
                print_rule("result", &last.spawn, &last.keep);
            }

            // Start from an ordered rule
            println!("\n\nStarting from a sparse ordered rule:");
            let ordered_spawn = [0.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0];
            let ordered_keep = [0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0];
            print_rule("start", &ordered_spawn, &ordered_keep);

            let trajectory2 = gradient_descent_to_critical(&ordered_spawn, &ordered_keep, 15, 99, &cfg);
            println!("\n  Step | Spread | Cmplx | Alive");
            println!("  -----|--------|-------|------");
            for (i, r) in trajectory2.iter().enumerate() {
                println!(
                    "  {:3}  | {:.3}  | {:5.1} | {:.3}",
                    i, r.spreading_rate, r.complexity, r.alive_ratio
                );
            }
            if let Some(last) = trajectory2.last() {
                println!("\nFinal rule:");
                print_rule("result", &last.spawn, &last.keep);
            }

            if mode == "gradient" {
                return;
            }
        }
        _ => {}
    }

    match mode {
        "binary" | "all" => {
            println!("\n\n########## BINARY SEARCH ON TRANSECTS ##########\n");

            // Find exact critical point between GoL (ordered-ish) and a chaotic rule
            let (gol_s, gol_k) = rules::b3s23();
            let chaotic_spawn = [0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0];
            let chaotic_keep = [0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0];

            println!("Bisecting between GoL and B2345/S2345:");
            let result = binary_search_critical(
                &gol_s, &gol_k, &chaotic_spawn, &chaotic_keep, 0.02, 12, 42, &cfg,
            );
            println!(
                "  Found: λ={:.4}, complexity={:.1}, alive={:.3}",
                result.spreading_rate, result.complexity, result.alive_ratio
            );
            print_rule("critical", &result.spawn, &result.keep);

            // Between zero-spawn and full-spawn (with GoL keep)
            println!("\nBisecting between empty spawn and full spawn (GoL keep):");
            let empty_spawn = [0.0; 9];
            let full_spawn = [1.0; 9];
            let result2 = binary_search_critical(
                &empty_spawn, &gol_k, &full_spawn, &gol_k, 0.02, 12, 77, &cfg,
            );
            println!(
                "  Found: λ={:.4}, complexity={:.1}, alive={:.3}",
                result2.spreading_rate, result2.complexity, result2.alive_ratio
            );
            print_rule("critical", &result2.spawn, &result2.keep);

            // Between GoL and HighLife
            println!("\nBisecting between GoL and HighLife:");
            let (hl_s, hl_k) = rules::b36s23();
            let result3 = binary_search_critical(
                &gol_s, &gol_k, &hl_s, &hl_k, 0.02, 12, 55, &cfg,
            );
            println!(
                "  Found: λ={:.4}, complexity={:.1}, alive={:.3}",
                result3.spreading_rate, result3.complexity, result3.alive_ratio
            );
            print_rule("critical", &result3.spawn, &result3.keep);

            if mode == "binary" {
                return;
            }
        }
        _ => {}
    }

    match mode {
        "evolve" | "all" => {
            println!("\n\n########## CMA-ES EVOLUTION: CRITICALITY × COMPLEXITY ##########\n");
            println!("Population 16, 10 generations:");

            let results = cma_evolve_critical(16, 10, 42, &cfg);
            println!("\n  Gen | Spread | Cmplx | Alive");
            println!("  ----|--------|-------|------");
            for (i, r) in results.iter().enumerate() {
                println!(
                    "  {:3} | {:.3}  | {:5.1} | {:.3}",
                    i, r.spreading_rate, r.complexity, r.alive_ratio
                );
            }
            if let Some(best) = results.last() {
                println!("\nBest evolved rule:");
                print_rule("result", &best.spawn, &best.keep);
            }

            if mode == "evolve" {
                return;
            }
        }
        _ => {}
    }

    match mode {
        "trace" | "all" => {
            println!("\n\n########## CRITICAL MANIFOLD TRACING ##########\n");

            // Start from a known critical rule (from our survey, rule #26)
            let start_spawn = [0.05, 0.0, 0.0, 0.45, 0.0, 0.63, 0.0, 0.0, 0.0];
            let start_keep = [0.0, 0.84, 0.67, 0.92, 0.0, 0.0, 0.0, 0.0, 0.63];
            println!("Starting from critical rule (λ≈0.96, survey #26):");
            print_rule("start", &start_spawn, &start_keep);

            let trajectory = trace_critical_manifold(&start_spawn, &start_keep, 8, 0.05, 42, &cfg);
            println!("\n  Step | Spread | Cmplx | Alive");
            println!("  -----|--------|-------|------");
            for (i, r) in trajectory.iter().enumerate() {
                println!(
                    "  {:3}  | {:.3}  | {:5.1} | {:.3}",
                    i, r.spreading_rate, r.complexity, r.alive_ratio
                );
            }
            if let Some(last) = trajectory.last() {
                println!("\nFinal rule on manifold:");
                print_rule("result", &last.spawn, &last.keep);
                let first = &trajectory[0];
                let delta_cx = last.complexity - first.complexity;
                println!(
                    "\nComplexity change: {:.1} → {:.1} (Δ={:+.1})",
                    first.complexity, last.complexity, delta_cx
                );
            }

            if mode == "trace" {
                return;
            }
        }
        _ => {}
    }
}
