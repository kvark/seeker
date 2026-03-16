//! Narrative event tracking for Level 4 interestingness.
//!
//! Periodically snapshots connected components during simulation and diffs
//! consecutive snapshots to detect structural events (splits, merges, births,
//! deaths). The event count and diversity form a "narrative richness" signal.

use crate::analysis::connected_components;
use crate::grid::Grid;

/// How often (in simulation steps) to take a component snapshot.
pub const SAMPLE_INTERVAL: usize = 16;

/// A lightweight snapshot of component structure at a point in time.
#[derive(Clone)]
struct Component {
    /// Center of mass (x, y).
    center: (f32, f32),
    /// Number of alive cells.
    cell_count: usize,
}

/// Accumulated narrative statistics.
#[derive(Copy, Clone, Debug, Default)]
pub struct NarrativeStats {
    pub total_events: usize,
    pub splits: usize,
    pub merges: usize,
    pub births: usize,
    pub deaths: usize,
}

impl NarrativeStats {
    /// Number of distinct event types observed (0-4).
    pub fn event_diversity(&self) -> usize {
        (self.splits > 0) as usize
            + (self.merges > 0) as usize
            + (self.births > 0) as usize
            + (self.deaths > 0) as usize
    }

    /// Narrative richness score: event count scaled by diversity.
    pub fn richness(&self) -> usize {
        self.total_events * self.event_diversity().max(1)
    }
}

/// Tracks component structure over time and detects events.
pub struct NarrativeTracker {
    prev_snapshot: Vec<Component>,
    pub stats: NarrativeStats,
}

impl NarrativeTracker {
    pub fn new() -> Self {
        Self {
            prev_snapshot: Vec::new(),
            stats: NarrativeStats::default(),
        }
    }

    /// Take a snapshot and diff against the previous one.
    /// Call this every `SAMPLE_INTERVAL` steps.
    pub fn sample(&mut self, grid: &Grid) {
        let components = connected_components(grid);
        let snapshot: Vec<Component> = components
            .iter()
            .map(|comp| {
                let (sx, sy): (f32, f32) = comp.iter().fold((0.0, 0.0), |(sx, sy), c| {
                    (sx + c.x as f32, sy + c.y as f32)
                });
                let n = comp.len() as f32;
                Component {
                    center: (sx / n, sy / n),
                    cell_count: comp.len(),
                }
            })
            .collect();

        if !self.prev_snapshot.is_empty() {
            self.diff_snapshots(&snapshot, grid.size());
        }

        self.prev_snapshot = snapshot;
    }

    /// Diff two snapshots using greedy nearest-neighbor matching.
    fn diff_snapshots(
        &mut self,
        current: &[Component],
        grid_size: crate::grid::Coordinates,
    ) {
        let mut old_matched = vec![false; self.prev_snapshot.len()];
        let mut new_matched = vec![false; current.len()];

        // Cost function: weighted centroid distance + size difference.
        let grid_diag = ((grid_size.x as f32).powi(2) + (grid_size.y as f32).powi(2)).sqrt();
        let gating_threshold = 0.7;

        // Greedy nearest-neighbor: for each old component, find best new match.
        for (oi, old) in self.prev_snapshot.iter().enumerate() {
            let mut best_cost = f32::MAX;
            let mut best_ni = None;

            for (ni, new) in current.iter().enumerate() {
                if new_matched[ni] {
                    continue;
                }
                let dx = old.center.0 - new.center.0;
                let dy = old.center.1 - new.center.1;
                let dist = (dx * dx + dy * dy).sqrt() / grid_diag;
                let size_max = old.cell_count.max(new.cell_count) as f32;
                let size_diff = (old.cell_count as f32 - new.cell_count as f32).abs()
                    / size_max.max(1.0);
                let cost = 0.6 * dist + 0.4 * size_diff;

                if cost < best_cost {
                    best_cost = cost;
                    best_ni = Some(ni);
                }
            }

            if let Some(ni) = best_ni {
                if best_cost < gating_threshold {
                    old_matched[oi] = true;
                    new_matched[ni] = true;
                }
            }
        }

        // Unmatched old components: potential deaths or merge sources.
        let unmatched_old: usize = old_matched.iter().filter(|&&m| !m).count();
        // Unmatched new components: potential births or split products.
        let unmatched_new: usize = new_matched.iter().filter(|&&m| !m).count();

        // Heuristic event classification:
        // - If old count > new count and unmatched_old > unmatched_new → merges + deaths
        // - If new count > old count and unmatched_new > unmatched_old → splits + births
        let old_total = self.prev_snapshot.len();
        let new_total = current.len();

        if unmatched_old > 0 || unmatched_new > 0 {
            if new_total > old_total {
                // Net gain in components → likely splits or births
                // Births: new components far from any old component
                // Splits: new components near an old component that also has a match
                // Simplified: attribute unmatched_new as splits if there are unmatched_old nearby,
                // otherwise births.
                let split_count = unmatched_new.min(unmatched_old);
                let birth_count = unmatched_new.saturating_sub(split_count);
                let death_count = unmatched_old.saturating_sub(split_count);
                self.stats.splits += split_count;
                self.stats.births += birth_count;
                self.stats.deaths += death_count;
                self.stats.total_events += split_count + birth_count + death_count;
            } else if new_total < old_total {
                // Net loss → likely merges or deaths
                let merge_count = unmatched_old.min(unmatched_new);
                let death_count = unmatched_old.saturating_sub(merge_count);
                let birth_count = unmatched_new.saturating_sub(merge_count);
                self.stats.merges += merge_count;
                self.stats.deaths += death_count;
                self.stats.births += birth_count;
                self.stats.total_events += merge_count + death_count + birth_count;
            } else {
                // Same count but different components — likely replacements
                // Attribute as deaths + births
                self.stats.deaths += unmatched_old;
                self.stats.births += unmatched_new;
                self.stats.total_events += unmatched_old + unmatched_new;
            }
        }
    }
}
