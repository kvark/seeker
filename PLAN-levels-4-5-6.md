# Implementation Plan: Interestingness Levels 4, 5, 6

## Architecture Overview

Levels 4 and 5 require **in-simulation tracking** — they need to observe the
trajectory, not just the final state. Level 6 enhances the existing post-hoc
analysis in `analysis.rs`.

The key constraint: `advance()` runs in a tight loop on Choir worker threads.
Any per-step work must be cheap. Expensive work (connected components) must be
sampled at intervals.

```
sim.rs::advance()          analysis.rs (post-hoc)     lab.rs (fitness)
  │                             │                         │
  ├─ spatial_entropy (L5)       │                         │
  │  (cheap, every step)        │                         │
  │                             │                         │
  ├─ component snapshot (L4)    │                         │
  │  (expensive, every N steps) │                         │
  │                             │                         │
  └─ Conclusion::Done ─────────┼─── analyze_grid ────────┤
                                │    + composability (L6)  │
                                │                         │
                                └─────────────────────────┘
```

---

## Step 1: Spatial Entropy (Level 5) — in `sim.rs`

**Why first**: cheapest to compute, no new dependencies, immediate fitness signal.

### 1a. Add spatial entropy to `GridAnalysis`

In `grid.rs`, add to `GridAnalysis`:
```rust
pub struct GridAnalysis {
    pub alive_ratio: f32,
    pub birth_rate: f32,
    /// Variance of alive density across grid quadrants (2x2 regions).
    pub spatial_entropy: f32,
}
```

In `analyze_with_births()`, divide the grid into a 4x4 macro-grid (16 regions).
For each region, compute `alive_count / region_cell_count`. Then compute the
variance of these 16 ratios. This is O(cells) — same cost as the existing
alive count, just with a branch per cell to bucket it.

### 1b. Track spatial entropy in `Statistics`

Add to `Statistics`:
```rust
pub spatial_entropy_average: f32,
pub spatial_entropy_variance: f32,
```

Update with the same exponential weighting used for alive_ratio and birth_rate.

### 1c. Wire into fitness

In `lab.rs`, add a `spatial_score`:
- High spatial entropy average = structured, non-uniform dynamics = interesting
- `(spatial_entropy_average * K).min(20.0) as usize`

**Files**: `grid.rs`, `sim.rs`, `lab.rs`, `main.rs` (display column)

---

## Step 2: Narrative Event Tracking (Level 4) — in `sim.rs`

**Why second**: this is the most impactful but most architecturally involved change.

### 2a. Lightweight component snapshot

Create a new module `src/narrative.rs` with a compact component representation:

```rust
/// Lightweight snapshot of the component structure at a point in time.
pub struct ComponentSnapshot {
    /// For each component: (normalized_hash, center_of_mass, cell_count)
    components: Vec<(u64, (f32, f32), usize)>,
}
```

The snapshot uses the existing `connected_components()` from `analysis.rs` +
`normalized_hash()` (needs to be made `pub`). This is the expensive part — so
it's sampled every `NARRATIVE_SAMPLE_INTERVAL` steps (e.g., 64 or 128).

### 2b. Event detection by diffing snapshots

```rust
pub enum NarrativeEvent {
    Split,    // one hash → two (or hash gone + two new hashes)
    Merge,    // two hashes → one
    Birth,    // new hash appears, not near any previous component
    Death,    // hash disappears, not accounted for by merge
}
```

Diffing algorithm:
1. Match components between snapshots by normalized_hash
2. Unmatched old components → potential Deaths or Merges
3. Unmatched new components → potential Births or Splits
4. Use center-of-mass proximity to disambiguate splits from births

This doesn't need to be perfect. Even a coarse event count is a powerful signal.

### 2c. Accumulate in Statistics

```rust
pub struct NarrativeStats {
    pub total_events: usize,
    pub splits: usize,
    pub merges: usize,
    pub births: usize,
    pub deaths: usize,
}
```

Add `narrative: NarrativeStats` to `Statistics`.

### 2d. Wire into fitness

Narrative richness score:
- `event_count.min(100)` — raw count, capped
- Bonus for event diversity (multiple event types seen)
- Methuselahs naturally score high here

### 2e. Performance budget

`connected_components()` on a 128×128 grid: ~16K cells, BFS traversal.
Sampled every 128 steps over 8192 max_steps = ~64 snapshots per experiment.
With 5 active experiments, that's ~320 component analyses per full run.
Each takes ~microseconds. Total overhead: negligible.

**Files**: new `src/narrative.rs`, `sim.rs`, `lab.rs`, `main.rs`, `lib.rs`

---

## Step 3: Composability Score (Level 6) — in `analysis.rs`

**Why last**: builds on existing infrastructure, enhances post-hoc analysis.

### 3a. Classified cell coverage

In `AnalysisSummary`, add:
```rust
/// Fraction of alive cells that belong to classified (non-Unknown) patterns.
pub classified_ratio: f32,
/// Number of distinct recognized pattern types present.
pub distinct_classified_types: usize,
```

In `analyze_grid()`, after classifying all components, sum up cells in
StillLife + Oscillator + Spaceship vs total alive cells. A grid where 95% of
cells are in recognized patterns is more "composable" than one where 10% are.

### 3b. Component independence check

For each pair of classified components, check if they're far enough apart to
be non-interacting. A "functional composition" is one where components work
independently — their behavior in isolation matches their behavior in context.

Simple proxy: minimum distance between any two components. If all classified
components are separated by at least `2 * max_component_diameter`, they're
likely independent.

```rust
/// True if all classified components are well-separated (non-interacting).
pub components_independent: bool,
```

### 3c. Wire into fitness

Composability score:
- `classified_ratio * 30` — strong reward for grids made of known patterns
- Bonus if `components_independent` — patterns that compose without interfering
- Bonus for `distinct_classified_types > 1` — diverse composition

**Files**: `analysis.rs`, `lab.rs`, `main.rs`

---

## Implementation Order & Dependencies

```
Step 1 (Spatial Entropy)     — standalone, no new modules
    │
Step 2 (Narrative)           — new module, depends on analysis.rs being pub
    │
Step 3 (Composability)       — extends analysis.rs, no new modules
```

Steps 1 and 3 are independent and could be done in parallel.
Step 2 depends on `normalized_hash` and `connected_components` being pub.

## Estimated Complexity

| Step | New lines | Files touched | Risk |
|------|-----------|---------------|------|
| 1 - Spatial entropy | ~40 | 4 (grid, sim, lab, main) | Low — same pattern as birth_rate |
| 2 - Narrative | ~200 | 5 (new narrative.rs, sim, lab, main, lib) | Medium — component diffing logic |
| 3 - Composability | ~60 | 3 (analysis, lab, main) | Low — extends existing analysis |

## Open Questions

1. **Sampling interval for narrative**: 64 steps? 128? Should it adapt based
   on how dynamic the simulation is?
2. **Event diffing precision**: fuzzy matching by hash+proximity, or exact?
   Exact is simpler but misses transformed components.
3. **Spatial entropy grid size**: 4×4 (16 regions) seems right for 64×64 to
   256×256 grids. Should it scale with grid size?
4. **Composability independence threshold**: what separation distance counts
   as "non-interacting"? Depends on max spaceship speed (c/4 for GoL gliders).
