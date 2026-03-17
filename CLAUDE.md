# Seeker - Research Notes

## What This Project Is

Seeker is an experimental research project exploring how life emerges from simple rules (QLUE - Question of Life, Universe, and Everything). It's a cellular automata-based system that searches for interesting, survivable patterns through evolutionary mutation and selection.

Two modes:
- **Play** — manually advance a cellular automaton step-by-step in a TUI
- **Find** — automatically search for interesting rule configurations via evolutionary algorithms

## Architecture

- `src/grid.rs` — 2D wrapped-coordinate grid with `Option<Cell>` storage
- `src/sim.rs` — Simulation engine: probabilistic CA rules, grid advancement, statistics
- `src/lab.rs` — Evolutionary search: parallel experiments, fitness-proportional selection, mutation
- `src/main.rs` — TUI application (ratatui + crossterm), play/find modes

## How the Search Works

1. **Rules**: Probabilistic CA with a weighted kernel (neighborhood), spawn table, and keep table
2. **Parallel experiments**: Up to `max_active` concurrent simulations via Choir work-stealing executor
3. **Fitness-proportional selection**: Parents chosen weighted by fitness score
4. **Mutations**: One of {spawn probs, keep probs, kernel weights, grid size} mutated per offspring
5. **Fitness**: `log2(steps)` for extinct/saturate, `100 - 60*alive_avg` for survivors

## Hierarchy of Interestingness

Why do humans find certain CA patterns interesting? The patterns we've named and
cataloged share a common trait: their behavior is best described using vocabulary
that doesn't exist in the rules. B3/S23 has no concept of "movement," yet a
glider "moves." A gun "produces." A methuselah "tells a story."

The deeper principle: humans find patterns interesting when they exhibit emergent
higher-level structure that can be described in fewer concepts than the raw
cell-level description. We're pattern-matching CA phenomena onto our intuitions
about agency, causality, narrative, and hierarchy.

Seeker's search should climb this ladder:

| Level | Property | What it means | Example | Measured? |
|-------|----------|---------------|---------|-----------|
| 1 | **Persistence** | Didn't die or saturate | Survivors | Yes (fitness) |
| 2 | **Structure** | Has identifiable, separated components | Block + blinker debris | Yes (pattern count, analysis.rs) |
| 3 | **Dynamics** | Components oscillate or translate | Glider, blinker | Yes (period detection, birth rate) |
| 4 | **Narrative** | Structural events happen over time: splits, collisions, births, deaths of components | Methuselah R-pentomino: 1103 steps of drama | **Not yet** |
| 5 | **Emergence** | Behavior needs higher-level description; spatial structure is non-uniform and evolving | Gun producing gliders; localized activity regions | **Not yet** |
| 6 | **Composability** | Simple classified units compose into functional mechanisms; components are independent and recognizable | Glider + eater interaction; gun = oscillator + emitter | **Not yet** |

### Level 4: Narrative — Event Tracking During Simulation

A "narrative" is a sequence of structural events. A methuselah has a rich event
history. A still life has zero events. A gun has periodic emission events.

**Events** (detected by diffing connected components between sampled steps):
- **Split**: one component becomes two (mitosis)
- **Merge**: two components become one (collision)
- **Birth**: a new component appears far from existing ones (emission)
- **Death**: a component disappears (extinction)

**Metric**: `event_count * event_type_diversity`. High = interesting trajectory.

### Level 5: Emergence — Spatial Structure

Emergence means behavior is best described at a higher level than individual
cells. Practical proxy: **spatial entropy**.

Divide the grid into NxN regions. Measure alive-density variance across regions.
High spatial variance + temporal variation = structured, localized dynamics
(guns, factories). Uniform density = boring soup or static noise.

**Metric**: variance of regional alive densities, tracked over time.

### Level 6: Composability — Functional Decomposition

Can the final state be understood as a composition of independent, classified
sub-patterns? A grid full of Unknown blobs scores low. A grid with 3 gliders,
2 blinkers, and a beehive scores high — each component is independently
meaningful.

**Metric**: fraction of alive cells belonging to classified (non-Unknown)
patterns. Already partially measured by `analysis.rs`; needs enhancement to
track classified-cell coverage and component independence.

## High-Level Analysis & Suggestions

### 1. Fitness Function Is Coarse
Any simulation surviving to completion (even a boring static blob) gets fitness ~46-100, dominating experiments that went extinct at step 2048 (fit ~11). The search selects for "didn't die" not "interesting."
**Suggestion**: Incorporate `alive_ratio_variance` into fitness — high variance = oscillation/dynamism.

### 2. Only One Mutation Per Generation
`mutate_snap` applies exactly one mutation type (25% chance each). Co-adapting spawn+keep tables requires many generations.
**Suggestion**: Multiple mutations per offspring (Poisson-distributed), or correlated mutations.

### 3. No Population Diversity Mechanism
Fitness-proportional selection has no diversity pressure. One high-fitness experiment can dominate, causing premature convergence.
**Suggestion**: Tournament selection, novelty bonus, or occasional random injection.

### 4. Experiment Pool Grows Unbounded
All concluded experiments remain in the pool forever. Early lucky survivors permanently skew selection.
**Suggestion**: Sliding window or capped archive of N best. Decay fitness over time.

### 5. Worker Thread Count Is Hardcoded
Only 2 Choir workers but `max_active` defaults to 5 experiments.
**Suggestion**: Scale workers to match cores or `max_active`.

### 6. Kernel Mutation Is String-Level Surgery
Kernel weights mutated via ASCII byte manipulation with `unsafe`. Fragile, limits weights to 0-9.
**Suggestion**: Store kernel as numeric 2D structure, mutate numerically.

### 7. Cell Velocity Tracked But Unused for Fitness
`avg_velocity` computed per cell but never aggregated for fitness. Could be a secondary interestingness signal.

### 8. No Structural Kernel Mutation
Only weight values mutate, not kernel shape. Search is locked into initial topology.
**Suggestion**: Add mutations that add/remove kernel positions to explore different neighborhoods.

## Implemented Improvements

### Early Discard of Boring Experiments
Workers now check alive_ratio_variance at periodic intervals during simulation.
If variance stays below `BORING_VARIANCE_THRESHOLD` (0.0001) for
`BORING_STREAK_LIMIT` (3) consecutive checks, the experiment is aborted via an
atomic flag. This frees up worker slots for more promising experiments.

### Novelty Penalty
Concluded experiments have their signature hashed (quantized alive_ratio,
variance, period). Duplicate signatures in a sliding window of 100 recent
experiments receive a fitness penalty (10 points per duplicate, max 30). This
prevents the population from converging on a single pattern type.

### Multiple Mutations Per Offspring
Instead of exactly one mutation, offspring now receive 1-3 mutations (50%/30%/20%
distribution). This allows spawn+keep tables and initial conditions to co-adapt
faster, reducing the number of generations needed to find interesting
combinations.

## GPU Acceleration Plan (blade-graphics)

### Goal
Simulate thousands of CA grids simultaneously on GPU, replacing the CPU-bound
Choir worker pool. Target: 100-1000x throughput improvement.

### Architecture

```
CPU (lab.rs)                          GPU (blade compute)
┌──────────────┐                     ┌──────────────────────┐
│ Selection     │──── upload ────>   │ Grid buffer          │
│ Mutation      │    (N soups)       │ [N × W × H] bits     │
│ Fitness eval  │                    │                       │
│              │<── readback ────   │ Stats buffer          │
│              │    (stats only)    │ [N × {alive, births,  │
│              │                    │   spatial_var}]        │
└──────────────┘                     └──────────────────────┘
                                      │
                                      │ dispatch K times
                                      ▼
                                     ┌──────────────────────┐
                                     │ CA step shader        │
                                     │ workgroup per grid    │
                                     │ thread per cell       │
                                     └──────────────────────┘
```

### Data Layout

Each grid stored as a bitfield: 1 bit per cell, packed into u32 words.
For a 128×128 grid = 16,384 bits = 512 u32s = 2KB per grid.
1024 grids = 2MB total — fits easily in GPU memory.

No need to transfer full grids back to CPU. Only transfer per-grid statistics
(alive count, birth count, spatial variance) for fitness evaluation.

### Compute Shader (WGSL)

```
// One workgroup per grid, threads cooperate on cells
@group(0) @binding(0) var<storage, read> grids_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> grids_out: array<u32>;
@group(0) @binding(2) var<storage, read_write> stats: array<GridStats>;
@group(0) @binding(3) var<uniform> params: SimParams;

struct SimParams {
    grid_width: u32,
    grid_height: u32,
    words_per_grid: u32,
    num_grids: u32,
    // RNG counter (for probabilistic rules)
    step: u32,
}

struct GridStats {
    alive_count: atomic<u32>,
    birth_count: atomic<u32>,
    region_alive: array<atomic<u32>, 16>,  // 4x4 macro-grid
}

@compute @workgroup_size(256)
fn ca_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let grid_idx = gid.y;  // which grid
    let cell_idx = gid.x;  // which cell (linear)
    if grid_idx >= params.num_grids { return; }

    let x = cell_idx % params.grid_width;
    let y = cell_idx / params.grid_width;
    if y >= params.grid_height { return; }

    let base = grid_idx * params.words_per_grid;

    // Count neighbors (standard Moore neighborhood)
    var count: u32 = 0u;
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            if dx == 0 && dy == 0 { continue; }
            let nx = (i32(x) + dx + i32(params.grid_width)) % i32(params.grid_width);
            let ny = (i32(y) + dy + i32(params.grid_height)) % i32(params.grid_height);
            let ni = u32(ny) * params.grid_width + u32(nx);
            let word = grids_in[base + ni / 32u];
            count += (word >> (ni % 32u)) & 1u;
        }
    }

    // B3/S23 rule (deterministic GoL)
    let ci = y * params.grid_width + x;
    let old_word = grids_in[base + ci / 32u];
    let alive = (old_word >> (ci % 32u)) & 1u;
    let new_alive = select(
        select(0u, 1u, count == 3u),           // dead: birth if 3
        select(0u, 1u, count == 2u || count == 3u),  // alive: survive if 2-3
        alive == 1u
    );

    // Atomic OR into output (each thread writes one bit)
    if new_alive == 1u {
        atomicOr(&grids_out[base + ci / 32u], 1u << (ci % 32u));
        atomicAdd(&stats[grid_idx].alive_count, 1u);
        if alive == 0u {
            atomicAdd(&stats[grid_idx].birth_count, 1u);
        }
        // Spatial variance: accumulate into 4x4 region
        let rx = x * 4u / params.grid_width;
        let ry = y * 4u / params.grid_height;
        atomicAdd(&stats[grid_idx].region_alive[ry * 4u + rx], 1u);
    }
}
```

### blade-graphics Integration Steps

1. **Add dependency**: `blade-graphics = { git = "https://github.com/kvark/blade" }`

2. **New module `src/gpu.rs`**:
   - `GpuSimulator` struct: holds blade `Context`, pipeline, buffers
   - `fn upload_grids(&mut self, soups: &[BitGrid])` — pack soups into GPU buffer
   - `fn step(&mut self, count: usize)` — dispatch CA step shader N times,
     ping-ponging between grids_in and grids_out
   - `fn readback_stats(&self) -> Vec<GridStats>` — read stats buffer back to CPU

3. **Modify `src/lab.rs`**:
   - Add `GpuBatch` mode alongside Choir workers
   - Selection/mutation stays on CPU
   - Instead of spawning one Choir task per experiment, batch 256-1024 soups,
     upload to GPU, run K steps, readback stats, evaluate fitness
   - Early discard: after every ~500 GPU steps, readback stats and discard
     boring grids (zero the grid buffer to skip further work)

4. **Shader compilation**: blade uses Naga + WGSL. Ship shader as
   `include_str!("shaders/ca_step.wgsl")` compiled at runtime.

5. **Probabilistic rules**: For non-frozen mode, add a counter-based RNG
   (Philox) to the shader. Each cell gets deterministic randomness from
   `hash(grid_id, step, cell_idx)`. For frozen GoL mode, no RNG needed.

### Performance Estimate

- 128×128 grid = 16K cells, 1 workgroup of 256 threads = 64 dispatches per grid per step
- With 1024 grids batched: ~16M cells per step
- Modern GPU at ~10 TFLOPS: each step takes ~0.1ms
- 5000 steps = 0.5 seconds for 1024 experiments
- **~2000 experiments/second** vs current ~1 experiment/second

### Phases

1. **Phase 1**: GPU batch simulation for frozen GoL (deterministic B3/S23).
   No RNG on GPU. Stats readback for fitness. CPU still does analysis/selection.

2. **Phase 2**: GPU RNG for probabilistic rules (non-frozen mode). Philox
   counter-based RNG in shader.

3. **Phase 3**: GPU-side early discard. Readback stats periodically, zero out
   boring grids to avoid wasting compute cycles.

4. **Phase 4**: GPU-side analysis. Run connected components on GPU (parallel
   BFS) for even faster fitness evaluation.
