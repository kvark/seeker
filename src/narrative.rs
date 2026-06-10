//! Narrative event tracking for Level 4 interestingness.
//!
//! Periodically snapshots connected components during simulation and diffs
//! consecutive snapshots to detect structural events (splits, merges, births,
//! deaths). The event count and diversity form a "narrative richness" signal.
//!
//! Component identity across snapshots is determined by **cell overlap**: two
//! components in consecutive snapshots are considered the same entity if they
//! share at least one cell position. This is far more accurate than centroid
//! distance for detecting splits, merges, births, and deaths.

use crate::analysis::connected_components;
use crate::grid::Grid;

/// How often (in simulation steps) to take a component snapshot.
pub const SAMPLE_INTERVAL: usize = 16;

/// A snapshot of a single connected component, identified by its cell positions.
#[derive(Clone)]
struct Component {
    /// Sorted cell positions (x, y) for efficient overlap checks via binary search.
    cells: Vec<(i32, i32)>,
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
            .into_iter()
            .map(|comp| {
                let mut cells: Vec<(i32, i32)> =
                    comp.iter().map(|c| (c.x, c.y)).collect();
                cells.sort_unstable();
                Component { cells }
            })
            .collect();

        if !self.prev_snapshot.is_empty() {
            self.diff_snapshots(&snapshot);
        }

        self.prev_snapshot = snapshot;
    }

    /// Check whether two components share at least one cell position.
    /// Both `a` and `b` must be sorted.
    fn overlaps(a: &[(i32, i32)], b: &[(i32, i32)]) -> bool {
        // Merge-join on two sorted slices — O(|a| + |b|).
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }

    /// Diff two snapshots using cell-overlap identity.
    ///
    /// Builds a bipartite overlap graph between old and new components, then
    /// classifies events:
    /// - Old → 0 new: death
    /// - Old → 1 new (and that new ← 1 old): continuation
    /// - Old → 2+ new: split
    /// - New ← 0 old: birth
    /// - New ← 2+ old: merge
    fn diff_snapshots(&mut self, current: &[Component]) {
        let old_len = self.prev_snapshot.len();
        let new_len = current.len();

        // For each old component, which new components does it overlap with?
        let mut old_to_new: Vec<Vec<usize>> = vec![Vec::new(); old_len];
        // For each new component, which old components does it overlap with?
        let mut new_to_old: Vec<Vec<usize>> = vec![Vec::new(); new_len];

        for (oi, old) in self.prev_snapshot.iter().enumerate() {
            for (ni, new) in current.iter().enumerate() {
                if Self::overlaps(&old.cells, &new.cells) {
                    old_to_new[oi].push(ni);
                    new_to_old[ni].push(oi);
                }
            }
        }

        // Classify events from the overlap graph.

        // Splits: an old component overlaps with 2+ new components.
        for neighbors in &old_to_new {
            if neighbors.len() >= 2 {
                // One split event produces N-1 additional components.
                self.stats.splits += 1;
                self.stats.total_events += 1;
            }
        }

        // Merges: a new component overlaps with 2+ old components.
        for neighbors in &new_to_old {
            if neighbors.len() >= 2 {
                // One merge event consumes N-1 components.
                self.stats.merges += 1;
                self.stats.total_events += 1;
            }
        }

        // Deaths: old component overlaps with 0 new components.
        for neighbors in &old_to_new {
            if neighbors.is_empty() {
                self.stats.deaths += 1;
                self.stats.total_events += 1;
            }
        }

        // Births: new component overlaps with 0 old components.
        for neighbors in &new_to_old {
            if neighbors.is_empty() {
                self.stats.births += 1;
                self.stats.total_events += 1;
            }
        }
    }
}
