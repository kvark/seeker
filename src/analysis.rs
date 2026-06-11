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
    /// Fraction of alive cells belonging to classified (non-Unknown) patterns.
    pub classified_ratio: f32,
    /// Number of distinct recognized pattern types (still life, each osc period, each ship period).
    pub distinct_classified_types: usize,
    /// True if all classified components are well-separated (non-interacting).
    pub components_independent: bool,
}

impl AnalysisSummary {
    /// Composability score: how well the grid decomposes into independent classified patterns.
    pub fn composability_score(&self) -> usize {
        // Classified coverage: 0-30 points
        let coverage = (self.classified_ratio * 30.0) as usize;
        // Independence bonus: 10 points if all components are well-separated
        let independence = if self.components_independent { 10 } else { 0 };
        // Type diversity bonus: up to 10 points
        let diversity = self.distinct_classified_types.min(3) * 3;
        coverage + independence + diversity
    }
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

/// Extract connected components from a grid (8-connected).
/// Uses unwrapped coordinates so components crossing wrap boundaries
/// remain contiguous (e.g. a glider at x=127→0 gets coords 127,128,129
/// instead of 127,0,1).
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
                            if grid.get(raw_x, raw_y).is_none() {
                                continue;
                            }
                            let nx = raw_x.rem_euclid(size.x);
                            let ny = raw_y.rem_euclid(size.y);
                            let nidx = ny as usize * size.x as usize + nx as usize;
                            if !visited[nidx] {
                                visited[nidx] = true;
                                // Store unwrapped coords to keep contiguity
                                queue.push_back(Coordinates { x: raw_x, y: raw_y });
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

/// Compute a position-independent hash of a component given as a slice of coordinates.
/// The hash depends only on the relative arrangement of cells, not absolute position.
pub fn component_shape_hash(cells: &[Coordinates]) -> u64 {
    if cells.is_empty() {
        return 0;
    }
    let minx = cells.iter().map(|c| c.x).min().unwrap();
    let miny = cells.iter().map(|c| c.y).min().unwrap();

    let mut sorted: Vec<_> = cells.iter().map(|c| (c.x - minx, c.y - miny)).collect();
    sorted.sort();

    let mut hasher = rustc_hash::FxHasher::default();
    for pos in &sorted {
        pos.hash(&mut hasher);
    }
    hasher.finish()
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
    let mut min_cells = cells.len();

    // Run up to 180 steps looking for the pattern to return.
    // Needs to exceed common periods: pentadecathlon (15), pulsar (3),
    // plus margin for complex oscillators.
    for step in 1..=180 {
        grid = gol_step(&grid);

        let alive = grid.alive_count();
        if alive == 0 {
            return PatternClass::Extinct;
        }
        min_cells = min_cells.min(alive);

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
                        cells: min_cells,
                    };
                }
            } else {
                return PatternClass::Spaceship {
                    period: step,
                    cells: min_cells,
                };
            }
        }
    }

    PatternClass::Unknown { cells: cells.len() }
}

/// Return the name of a known GoL pattern given a canonical hash.
pub fn name_pattern(_hash: u64, class: &PatternClass) -> Option<&'static str> {
    // We identify by (class, cell_count, hash) to avoid computing complex canonical forms.
    // Instead, we use the classification + cell count.
    match class {
        PatternClass::StillLife { cells: 4 } => Some("block"),
        PatternClass::StillLife { cells: 5 } => Some("boat/tub"),
        PatternClass::StillLife { cells: 6 } => Some("beehive/ship"),
        PatternClass::StillLife { cells: 7 } => Some("loaf/long-boat"),
        PatternClass::StillLife { cells: 8 } => Some("pond/long-ship"),
        PatternClass::StillLife { cells: 9 } => Some("hat/shillelagh"),
        PatternClass::StillLife { cells: 10 } => Some("10-cell-sl"),
        PatternClass::Oscillator { period: 2, cells: 3 } => Some("blinker"),
        PatternClass::Oscillator { period: 2, cells: 6 } => Some("toad/beacon"),
        PatternClass::Oscillator { period: 2, cells: 8 } => Some("beacon"),
        PatternClass::Oscillator { period: 3, cells: 48 } => Some("pulsar"),
        PatternClass::Oscillator { period: 15, cells: 12 } => Some("pentadecathlon"),
        PatternClass::Spaceship { period: 4, cells: 5 } => Some("glider"),
        PatternClass::Spaceship { period: 4, cells: 9 } => Some("LWSS"),
        PatternClass::Spaceship { period: 4, cells: 11 } => Some("MWSS"),
        PatternClass::Spaceship { period: 4, cells: 13 } => Some("HWSS"),
        _ => None,
    }
}

/// Compute the bounding box of a component as (min_x, min_y, max_x, max_y).
fn bounding_box(comp: &[Coordinates]) -> (Coordinate, Coordinate, Coordinate, Coordinate) {
    let minx = comp.iter().map(|c| c.x).min().unwrap();
    let miny = comp.iter().map(|c| c.y).min().unwrap();
    let maxx = comp.iter().map(|c| c.x).max().unwrap();
    let maxy = comp.iter().map(|c| c.y).max().unwrap();
    (minx, miny, maxx, maxy)
}

/// Chebyshev distance between two bounding boxes.
fn bbox_chebyshev_distance(
    a: (Coordinate, Coordinate, Coordinate, Coordinate),
    b: (Coordinate, Coordinate, Coordinate, Coordinate),
) -> Coordinate {
    let dx = if a.2 < b.0 {
        b.0 - a.2
    } else if b.2 < a.0 {
        a.0 - b.2
    } else {
        0
    };
    let dy = if a.3 < b.1 {
        b.1 - a.3
    } else if b.3 < a.1 {
        a.1 - b.3
    } else {
        0
    };
    dx.max(dy)
}

/// Merge connected components into constellations.
///
/// Components whose bounding boxes have Chebyshev distance <= 2 are merged.
/// Distance 2 means a birth in the 1-cell gap between components is possible
/// (a cell can have neighbors in both components). This allows multi-component
/// patterns like pulsars (48 cells, period 3) to be classified as a single
/// unit — matching how apgsearch handles "pseudo-objects".
///
/// Returns a list of merged cell groups (constellations).
fn merge_constellations(components: &[Vec<Coordinates>]) -> Vec<Vec<Coordinates>> {
    let n = components.len();
    if n == 0 {
        return Vec::new();
    }

    // Compute bounding boxes
    let bboxes: Vec<_> = components.iter().map(|c| bounding_box(c)).collect();

    // Union-Find
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank = vec![0u8; n];

    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]]; // path compression
            i = parent[i];
        }
        i
    }

    fn union(parent: &mut [usize], rank: &mut [u8], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra == rb {
            return;
        }
        if rank[ra] < rank[rb] {
            parent[ra] = rb;
        } else if rank[ra] > rank[rb] {
            parent[rb] = ra;
        } else {
            parent[rb] = ra;
            rank[ra] += 1;
        }
    }

    // Merge components with Chebyshev distance < 2 between bounding boxes
    for i in 0..n {
        for j in (i + 1)..n {
            if bbox_chebyshev_distance(bboxes[i], bboxes[j]) <= 2 {
                union(&mut parent, &mut rank, i, j);
            }
        }
    }

    // Group components by their root
    let mut groups: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    // Build merged cell lists, preserving a stable ordering (by smallest component index)
    let mut roots: Vec<usize> = groups.keys().copied().collect();
    roots.sort();

    let mut merged = Vec::with_capacity(roots.len());
    for root in roots {
        let indices = &groups[&root];
        let mut cells = Vec::new();
        for &idx in indices {
            cells.extend_from_slice(&components[idx]);
        }
        merged.push(cells);
    }
    merged
}

/// Analyze a grid: extract components, merge nearby ones into constellations,
/// and classify each constellation.
pub fn analyze_grid(grid: &Grid) -> (Vec<PatternClass>, AnalysisSummary, Vec<Vec<Coordinates>>) {
    let raw_components = connected_components(grid);

    // Merge nearby components into constellations for classification.
    let components = merge_constellations(&raw_components);

    let mut patterns = Vec::with_capacity(components.len());
    let mut summary = AnalysisSummary::default();
    let mut pattern_hashes = std::collections::HashSet::new();
    let mut classified_types = std::collections::HashSet::new();

    summary.total_components = components.len();

    let mut total_alive = 0usize;
    let mut classified_alive = 0usize;
    let mut classified_bboxes = Vec::new();

    for comp in &components {
        let comp_cells = comp.len();
        total_alive += comp_cells;

        // Compute normalized hash for uniqueness tracking
        let minx = comp.iter().map(|c| c.x).min().unwrap();
        let miny = comp.iter().map(|c| c.y).min().unwrap();
        let mut hasher = rustc_hash::FxHasher::default();
        let mut sorted_positions: Vec<_> = comp.iter().map(|c| (c.x - minx, c.y - miny)).collect();
        sorted_positions.sort();
        for pos in &sorted_positions {
            pos.hash(&mut hasher);
        }
        let hash = hasher.finish();
        pattern_hashes.insert(hash);

        let class = classify_component(comp);
        if let Some(name) = name_pattern(hash, &class) {
            summary.named_patterns.push(name);
        }

        let is_classified = match &class {
            PatternClass::StillLife { .. } => {
                summary.still_lifes += 1;
                classified_types.insert("still_life");
                true
            }
            PatternClass::Oscillator { period, .. } => {
                summary.oscillators.push(*period);
                classified_types.insert("oscillator");
                true
            }
            PatternClass::Spaceship { period, .. } => {
                summary.spaceships.push(*period);
                classified_types.insert("spaceship");
                true
            }
            _ => false,
        };

        if is_classified {
            classified_alive += comp_cells;
            classified_bboxes.push(bounding_box(comp));
        }
        patterns.push(class);
    }
    summary.unique_patterns = pattern_hashes.len();
    summary.classified_ratio = if total_alive > 0 {
        classified_alive as f32 / total_alive as f32
    } else {
        0.0
    };
    summary.distinct_classified_types = classified_types.len();

    // Check independence: all classified components separated by >= 2 cells
    // (Chebyshev distance >= 2, which is the constellation criterion for Moore neighborhood).
    // Note: merged constellations are already single entries, so internal components
    // (which were merged because they were close) don't affect this check.
    summary.components_independent = if classified_bboxes.len() > 1 {
        let mut independent = true;
        'outer: for i in 0..classified_bboxes.len() {
            for j in (i + 1)..classified_bboxes.len() {
                if bbox_chebyshev_distance(classified_bboxes[i], classified_bboxes[j]) <= 2 {
                    independent = false;
                    break 'outer;
                }
            }
        }
        independent
    } else {
        true
    };

    (patterns, summary, components)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Grid;

    #[test]
    fn glider_classified_as_spaceship() {
        // .O.
        // ..O
        // OOO
        let mut grid = Grid::new(Coordinates { x: 32, y: 32 });
        grid.init(16, 15);
        grid.init(17, 16);
        grid.init(15, 17);
        grid.init(16, 17);
        grid.init(17, 17);

        let (patterns, summary, _) = analyze_grid(&grid);
        assert_eq!(patterns.len(), 1);
        assert!(matches!(patterns[0], PatternClass::Spaceship { period: 4, cells: 5 }));
        assert_eq!(summary.spaceships, vec![4]);
        assert!(summary.named_patterns.contains(&"glider"));
    }

    #[test]
    fn pulsar_classified_as_oscillator() {
        // Pulsar: period-3, 48 cells, symmetric. Place it centered on a grid.
        let mut grid = Grid::new(Coordinates { x: 32, y: 32 });
        let cx = 16i32;
        let cy = 16i32;
        // A pulsar has 4 symmetric arms. The canonical form:
        // Relative coordinates (one quadrant, then mirror 4 ways)
        let quadrant = [
            (1, 2), (1, 3), (1, 4),
            (2, 1), (3, 1), (4, 1),
            (2, 6), (3, 6), (4, 6),
            (6, 2), (6, 3), (6, 4),
        ];
        for &(dx, dy) in &quadrant {
            grid.init(cx + dx, cy + dy);
            grid.init(cx - dx, cy + dy);
            grid.init(cx + dx, cy - dy);
            grid.init(cx - dx, cy - dy);
        }

        let (patterns, summary, _) = analyze_grid(&grid);
        // Should be one merged constellation classified as oscillator
        let osc_count: usize = patterns.iter().filter(|p| matches!(p, PatternClass::Oscillator { .. })).count();
        assert!(osc_count >= 1, "Expected pulsar to be classified as oscillator, got {:?}", patterns);
        assert!(summary.oscillators.contains(&3), "Expected period 3, got {:?}", summary.oscillators);
    }
}
