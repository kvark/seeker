use crate::grid::Grid;
use crate::sim::Simulation;
use std::fs::File;
use std::path::Path;

const CELL_SIZE: u16 = 2;

fn velocity_color(cell: &crate::grid::Cell) -> u8 {
    let v = cell.avg_velocity[0] * cell.avg_velocity[0]
        + cell.avg_velocity[1] * cell.avg_velocity[1];
    if v <= 0.03 {
        1 // red
    } else if v <= 0.10 {
        2 // green
    } else {
        3 // blue
    }
}

fn render_grid(grid: &Grid) -> Vec<u8> {
    let size = grid.size();
    let w = size.x as u16 * CELL_SIZE;
    let h = size.y as u16 * CELL_SIZE;
    let mut pixels = vec![0u8; w as usize * h as usize];
    for y in 0..size.y {
        for x in 0..size.x {
            let color = match grid.get(x, y) {
                Some(cell) => velocity_color(cell),
                None => 0, // black
            };
            for dy in 0..CELL_SIZE {
                for dx in 0..CELL_SIZE {
                    let px = x as u16 * CELL_SIZE + dx;
                    let py = y as u16 * CELL_SIZE + dy;
                    pixels[py as usize * w as usize + px as usize] = color;
                }
            }
        }
    }
    pixels
}

/// Record an animated GIF of a simulation run.
/// Captures a frame every `frame_interval` steps, up to `max_frames` frames.
pub fn record_gif(
    sim: &mut Simulation,
    output_path: &Path,
    frame_interval: usize,
    max_frames: usize,
) -> Result<usize, Box<dyn std::error::Error>> {
    let size = sim.grid().size();
    let w = size.x as u16 * CELL_SIZE;
    let h = size.y as u16 * CELL_SIZE;

    let file = File::create(output_path)?;
    let mut encoder = gif::Encoder::new(file, w, h, &[
        0, 0, 0,       // 0: black (empty)
        200, 50, 50,   // 1: red (low velocity)
        50, 200, 50,   // 2: green (medium velocity)
        50, 50, 200,   // 3: blue (high velocity)
    ])?;
    encoder.set_repeat(gif::Repeat::Infinite)?;

    // First frame from initial state
    let pixels = render_grid(sim.grid());
    let mut frame = gif::Frame::default();
    frame.width = w;
    frame.height = h;
    frame.delay = 8; // 80ms per frame
    frame.buffer = std::borrow::Cow::Borrowed(&pixels);
    encoder.write_frame(&frame)?;

    let mut frames_written = 1;

    loop {
        if frames_written >= max_frames {
            break;
        }
        match sim.advance() {
            Ok(_) => {
                if sim.last_step() % frame_interval == 0 {
                    let pixels = render_grid(sim.grid());
                    let mut frame = gif::Frame::default();
                    frame.width = w;
                    frame.height = h;
                    frame.delay = 8;
                    frame.buffer = std::borrow::Cow::Borrowed(&pixels);
                    encoder.write_frame(&frame)?;
                    frames_written += 1;
                }
            }
            Err(_) => break,
        }
    }

    Ok(frames_written)
}
