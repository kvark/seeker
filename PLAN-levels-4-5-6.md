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

## Open Questions — Resolved via Literature Review

### Q1: Sampling interval for narrative

**Answer: Fixed 16-step base interval + cheap activity trigger.**

The literature strongly favors a hybrid approach over pure adaptive sampling:

- **apgsearch** doesn't track mid-simulation at all — it runs to completion and
  analyzes the ash. But we need trajectory data, so we must sample.
- **McCaskill & Packard (2019)** track components every generation, which is
  feasible for their small grids but expensive for ours.
- **BMC Bioinformatics (2004)** showed that per-cell activity indicators with
  skip counters achieve 4-5x speedup in CA simulation with negligible accuracy
  loss — but this optimizes the simulation, not the analysis.
- **Sensor network literature** consistently shows 50-80% sample reduction
  with threshold-based adaptive schemes, but the overhead of the adaptive logic
  itself can neutralize gains when the per-sample cost is already low.

**Recommendation**: Sample every 16 steps (cheap, predictable). Additionally,
if `|delta_alive_count|` between consecutive `advance()` calls exceeds a
threshold (e.g., >5% of total cells), insert an extra snapshot — this catches
sudden mass-extinction or explosion events at near-zero cost. Once period is
detected, stop sampling entirely (the HashLife insight: temporal regularity
means there's nothing new to learn).

16 steps over 8192 max_steps = 512 snapshots. Connected components on a 128×128
grid is ~16K cells BFS, takes microseconds. Total overhead: <1% of simulation.

### Q2: Event diffing precision

**Answer: Greedy nearest-neighbor with overlap+centroid+size cost function.**

The MOT (Multiple Object Tracking) literature converges on a layered approach:

- **Hungarian algorithm** (O(n³)) gives optimal assignment but is overkill for
  our scale (<50 components). At n<50, greedy nearest-neighbor produces
  identical results in the vast majority of cases.
- **Jonker-Volgenant** is 10x faster than Hungarian for n>200, irrelevant here.
- **ByteTrack (ECCV 2022)** uses a two-pass matching cascade with gating
  thresholds — first match high-confidence pairs, then handle remainders.
- **NIST Overlap-Based Cell Tracker** uses a weighted cost function:
  `cost = w1 * overlap + w2 * centroid_distance + w3 * size_change`

**Recommendation**: Use greedy nearest-neighbor matching with a 3-term cost:

```
cost(old, new) = w1 * (1 - overlap_ratio)
               + w2 * centroid_distance / grid_diagonal
               + w3 * |size_old - size_new| / max(size_old, size_new)
```

With a **gating threshold**: if `cost > 0.7`, don't match (treat as
death+birth rather than a transformed component). Unmatched old → Death or
merge contributor. Unmatched new → Birth or split product.

For splits/merges specifically: if one old component overlaps with two new
components (both below gating threshold), that's a Split. If two old
components overlap with one new component, that's a Merge. This handles the
common cases without needing exact hash matching.

Hash matching is still useful as a **fast path**: if `normalized_hash(old) ==
normalized_hash(new)` and centroids are close, skip the cost calculation.

### Q3: Spatial entropy grid size

**Answer: 4×4 macro-grid (16 regions), with compression ratio as a second signal.**

The literature reveals multiple approaches:

- **Israeli & Goldenfeld**: Use block sizes 2-3 cells (absolute) for 1D ECA.
  Block size is chosen relative to the rule neighborhood, not grid size.
- **Javaheri Javid (EPIA 2015)**: No subdivision at all — uses conditional
  entropy H(X|Y) between each cell and its neighbors. Sensitive to spatial
  arrangement, unlike plain Shannon entropy.
- **Practical consensus**: 8-16 cells per region gives meaningful per-region
  statistics. For a 64×64 grid → 4×4 to 8×8 regions. For 128×128 → 8×8 to
  16×16 regions.

**Recommendation**: Use a 4×4 macro-grid (16 regions). This gives:
- 64×64 grid: 16×16 cells per region (256 cells) — good statistics
- 128×128 grid: 32×32 cells per region (1024 cells) — excellent statistics
- 32×32 grid: 8×8 cells per region (64 cells) — marginal but usable

Don't scale with grid size — fixed 4×4 is simpler and the per-region cell
counts are already adequate for all our grid sizes (32-256).

**Bonus measure**: Compression ratio as a second complexity proxy. Serialize
the grid row-major, compress with a fast algorithm, use
`compressed_size / raw_size` as a Kolmogorov complexity proxy. This is:
- Nearly free (sub-millisecond for our grid sizes)
- Spatially sensitive (unlike Shannon entropy of global density)
- Well-validated: used alongside block entropy in CA complexity research
  (arXiv:1304.2816)
- Complementary to regional variance: compression catches fine-grained
  structure that coarse regions miss

### Q4: Composability independence threshold

**Answer: Chebyshev distance ≥ 2 × kernel_radius between bounding boxes.**

The GoL community has a precise hierarchy:

| Category | Separation | Meaning |
|---|---|---|
| **Constellation** | Chebyshev ≥ 2 | Fully non-interacting; shared dead-cell neighborhoods impossible |
| **Quasi still life** | Chebyshev = 1 (diagonal) | Shared dead cells, but independently stable |
| **Pseudo still life** | Chebyshev = 1 (specific overlap) | Combined neighborhood matters |
| **Strict still life** | N/A | Cannot decompose |

For standard GoL (Moore neighborhood, radius 1), the speed of light is
c = 1 cell/generation. Two static patterns with Chebyshev gap ≥ 2 can never
interact.

**For Seeker's probabilistic CA**: The kernel defines the effective
neighborhood. The maximum kernel extent (max Chebyshev distance of any kernel
entry from center) is the effective radius `r`. Two patterns separated by
Chebyshev distance > `2r` cannot interact in a single step, and if both are
stable/oscillating within their bounding boxes, they're permanently
independent.

**Recommendation**: Compute `kernel_radius = max(|offset.x|, |offset.y|)` over
all kernel entries. Two components are independent if the Chebyshev distance
between their bounding boxes is ≥ `2 * kernel_radius`. This is the
"constellation" criterion generalized to arbitrary kernels.

For dynamic patterns (spaceships), true independence requires light-cone
analysis over the remaining simulation time: gap ≥ `kernel_radius *
remaining_steps`. But this is too conservative — in practice, if patterns
haven't interacted in 100+ steps and are separated by ≥ `4 * kernel_radius`,
call them independent.

**apgsearch's approach**: Uses a custom "ContagiousLife" infection-propagation
rule to separate pseudo still lifes. This is elegant but GoL-specific. For
Seeker's probabilistic rules, the bounding-box Chebyshev distance is simpler
and sufficient.

---

## Prior Art & Key References

### Component Tracking (Level 4)

- **McCaskill & Packard (2019)** — "Analysing Emergent Dynamics of Evolving
  Computation in 2D Cellular Automata," TPNC 2019. Connected component
  labelling + 64-bit quadtree hashing + temporal tracking. The most directly
  relevant paper for our narrative tracking.
- **NIST Overlap-Based Cell Tracker (Chalfoun et al., 2016)** — Overlap +
  centroid + size cost function for tracking biological cells between frames.
  Directly transferable to CA component tracking.
- **ByteTrack (ECCV 2022)** — Two-pass matching cascade with gating. State of
  the art in multi-object tracking.
- **apgsearch v5 (Adam P. Goucher)** — Runs soups to completion, classifies
  ash via apgcode canonical hashing. No mid-simulation tracking, but the
  separation and classification pipeline is the gold standard.

### Spatial Entropy & Emergence (Level 5)

- **Crutchfield & Young (1989)** — Statistical complexity C_μ via
  epsilon-machines. The theoretical ideal: C_μ measures structure while entropy
  rate h measures randomness. "Interesting" systems have high C_μ and moderate
  h. Limited to 1D in practice.
- **Shalizi (2001 thesis)** — Local statistical complexity as a spatial filter
  for detecting coherent structures. Rare causal states (collisions, particles)
  have high local complexity. Beautiful but computationally expensive for 2D.
- **Lizier et al. (2007-2012)** — Information dynamics decomposition: Active
  Information Storage (identifies oscillators/still lifes), Transfer Entropy
  (identifies gliders as "information rivers"), information modification
  (identifies collision/computation sites). JIDT toolkit available.
- **Israeli & Goldenfeld (2006)** — Coarse-graining CA: if a simpler rule
  governs coarse dynamics, the system has emergent simplification. Class 4
  rules resist coarse-graining — that's what makes them interesting.
- **Javaheri Javid (EPIA 2015)** — Mean information gain H(X|Y) across
  neighbor pairs. Spatially sensitive, unlike plain Shannon entropy.
- **Compression as Kolmogorov proxy** — Serialize grid, compress, use ratio.
  Validated in arXiv:1304.2816 for CA complexity measurement.

### Composability (Level 6)

- **LifeWiki constellation/quasi/pseudo still life hierarchy** — Formal
  definitions of component independence based on Chebyshev distance and
  shared-neighborhood analysis.
- **apgsearch pseudo_bangbang** — Infection-propagation algorithm for
  separating pseudo still lifes. GoL-specific but conceptually elegant.
- **Catagolue census** — Top 16 GoL ash objects account for >99.9% of all
  occurrences. Classified cell coverage is implicitly the standard metric.
- **ASAL (Sakana AI, 2024)** — Uses CLIP embeddings for open-ended CA
  novelty. Not composability per se, but a foundation-model approach to
  "interestingness" that could complement rule-based metrics.

### Adaptive Sampling

- **CUSUM / KCUSUM** — Change-point detection via cumulative sum statistics.
  Non-parametric KCUSUM variant works without distribution assumptions.
- **HashLife (Gosper, 1984)** — Memoizes temporal blocks; exponential speedup
  for periodic patterns. Key insight: cache miss rate is a proxy for
  interestingness. We apply this via: stop sampling once period is detected.
- **BMC Bioinformatics (2004)** — Per-cell activity indicators with skip
  counters achieve 4-5x speedup in CA simulation.
