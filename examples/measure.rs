//! F1 measurement harness in action: run a Flow-Lenia world and report the
//! intrinsic metrics — field concentration, blob count/size, activity, and
//! center-of-mass velocity — as a time series plus a run-summary behavior
//! fingerprint (the axes an F2 outer-loop search would illuminate).
//!
//! Usage:
//!   cargo run --release --example measure [steps] [seed-mode]
//!   seed-mode: blobs (default) | soup

use rand::SeedableRng;
use seeker::flow_lenia::{FlowLeniaParams, World};
use seeker::harness::{connected_components, field_stats, measure_run, Tracker};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let steps: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let seed_mode = args.get(2).map(|s| s.as_str()).unwrap_or("blobs");

    let (w, h) = (128usize, 128usize);
    let threshold = 0.05f32;
    let mut world = World::new(w, h, FlowLeniaParams::default());
    match seed_mode {
        "soup" => {
            let mut rng = rand::rngs::StdRng::seed_from_u64(1234);
            world.seed_random_patch(&mut rng, 0, 64.0, 64.0, 90.0, 0.6);
        }
        _ => {
            world.seed_blob(0, 64.0, 64.0, 9.0, 0.95);
            world.seed_blob(0, 40.0, 80.0, 5.0, 0.8);
            world.seed_blob(0, 90.0, 40.0, 4.0, 0.7);
        }
    }

    println!("F1 harness  |  {w}×{h}, {steps} steps, seed={seed_mode}, threshold={threshold}");
    println!("\n  step | conc  | blobs | mean-sz | activity |  speed | maxspd | occ");
    println!("  -----|-------|-------|---------|----------|--------|--------|------");

    // Manual loop mirroring measure_run so we can print a live time series.
    let mut tracker = Tracker::new(w, h, 8.0);
    let mut prev: Option<Vec<f32>> = None;
    let report_every = (steps / 15).max(1);
    for step in 0..=steps {
        if step > 0 {
            world.step();
        }
        let show = step % report_every == 0 || step == steps;
        if !show {
            continue;
        }
        let field = world.mass_field();
        let stats = field_stats(&field, threshold);
        let comps = connected_components(&field, w, h, threshold);
        let vel = tracker.observe(&comps);
        let act = prev.as_ref().map(|p| seeker::harness::activity(p, &field)).unwrap_or(0.0);
        prev = Some(field);
        println!(
            "  {:4} | {:.3} | {:5} | {:7.1} | {:8.5} | {:6.3} | {:6.3} | {:.3}",
            step,
            stats.concentration,
            comps.count(),
            comps.mean_size(),
            act,
            vel.mean_speed,
            vel.max_speed,
            stats.occupied_fraction,
        );
    }

    // Reset and fold the same run into a summary fingerprint.
    let mut world2 = World::new(w, h, FlowLeniaParams::default());
    match seed_mode {
        "soup" => {
            let mut rng = rand::rngs::StdRng::seed_from_u64(1234);
            world2.seed_random_patch(&mut rng, 0, 64.0, 64.0, 90.0, 0.6);
        }
        _ => {
            world2.seed_blob(0, 64.0, 64.0, 9.0, 0.95);
            world2.seed_blob(0, 40.0, 80.0, 5.0, 0.8);
            world2.seed_blob(0, 90.0, 40.0, 4.0, 0.7);
        }
    }
    let (summary, _series) = measure_run(&mut world2, steps, report_every, threshold, 8.0);

    println!("\n=== run summary (behavior fingerprint) ===");
    println!("  mass drift         : {:.2e}  (conservation check)", summary.mass_drift);
    println!("  mean concentration : {:.3}   (matter organized into structure)", summary.mean_concentration);
    println!("  mean components    : {:.2}", summary.mean_components);
    println!("  final components   : {}", summary.final_components);
    println!("  mean activity      : {:.5}  (dynamism)", summary.mean_activity);
    println!("  final activity     : {:.5}  (settled vs churning)", summary.final_activity);
    println!("  mean speed         : {:.3}   (motility)", summary.mean_speed);
    println!("  peak speed         : {:.3}", summary.peak_speed);
}
