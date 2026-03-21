use seeker::{analysis, lab, render, sim};

#[cfg(feature = "tui")]
mod tui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, fs::File, path::PathBuf};

    let args: Vec<String> = std::env::args().collect();
    let command = if args.len() < 2 {
        println!("Usage:");
        #[cfg(feature = "tui")]
        {
            println!("  seeker play [<path_to_snap>]");
            println!("  seeker find [<path_to_init_snap>]");
        }
        println!("  seeker headless [<path_to_init_snap>] [<duration_secs>] [<config_path>]");
        println!("  seeker replay <path_to_snap> <output.gif>");
        return Ok(());
    } else {
        args[1].clone()
    };

    let snap_name = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "data/default-snap.ron".to_string());
    let mut snap_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    snap_path.push(&snap_name);
    let init_snap: sim::Snap =
        ron::de::from_reader(File::open(&snap_path).unwrap()).unwrap();

    match command.as_str() {
        #[cfg(feature = "tui")]
        "play" => {
            tui::run_play(init_snap)?;
        }
        #[cfg(feature = "tui")]
        "find" => {
            let mut config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            config_path.push("data");
            config_path.push("config.ron");
            let config = ron::de::from_reader(File::open(config_path).unwrap()).unwrap();
            tui::run_find(init_snap, config)?;
        }
        "headless" => {
            let duration_secs: u64 = args
                .get(3)
                .and_then(|s| s.parse().ok())
                .unwrap_or(120);
            let config_name = args
                .get(4)
                .cloned()
                .unwrap_or_else(|| "data/config.ron".to_string());
            let mut config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            config_path.push(&config_name);
            let config = ron::de::from_reader(File::open(config_path).unwrap()).unwrap();
            let mut lab = lab::Laboratory::new(config, "data/active/");
            lab.add_experiment(init_snap, 0);
            eprintln!("Running headless search for {}s...", duration_secs);
            let start = std::time::Instant::now();
            let deadline = std::time::Duration::from_secs(duration_secs);
            while start.elapsed() < deadline {
                lab.update();
                std::thread::sleep(std::time::Duration::from_millis(10));
                let experiments = lab.experiments();
                let concluded = experiments.iter().filter(|e| e.conclusion.is_some()).count();
                let active = experiments.len() - concluded;
                let max_fit = experiments.iter().map(|e| e.fit).max().unwrap_or(0);
                eprint!(
                    "\r[{} total, {} active, {} concluded, {} discarded] best fit: {}    ",
                    experiments.len(),
                    active,
                    concluded,
                    lab.early_discards,
                    max_fit
                );
            }
            eprintln!();

            // Collect interesting concluded experiments (survivors only)
            let mut survivors: Vec<_> = lab
                .experiments()
                .iter()
                .filter(|e| {
                    matches!(
                        e.conclusion,
                        Some(sim::Conclusion::Done(..))
                    )
                })
                .collect();
            survivors.sort_by(|a, b| b.fit.cmp(&a.fit));

            // Print structured summary
            let total = lab.experiments().len();
            let extinct_count = lab
                .experiments()
                .iter()
                .filter(|e| matches!(e.conclusion, Some(sim::Conclusion::Extinct)))
                .count();
            let saturate_count = lab
                .experiments()
                .iter()
                .filter(|e| matches!(e.conclusion, Some(sim::Conclusion::Saturate)))
                .count();

            println!("## Search Results");
            println!();
            println!("- Duration: {}s", duration_secs);
            println!("- Total experiments: {}", total);
            println!("- Survivors: {}", survivors.len());
            println!("- Extinct: {}", extinct_count);
            println!("- Saturated: {}", saturate_count);
            println!();

            if !survivors.is_empty() {
                println!("### Top Survivors");
                println!();
                println!("| ID | Fitness | Alive Avg | Alive Var | Birth Rate | Spatial Var | Period | Ships | MaxOsc | Steps | Snap |");
                println!("|----|---------|-----------|-----------|------------|-------------|--------|-------|--------|-------|------|");
                let top_n = survivors.len().min(10);
                for exp in &survivors[..top_n] {
                    if let Some(sim::Conclusion::Done(stats, _)) = &exp.conclusion {
                        let snap_file = format!("e{}-{}.ron", exp.id, exp.steps);
                        println!(
                            "| {} | {} | {:.4} | {:.6} | {:.6} | {:.6} | {} | {} | {} | {} | {} |",
                            exp.id, exp.fit, stats.alive_ratio_average,
                            stats.alive_ratio_variance, stats.birth_rate_average,
                            stats.spatial_variance_average,
                            stats.period, stats.transient_spaceships,
                            stats.max_oscillator_period,
                            exp.steps, snap_file
                        );
                    }
                }
                println!();

                // Pattern analysis for top survivors
                println!("### Pattern Analysis");
                println!();
                let analyze_count = top_n.min(10);
                for exp in &survivors[..analyze_count] {
                    if let Some(sim::Conclusion::Done(..)) = &exp.conclusion {
                        // Re-run simulation to get the stabilized grid
                        if let Ok(mut sim) = sim::Simulation::new(exp.snap()) {
                            loop {
                                match sim.advance() {
                                    Ok(_) => {}
                                    Err(_) => break,
                                }
                            }
                            let (_patterns, summary) = analysis::analyze_grid(sim.grid());
                            print!("- E[{}]: {}", exp.id, summary);
                            // Highlight interesting finds
                            if !summary.spaceships.is_empty() {
                                print!(" **SPACESHIP FOUND!**");
                            }
                            let high_period: Vec<_> = summary
                                .oscillators
                                .iter()
                                .filter(|&&p| p > 2)
                                .collect();
                            if !high_period.is_empty() {
                                print!(
                                    " **HIGH-PERIOD OSC: {:?}**",
                                    high_period
                                );
                            }
                            println!();
                        }
                    }
                }
                println!();

                // Record GIFs for interesting survivors
                let gif_count = top_n.min(5);
                let gif_dir = PathBuf::from("data/active/gifs");
                fs::create_dir_all(&gif_dir).unwrap();
                println!("### Recorded GIFs");
                println!();
                for exp in &survivors[..gif_count] {
                    let gif_path = gif_dir.join(format!("e{}.gif", exp.id));
                    eprint!("Recording GIF for E[{}]...", exp.id);
                    let mut sim = sim::Simulation::new(exp.snap()).unwrap();
                    match render::record_gif(&mut sim, &gif_path, 4, 200) {
                        Ok(frames) => {
                            eprintln!(" {} frames", frames);
                            println!("- `gifs/e{}.gif` ({} frames)", exp.id, frames);
                        }
                        Err(e) => {
                            eprintln!(" error: {}", e);
                        }
                    }
                }
            }
        }
        "replay" => {
            let output_path = args
                .get(3)
                .map(|s| PathBuf::from(s))
                .unwrap_or_else(|| PathBuf::from("replay.gif"));
            let mut sim = sim::Simulation::new(&init_snap).unwrap();
            eprintln!(
                "Replaying {} -> {}",
                snap_path.display(),
                output_path.display()
            );
            match render::record_gif(&mut sim, &output_path, 4, 200) {
                Ok(frames) => {
                    eprintln!("Wrote {} frames to {}", frames, output_path.display());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            println!("Unknown command: '{}'", command);
        }
    }

    Ok(())
}
