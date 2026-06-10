//! Calibration tests for the interestingness detector.
//!
//! These tests validate the detector against well-known Game of Life phenomena
//! to establish baseline measurements before exploring unknown rule spaces.
//!
//! - R-pentomino: a 5-cell methuselah that stabilizes after ~1103 steps,
//!   producing a rich narrative of splits, merges, births, and deaths.
//! - Gosper glider gun: a period-30 gun that emits gliders, producing high
//!   spatial variance (localized activity) and periodic birth events.

use rustc_hash::FxHashMap;
use seeker::grid::BoundaryMode;
use seeker::sim::{Data, HumanRules, Snap};

/// Build a Snap for standard B3/S23 Game of Life with a specific pattern
/// placed on a grid of the given size with Dead boundary.
///
/// `alive_cells` are (x, y) positions of alive cells (absolute coordinates).
fn gol_snap(width: i32, height: i32, max_steps: usize, alive_cells: &[(i32, i32)]) -> Snap {
    // Build the hex-encoded grid lines.
    // Each hex character encodes 4 consecutive cells (bit 0 = leftmost).
    assert!(width % 4 == 0, "Grid width must be a multiple of 4");
    let chars_per_row = width as usize / 4;
    let mut grid_data: Vec<Vec<u8>> = vec![vec![0u8; chars_per_row]; height as usize];

    for &(x, y) in alive_cells {
        assert!(x >= 0 && x < width && y >= 0 && y < height, "Cell ({x}, {y}) out of bounds");
        let char_index = x as usize / 4;
        let bit_index = x as usize % 4;
        grid_data[y as usize][char_index] |= 1 << bit_index;
    }

    let lines: Vec<String> = grid_data
        .iter()
        .map(|row| {
            row.iter()
                .map(|&nibble| {
                    if nibble < 10 {
                        (b'0' + nibble) as char
                    } else {
                        (b'a' + nibble - 10) as char
                    }
                })
                .collect()
        })
        .collect();

    let mut spawn = FxHashMap::default();
    spawn.insert(3, 1.0);
    let mut keep = FxHashMap::default();
    keep.insert(2, 1.0);
    keep.insert(3, 1.0);

    Snap {
        data: Data::Grid(lines),
        rules: HumanRules {
            kernel: vec!["111".into(), "1X1".into(), "111".into()],
            spawn,
            keep,
        },
        random_seed: 0,
        limits: seeker::sim::Limits {
            max_steps,
            update_weight: 0.01,
        },
        boundary: BoundaryMode::Dead,
    }
}

/// R-pentomino pattern (5 cells):
/// ```text
/// .##
/// ##.
/// .#.
/// ```
/// Returns (x, y) positions centered on the given origin.
fn r_pentomino(cx: i32, cy: i32) -> Vec<(i32, i32)> {
    vec![
        (cx + 1, cy),
        (cx + 2, cy),
        (cx, cy + 1),
        (cx + 1, cy + 1),
        (cx + 1, cy + 2),
    ]
}

/// Gosper glider gun (36 cells, fits in ~38x11 area).
///
/// ```text
/// ........................O...........
/// ......................O.O...........
/// ............OO......OO............OO
/// ...........O...O....OO............OO
/// OO........O.....O...OO..............
/// OO........O...O.OO....O.O..........
/// ..........O.....O.......O...........
/// ...........O...O....................
/// ............OO......................
/// ```
/// Returns (x, y) positions offset by (ox, oy).
fn gosper_glider_gun(ox: i32, oy: i32) -> Vec<(i32, i32)> {
    let pattern = [
        "........................O...........",
        "......................O.O...........",
        "............OO......OO............OO",
        "...........O...O....OO............OO",
        "OO........O.....O...OO..............",
        "OO........O...O.OO....O.O..........",
        "..........O.....O.......O...........",
        "...........O...O....................",
        "............OO......................",
    ];
    let mut cells = Vec::new();
    for (y, row) in pattern.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if ch == 'O' {
                cells.push((ox + x as i32, oy + y as i32));
            }
        }
    }
    cells
}

/// Run a simulation for the given number of steps (or until it concludes).
/// Returns the final statistics and the step count reached.
fn run_simulation(
    snap: &Snap,
    target_steps: usize,
) -> (seeker::sim::Statistics, usize) {
    let mut sim = seeker::sim::Simulation::new(snap).expect("Failed to create simulation");
    for _ in 0..target_steps {
        match sim.advance() {
            Ok(_) => {}
            Err(seeker::sim::Conclusion::Done(stats, _)) => {
                return (stats, sim.last_step());
            }
            Err(seeker::sim::Conclusion::Extinct) => {
                panic!("Simulation went extinct unexpectedly");
            }
            Err(seeker::sim::Conclusion::Saturate) => {
                panic!("Simulation saturated unexpectedly");
            }
            Err(seeker::sim::Conclusion::Crash) => {
                panic!("Simulation crashed");
            }
        }
    }
    (*sim.stats(), sim.last_step())
}

// ---------------------------------------------------------------------------
// Test 1: R-pentomino narrative calibration
// ---------------------------------------------------------------------------

#[test]
fn r_pentomino_narrative() {
    let grid_size = 256;
    let center = grid_size / 2;
    let cells = r_pentomino(center, center);
    let snap = gol_snap(grid_size, grid_size, 2000, &cells);

    let (stats, steps) = run_simulation(&snap, 1200);

    println!("R-pentomino after {steps} steps:");
    println!("  narrative.total_events = {}", stats.narrative.total_events);
    println!("  narrative.splits       = {}", stats.narrative.splits);
    println!("  narrative.merges       = {}", stats.narrative.merges);
    println!("  narrative.births       = {}", stats.narrative.births);
    println!("  narrative.deaths       = {}", stats.narrative.deaths);
    println!("  narrative.event_diversity = {}", stats.narrative.event_diversity());
    println!("  narrative.richness     = {}", stats.narrative.richness());

    // The R-pentomino is a famous methuselah — it should produce a rich narrative
    // with many structural events over its ~1103-step transient.
    assert!(
        stats.narrative.total_events > 20,
        "R-pentomino should produce many narrative events, got {}",
        stats.narrative.total_events
    );
    assert!(
        stats.narrative.event_diversity() >= 3,
        "R-pentomino should exhibit at least 3 event types (splits, births, deaths), got {}",
        stats.narrative.event_diversity()
    );
    assert!(
        stats.narrative.richness() > 50,
        "R-pentomino narrative richness should be significant, got {}",
        stats.narrative.richness()
    );
    assert!(
        stats.narrative.splits > 0,
        "R-pentomino should have split events (debris separating)"
    );
    assert!(
        stats.narrative.births > 0,
        "R-pentomino should have birth events (gliders emitted)"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Gosper glider gun emergence calibration
// ---------------------------------------------------------------------------

#[test]
fn gosper_gun_emergence() {
    let grid_size = 256;
    // Place gun with room for gliders to travel (gun is ~36 wide, ~9 tall).
    // Put it in upper-left quadrant so gliders travel into open space.
    let cells = gosper_glider_gun(20, 20);
    let snap = gol_snap(grid_size, grid_size, 2000, &cells);

    let (stats, steps) = run_simulation(&snap, 500);

    println!("Gosper gun after {steps} steps:");
    println!("  spatial_variance_avg   = {}", stats.spatial_variance_average);
    println!("  narrative.total_events = {}", stats.narrative.total_events);
    println!("  narrative.splits       = {}", stats.narrative.splits);
    println!("  narrative.births       = {}", stats.narrative.births);
    println!("  narrative.deaths       = {}", stats.narrative.deaths);
    println!("  narrative.richness     = {}", stats.narrative.richness());
    println!("  alive_ratio_avg        = {}", stats.alive_ratio_average);
    println!("  birth_rate_avg         = {}", stats.birth_rate_average);

    // The gun produces localized activity — spatial variance should be non-trivial.
    assert!(
        stats.spatial_variance_average > 0.0,
        "Gosper gun should produce non-zero spatial variance (localized activity), got {}",
        stats.spatial_variance_average
    );

    // The gun emits a glider every 30 steps. Over 500 steps that's ~16 gliders.
    // The narrative tracker should detect births (new components appearing).
    assert!(
        stats.narrative.births > 0,
        "Gosper gun should produce birth events (glider emissions)"
    );

    // Overall narrative should show some activity.
    assert!(
        stats.narrative.total_events > 5,
        "Gosper gun should produce narrative events from glider emission, got {}",
        stats.narrative.total_events
    );
}
