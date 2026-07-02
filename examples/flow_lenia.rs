//! M-γ-0 demonstration: run the Flow-Lenia CPU substrate headless, report mass
//! conservation and center-of-mass drift (velocity is now a plain observable —
//! no movement-detection black box), and export an animated GIF.
//!
//! Usage:
//!   cargo run --release --example flow_lenia [steps] [out.gif] [seed-mode]
//!
//! seed-mode is `blobs` (default) or `soup` (full-grid noise → self-organizing
//! spots).

use rand::SeedableRng;
use seeker::flow_lenia::{FlowLeniaParams, World};
use std::borrow::Cow;
use std::fs::File;

const CELL: u16 = 3; // upscale factor for the GIF

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let steps: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("data/flow-lenia.gif");
    let seed_mode = args.get(3).map(|s| s.as_str()).unwrap_or("blobs");

    let (w, h) = (128usize, 128usize);
    let params = FlowLeniaParams::default();
    let mut world = World::new(w, h, params);

    // With mass fixed, the dynamics can only redistribute what we seed here.
    match seed_mode {
        "soup" => {
            // Full-grid noise: watch matter flow into spaced, persistent spots.
            let mut rng = rand::rngs::StdRng::seed_from_u64(1234);
            world.seed_random_patch(&mut rng, 0, 64.0, 64.0, 90.0, 0.6);
        }
        _ => {
            // A dense central blob plus an off-center companion.
            world.seed_blob(0, 64.0, 64.0, 9.0, 0.95);
            world.seed_blob(0, 40.0, 80.0, 5.0, 0.8);
        }
    }

    let initial_mass = world.total_mass();
    println!("Flow-Lenia M-γ-0  |  {w}×{h}, {steps} steps");
    println!("initial mass = {initial_mass:.4}");
    println!(
        "\n  step | mass drift | occupied | variance | center (x,y)  | speed"
    );
    println!("  -----|------------|----------|----------|---------------|------");

    let palette = build_palette();
    let gw = w as u16 * CELL;
    let gh = h as u16 * CELL;
    let file = File::create(out).expect("create gif");
    let mut encoder = gif::Encoder::new(file, gw, gh, &palette).expect("gif encoder");
    encoder.set_repeat(gif::Repeat::Infinite).ok();

    let frame_every = (steps / 150).max(1);
    let mut prev_center = world.center_of_mass();

    write_frame(&mut encoder, &world, gw, gh);
    report(0, &world, initial_mass, prev_center, &mut prev_center);

    for step in 1..=steps {
        world.step();
        if step % frame_every == 0 {
            write_frame(&mut encoder, &world, gw, gh);
        }
        if step % (steps / 12).max(1) == 0 || step == steps {
            let prev = prev_center;
            report(step, &world, initial_mass, prev, &mut prev_center);
        }
    }

    let final_mass = world.total_mass();
    let drift = (final_mass - initial_mass).abs() / initial_mass;
    println!(
        "\nfinal mass = {final_mass:.4}  |  relative drift = {drift:.2e}  |  gif → {out}"
    );
    if drift < 1e-3 {
        println!("mass conservation: PASS (< 1e-3)");
    } else {
        println!("mass conservation: FAIL — investigate transport");
    }
}

fn report(
    step: usize,
    world: &World,
    initial_mass: f64,
    prev: Option<(f32, f32)>,
    prev_slot: &mut Option<(f32, f32)>,
) {
    let mass = world.total_mass();
    let drift = (mass - initial_mass).abs() / initial_mass;
    let occ = world.occupied_fraction(0.05);
    let var = world.mass_variance();
    let center = world.center_of_mass();
    let speed = match (prev, center) {
        (Some((px, py)), Some((cx, cy))) => {
            // Toroidal nearest displacement, per reported interval.
            let dx = wrap_delta(cx - px, world.width() as f32);
            let dy = wrap_delta(cy - py, world.height() as f32);
            (dx * dx + dy * dy).sqrt()
        }
        _ => 0.0,
    };
    let (cx, cy) = center.unwrap_or((f32::NAN, f32::NAN));
    println!(
        "  {step:4} | {drift:10.2e} | {occ:8.3} | {var:8.4} | ({cx:5.1},{cy:5.1}) | {speed:5.2}"
    );
    *prev_slot = center;
}

fn wrap_delta(mut d: f32, size: f32) -> f32 {
    if d > size * 0.5 {
        d -= size;
    } else if d < -size * 0.5 {
        d += size;
    }
    d
}

fn write_frame(encoder: &mut gif::Encoder<File>, world: &World, gw: u16, gh: u16) {
    let (w, h) = (world.width(), world.height());
    let field = world.channel(0);
    let mut pixels = vec![0u8; gw as usize * gh as usize];
    for y in 0..h {
        for x in 0..w {
            let v = field[y * w + x].clamp(0.0, 1.0);
            let idx = (v * 255.0) as u8;
            for dy in 0..CELL {
                for dx in 0..CELL {
                    let px = x as u16 * CELL + dx;
                    let py = y as u16 * CELL + dy;
                    pixels[py as usize * gw as usize + px as usize] = idx;
                }
            }
        }
    }
    let frame = gif::Frame {
        width: gw,
        height: gh,
        delay: 6,
        buffer: Cow::Owned(pixels),
        ..Default::default()
    };
    encoder.write_frame(&frame).ok();
}

/// A 256-entry "inferno"-style heatmap: concentration 0 → black, 1 → pale yellow.
fn build_palette() -> Vec<u8> {
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
