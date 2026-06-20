use seeker::emergence::rule_transect;
use seeker::rules;

fn run_transect(
    name: &str,
    spawn_a: &[f32; 9],
    keep_a: &[f32; 9],
    spawn_b: &[f32; 9],
    keep_b: &[f32; 9],
) {
    println!("\n=== {name} ===");
    let points = rule_transect(spawn_a, keep_a, spawn_b, keep_b, 21, 64, 1000, 42);
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

fn main() {
    let (gol_s, gol_k) = rules::b3s23();
    let (hl_s, hl_k) = rules::b36s23();
    let (seeds_s, seeds_k) = rules::b2s();
    let (dn_s, dn_k) = rules::b3678s34678();

    run_transect("GoL (B3/S23) → HighLife (B36/S23)", &gol_s, &gol_k, &hl_s, &hl_k);
    run_transect("GoL (B3/S23) → Seeds (B2/S)", &gol_s, &gol_k, &seeds_s, &seeds_k);
    run_transect("GoL (B3/S23) → Day & Night (B3678/S34678)", &gol_s, &gol_k, &dn_s, &dn_k);
    run_transect("HighLife (B36/S23) → Day & Night (B3678/S34678)", &hl_s, &hl_k, &dn_s, &dn_k);
}
