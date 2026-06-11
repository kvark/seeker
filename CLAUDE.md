# Seeker - Research Notes

## What This Project Is

Seeker is an experimental research project exploring how life emerges from simple rules (QLUE - Question of Life, Universe, and Everything). It's a cellular automata-based system that searches for interesting, survivable patterns through evolutionary mutation and selection.

The ultimate goal is understanding the **conditions for emergence** — what properties of a local physics (rule set) allow complex, structured, agent-like behavior to arise from simple initial conditions? This is approached via:
1. Building a reliable **detector** for interestingness at multiple levels
2. Calibrating that detector on known ground truth (Game of Life's cataloged objects)
3. Using the trusted detector to explore unknown rule spaces

Two modes:
- **Play** — manually advance a cellular automaton step-by-step in a TUI
- **Find** — automatically search for interesting rule configurations via evolutionary algorithms

## Methodology: Detector-First

The project deliberately spends time on frozen B3/S23 (standard Game of Life)
before exploring novel rule spaces. Rationale: GoL has the richest catalog of
known objects (gliders, guns, pulsars, methuselahs, spaceships of every size)
with precise expected frequencies from Catagolue's census of ~10¹³ soups. This
serves as a **labeled dataset** for developing and validating the interestingness
detector.

Concretely: if our detector can't find/score a glider, it can't be trusted to
recognize analogous translating structures in an unknown rule. Each level of the
interestingness hierarchy needs calibration against known GoL phenomena before
we point it at the probabilistic rule space.

Current calibration status:
- Levels 1-3 (persistence, structure, dynamics): calibrated — reliably detects
  still lifes, blinkers, toads, beacons, gliders in final and transient states.
- Level 4 (narrative): implemented but not yet validated against known
  methuselah event sequences (R-pentomino should produce ~1103 steps of drama).
- Levels 5-6 (emergence, composability): implemented but need calibration
  against guns and multi-component mechanisms.

Once the detector passes calibration at all levels, the search shifts to
exploring the full probabilistic rule space with the detector as the fitness
landscape.

## Architecture

- `src/grid.rs` — 2D grid with `Option<Cell>` storage, supports Wrap and Dead boundaries
- `src/sim.rs` — Simulation engine: probabilistic CA rules, compiled kernel fast path, period detection, transient analysis
- `src/lab.rs` — Evolutionary search: MAP-Elites quality-diversity, parallel experiments via Choir
- `src/analysis.rs` — Post-stabilization pattern classification (still lifes, oscillators, spaceships) via connected components
- `src/narrative.rs` — Event tracking (splits, merges, births, deaths) for Level 4 measurement
- `src/gpu.rs` — Batch CA simulation on GPU via blade-graphics compute shaders
- `src/rules.rs` — Rule-space analysis: mean-field pre-filter, known rule tables, Bn/Sm parsing
- `src/main.rs` — CLI modes: play (TUI), find (TUI), headless, replay

## How the Search Works

1. **Rules**: Probabilistic CA with a weighted kernel (neighborhood), spawn table, and keep table. In frozen mode, rules are locked to B3/S23 and only initial conditions evolve.
2. **MAP-Elites**: Quality-diversity search over a 2D behavior space (density × interestingness). Uniform selection across occupied cells provides diversity pressure without explicit novelty mechanisms.
3. **Parallel experiments**: Up to `max_active` concurrent simulations via Choir work-stealing executor. Workers check for boring experiments (low variance streak) and abort early.
4. **Mutations (frozen mode)**: 30% fresh soup, 30% focused flip (1-8 cells near existing alive), 20% satellite soup, 20% symmetric soup. Soup strategies: census (16×16@50%), methuselah (10-20@37%), dense small, large sparse.
5. **Mutations (probabilistic mode)**: 1-3 mutations per offspring across spawn/keep tables, kernel weights, grid size, boundary mode.
6. **Fitness (frozen)**: composite of variance, late stabilization, analysis (unique patterns, ships×30, oscillator periods, composability), birth rate, spatial variance, narrative richness, transient ships, high periods.
7. **GPU path**: Bitpacked grids on GPU, B3/S23 compute shader, stats-only readback. Lab remains backend-agnostic.

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
| 3 | **Dynamics** | Components oscillate or translate | Glider, blinker | Yes (period detection, birth rate, transient analysis) |
| 4 | **Narrative** | Structural events happen over time: splits, collisions, births, deaths of components | Methuselah R-pentomino: 1103 steps of drama | Yes (narrative.rs) — needs calibration |
| 5 | **Emergence** | Behavior needs higher-level description; spatial structure is non-uniform and evolving | Gun producing gliders; localized activity regions | Partial (4×4 spatial variance) — needs calibration |
| 6 | **Composability** | Simple classified units compose into functional mechanisms; components are independent and recognizable | Glider + eater interaction; gun = oscillator + emitter | Yes (classified_ratio, independence) — needs calibration |

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

## Current Status

### What works
- MAP-Elites quality-diversity search with uniform cell selection (replaced fitness-proportional)
- Compiled kernel + deterministic fast path (2-3x speedup for B3/S23)
- Early discard via alive_ratio_variance streak detection
- Transient analysis (steps 500-3000) catches gliders before they crash into debris
- Diverse soup strategies: census, methuselah, symmetric, satellite, focused flip
- Multiple mutations per offspring (1-3) for faster co-adaptation
- Batch spawning fills all available worker slots
- ~31% MAP-Elites coverage on 128×128, fitness 253-257, up to 25 transient gliders
- GPU shader compiles and passes tests on lavapipe

### Detector calibration gaps (blocking rule-space exploration)
1. **Pulsar recognition**: `name_pattern` says 12 cells but a pulsar has 48 cells
   decomposed into multiple 8-connected components. Need multi-component
   pseudo-object merging (what apgsearch calls "constellations").
2. **Narrative validation**: Need to run R-pentomino through narrative tracker and
   verify event count/diversity matches expected behavior (high splits early,
   glider births mid-run, then calm).
3. **Emergence calibration**: No known gun has been tested against the spatial
   variance metric. A Gosper gun should produce high sustained spatial variance +
   periodic birth events.
4. **Transient counting**: A long-lived glider is counted at multiple sample points,
   inflating `transient_spaceships`. Should track unique ships (by trajectory hash)
   rather than sample occurrences.

### Known remaining issues
- `avg_velocity` cell field computed but unused — 16 bytes/cell overhead

### Previously fixed (from original analysis)
- ~~Fitness function is coarse~~ → composite fitness with level 2-6 signals
- ~~Only one mutation per generation~~ → 1-3 mutations (50/30/20% distribution)
- ~~No population diversity mechanism~~ → MAP-Elites with uniform cell selection
- ~~Experiment pool grows unbounded~~ → capped archive, concluded experiments pruned
- ~~Worker thread count is hardcoded~~ → workers scaled to max_active

## GPU Acceleration (blade-graphics)

### What's Built (Phase 1, partial)
- `src/gpu.rs`: `GpuSimulator` struct with blade context, pipeline, buffers
- `src/shaders/ca_step.wgsl`: B3/S23 compute shader (workgroup 256, toroidal wrap)
- Bitpacked grid upload, ping-pong stepping, stats readback (alive, births, regions)
- Tests pass on lavapipe (blinker oscillation, pack/unpack roundtrip)
- `GpuSimulator::new` is infallible (panics on failure — shaders must work)

### Phase 1: Complete
- `step(K)` encodes all K steps in one submission, GPU-side buffer clears,
  single sync at batch end
- Shader honors `BoundaryMode::Dead` via `boundary_mode` uniform
- `matches_cpu_simulation` test: GPU output is bit-identical to CPU
  `Simulation` for both boundary modes
- Multi-fidelity funnel wired into `Laboratory`: `gpu_screen` config spawns
  a screener thread that scores candidate batches (level-1/2 signals);
  only the best become CPU experiments. Slots the screener can't fill fall
  back to direct spawning, so a slow GPU (lavapipe) never starves the search.
  See `data/hunt-gpu.ron`.

### Phase 2: Complete
- Table-driven spawn/keep: `GpuBatchConfig` carries `spawn_table` and
  `keep_table` (`[f32; 9]` for Moore neighborhood). Uploaded once to GPU
  storage buffers. Shader indexes `spawn_table[count]` / `keep_table[count]`.
- Philox 2x32-10 counter-based RNG in WGSL: `rand_f32(grid_idx, step, cell_idx)`
  gives deterministic per-cell randomness. Matching CPU reference in `philox2x32()`.
- `rule_mode` switch in shader: 0 = hardcoded B3/S23 (fast path, no RNG),
  1 = table-driven probabilistic. Mode auto-selected from tables.
- Tests: `table_driven_matches_hardcoded` (mode 1 parity with mode 0),
  `highlife_differs_from_gol` (B36/S23 via tables), `probabilistic_rules_use_rng`
  (per-grid RNG seeding).
- `GpuBatchConfig::b3s23()` convenience constructor for existing callers.

### Phase 3: Complete
- Early discard during screening: after each interval readback, grids that
  went extinct (alive=0) or saturated (>90%) are zeroed in both ping-pong
  buffers so subsequent steps skip them (no live cells → no output).
- `GpuContext` shared across screener simulators: one Vulkan device instead
  of one per (width, height, boundary) combination. `GpuSimulator::with_context`
  accepts `Rc<GpuContext>` for sharing.

### Remaining work

**Phase 4: GPU-side analysis**:
- Parallel connected components (label propagation or union-find)
- Would eliminate CPU re-simulation for pattern classification

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

### Performance Target
- 128×128 grid, 1024 grids batched: ~16M cells per step
- ~2000 experiments/second vs current ~1 experiment/second (1000x)

## Roadmap: Detector Calibration → Rule Exploration

### Phase A: Calibrate the detector (current focus)
1. Fix pulsar/pseudo-object recognition (multi-component merging)
2. Validate narrative tracker against R-pentomino expected behavior
3. Test emergence metric against Gosper gun
4. Deduplicate transient ship counting (trajectory hash)

### Phase B: Complete GPU integration
1. ~~Batch K steps per submission, add Dead boundary support~~ done
2. ~~Multi-fidelity funnel: GPU screening → CPU detailed analysis~~ done
3. ~~Table-driven rules + Philox RNG in shader (Phase 2)~~ done

### Phase C: Rule-space exploration
1. ~~Mean-field pre-filter: analytically discard rules with trivial fixed points~~ done
2. MAP-Elites over rule space: genome = spawn/keep/kernel, behavior = ladder scores
3. ~~Landmarks: verify B3/S23, HighLife, Seeds, Day & Night score as expected~~ done
4. Interpolation: are "supports life" regions connected or isolated islands?

### Phase D: Rule-agnostic emergence metrics
1. Damage spreading (Derrida): twin grids, 1-cell perturbation, track Hamming divergence
2. Compression complexity: zstd ratio of spacetime blocks
3. Shift cross-correlation: detect translating structures without rule-specific classification
4. Stimulus-response: perturb stabilized system, measure response locality
