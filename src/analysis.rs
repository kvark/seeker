//! Post-simulation pattern analysis for GoL.
//! Extracts connected components, classifies each as still life,
//! oscillator, or spaceship by running them in isolation.

use crate::grid::{Coordinate, Coordinates, Grid};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

/// Classification of a single connected component.
#[derive(Debug, Clone)]
pub enum PatternClass {
    StillLife { cells: usize },
    Oscillator { period: usize, cells: usize },
    Spaceship { period: usize, cells: usize },
    Extinct,
    Unknown { cells: usize },
}

impl std::fmt::Display for PatternClass {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::StillLife { cells } => write!(f, "still-life({})", cells),
            Self::Oscillator { period, cells } => write!(f, "p{}-oscillator({})", period, cells),
            Self::Spaceship { period, cells } => write!(f, "p{}-spaceship({})", period, cells),
            Self::Extinct => write!(f, "extinct"),
            Self::Unknown { cells } => write!(f, "unknown({})", cells),
        }
    }
}

/// Summary of pattern analysis for one experiment.
#[derive(Debug, Default)]
pub struct AnalysisSummary {
    pub still_lifes: usize,
    pub oscillators: Vec<usize>,  // periods
    pub spaceships: Vec<usize>,   // periods
    pub total_components: usize,
    pub unique_patterns: usize,
    pub named_patterns: Vec<&'static str>,
}

impl std::fmt::Display for AnalysisSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} components ({} unique): {} still, {} osc, {} ships",
            self.total_components,
            self.unique_patterns,
            self.still_lifes,
            self.oscillators.len(),
            self.spaceships.len(),
        )?;
        if !self.named_patterns.is_empty() {
            let mut names = self.named_patterns.clone();
            names.sort();
            names.dedup();
            write!(f, " [{}]", names.join(", "))?;
        }
        Ok(())
    }
}

/// Extract connected components from a grid (8-connected, wrapping).
pub fn connected_components(grid: &Grid) -> Vec<Vec<Coordinates>> {
    let size = grid.size();
    let total = (size.x * size.y) as usize;
    let mut visited = vec![false; total];
    let mut components = Vec::new();

    for y in 0..size.y {
        for x in 0..size.x {
            let idx = y as usize * size.x as usize + x as usize;
            if grid.get(x, y).is_some() && !visited[idx] {
                let mut comp = Vec::new();
                let mut queue = VecDeque::new();
                queue.push_back(Coordinates { x, y });
                visited[idx] = true;

                while let Some(pos) = queue.pop_front() {
                    comp.push(pos);
                    for dx in [-1i32, 0, 1] {
                        for dy in [-1i32, 0, 1] {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let raw_x = pos.x + dx;
                            let raw_y = pos.y + dy;
                            // Use the grid's boundary mode: wrapping or dead
                            if grid.get(raw_x, raw_y).is_none() {
                                continue;
                            }
                            // For indexing visited[], use wrapped coords
                            let nx = raw_x.rem_euclid(size.x);
                            let ny = raw_y.rem_euclid(size.y);
                            let nidx = ny as usize * size.x as usize + nx as usize;
                            if !visited[nidx] {
                                visited[nidx] = true;
                                queue.push_back(Coordinates { x: nx, y: ny });
                            }
                        }
                    }
                }
                components.push(comp);
            }
        }
    }
    components
}

/// One step of pure Game of Life on a grid (no RNG, deterministic B3/S23).
fn gol_step(grid: &Grid) -> Grid {
    let size = grid.size();
    let mut next = Grid::new(size);
    for y in 0..size.y {
        for x in 0..size.x {
            let mut count = 0u32;
            for dx in [-1i32, 0, 1] {
                for dy in [-1i32, 0, 1] {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    if grid.get(x + dx, y + dy).is_some() {
                        count += 1;
                    }
                }
            }
            let alive = grid.get(x, y).is_some();
            if (alive && (count == 2 || count == 3)) || (!alive && count == 3) {
                next.init(x, y);
            }
        }
    }
    next
}

/// Compute a position-independent hash of alive cells.
fn normalized_hash(grid: &Grid) -> u64 {
    let size = grid.size();
    let mut positions = Vec::new();
    for y in 0..size.y {
        for x in 0..size.x {
            if grid.get(x, y).is_some() {
                positions.push((x, y));
            }
        }
    }
    if positions.is_empty() {
        return 0;
    }
    let minx = positions.iter().map(|p| p.0).min().unwrap();
    let miny = positions.iter().map(|p| p.1).min().unwrap();

    let mut hasher = rustc_hash::FxHasher::default();
    for (x, y) in &positions {
        (x - minx, y - miny).hash(&mut hasher);
    }
    hasher.finish()
}

/// Compute center of mass of alive cells.
fn center_of_mass(grid: &Grid) -> Option<(f32, f32)> {
    let size = grid.size();
    let mut sx = 0.0f64;
    let mut sy = 0.0f64;
    let mut count = 0u32;
    for y in 0..size.y {
        for x in 0..size.x {
            if grid.get(x, y).is_some() {
                sx += x as f64;
                sy += y as f64;
                count += 1;
            }
        }
    }
    if count == 0 {
        None
    } else {
        Some((sx as f32 / count as f32, sy as f32 / count as f32))
    }
}

/// Classify a connected component by running it in isolation.
pub fn classify_component(cells: &[Coordinates]) -> PatternClass {
    if cells.is_empty() {
        return PatternClass::Extinct;
    }

    let minx = cells.iter().map(|c| c.x).min().unwrap();
    let miny = cells.iter().map(|c| c.y).min().unwrap();
    let maxx = cells.iter().map(|c| c.x).max().unwrap();
    let maxy = cells.iter().map(|c| c.y).max().unwrap();

    let w = maxx - minx + 1;
    let h = maxy - miny + 1;
    // Pad enough for the pattern to oscillate + move for several periods
    let pad = 30.max(w).max(h);
    let grid_w = (w + 2 * pad) as Coordinate;
    let grid_h = (h + 2 * pad) as Coordinate;

    let mut grid = Grid::new(Coordinates {
        x: grid_w,
        y: grid_h,
    });
    for cell in cells {
        grid.init(cell.x - minx + pad, cell.y - miny + pad);
    }

    let initial_hash = normalized_hash(&grid);
    let initial_com = center_of_mass(&grid).unwrap();

    // Run up to 60 steps looking for the pattern to return
    for step in 1..=60 {
        grid = gol_step(&grid);

        if grid.alive_count() == 0 {
            return PatternClass::Extinct;
        }

        let hash = normalized_hash(&grid);
        if hash == initial_hash {
            let com = center_of_mass(&grid).unwrap();
            let dx = (com.0 - initial_com.0).abs();
            let dy = (com.1 - initial_com.1).abs();
            if dx < 0.5 && dy < 0.5 {
                if step == 1 {
                    return PatternClass::StillLife { cells: cells.len() };
                } else {
                    return PatternClass::Oscillator {
                        period: step,
                        cells: cells.len(),
                    };
                }
            } else {
                return PatternClass::Spaceship {
                    period: step,
                    cells: cells.len(),
                };
            }
        }
    }

    PatternClass::Unknown { cells: cells.len() }
}

/// Return the name of a known GoL pattern given a canonical hash.
fn name_pattern(_hash: u64, class: &PatternClass) -> Option<&'static str> {
    // We identify by (class, cell_count, hash) to avoid computing complex canonical forms.
    // Instead, we use the classification + cell count.
    match class {
        PatternClass::StillLife { cells: 4 } => Some("block"),
        PatternClass::StillLife { cells: 5 } => Some("boat/tub"),
        PatternClass::StillLife { cells: 6 } => Some("beehive/ship"),
        PatternClass::StillLife { cells: 7 } => Some("loaf/long-boat"),
        PatternClass::StillLife { cells: 8 } => Some("pond/long-ship"),
        PatternClass::Oscillator { period: 2, cells: 3 } => Some("blinker"),
        PatternClass::Oscillator { period: 2, cells: 6 } => Some("toad/beacon"),
        PatternClass::Oscillator { period: 3, cells: 12 } => Some("pulsar"),
        PatternClass::Oscillator { period: 15, cells: 12 } => Some("pentadecathlon"),
        PatternClass::Spaceship { period: 4, cells: 5 } => Some("glider"),
        PatternClass::Spaceship { period: 4, cells: 9 } => Some("LWSS"),
        PatternClass::Spaceship { period: 4, cells: 13 } => Some("MWSS"),
        PatternClass::Spaceship { period: 4, cells: 17 } => Some("HWSS"),
        _ => None,
    }
}

/// Analyze a grid: extract components and classify each.
pub fn analyze_grid(grid: &Grid) -> (Vec<PatternClass>, AnalysisSummary) {
    let components = connected_components(grid);
    let mut patterns = Vec::with_capacity(components.len());
    let mut summary = AnalysisSummary::default();
    let mut pattern_hashes = std::collections::HashSet::new();

    summary.total_components = components.len();

    for comp in &components {
        // Compute normalized hash for uniqueness tracking
        let minx = comp.iter().map(|c| c.x).min().unwrap();
        let miny = comp.iter().map(|c| c.y).min().unwrap();
        let mut hasher = rustc_hash::FxHasher::default();
        for c in comp {
            (c.x - minx, c.y - miny).hash(&mut hasher);
        }
        let hash = hasher.finish();
        pattern_hashes.insert(hash);

        let class = classify_component(comp);
        if let Some(name) = name_pattern(hash, &class) {
            summary.named_patterns.push(name);
        }
        match &class {
            PatternClass::StillLife { .. } => summary.still_lifes += 1,
            PatternClass::Oscillator { period, .. } => summary.oscillators.push(*period),
            PatternClass::Spaceship { period, .. } => summary.spaceships.push(*period),
            _ => {}
        }
        patterns.push(class);
    }
    summary.unique_patterns = pattern_hashes.len();

    (patterns, summary)
}
