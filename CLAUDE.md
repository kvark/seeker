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
- `src/emergence.rs` — Rule-agnostic emergence metrics: Derrida damage-spreading, spacetime complexity, shift cross-correlation, rule transects
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
- Rule-agnostic emergence metrics (Derrida, spacetime complexity, shift correlation)
  wired into simulation loop, fitness function, and headless output
- Rule transect sweeps (examples/transect.rs): GoL spreading_rate ≈ 1.098 confirms
  near-critical behavior; HighLife ≈ 1.108 (slightly more chaotic)
- Rule-space search with emergence-aware fitness reaches 42% MAP-Elites coverage
- Parallelized emergence measurements (transects, 2D slices, critical search) via
  std::thread::scope — ~2.5-4x speedup on multi-core machines
- Critical surface search with progress reporting, mean-field pre-filter,
  multi-seed averaging, and position-seeded probabilistic Derrida

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
2. ~~MAP-Elites over rule space: genome = spawn/keep/kernel, behavior = ladder scores~~ done
3. ~~Landmarks: verify B3/S23, HighLife, Seeds, Day & Night score as expected~~ done
4. ~~Interpolation: are "supports life" regions connected or isolated islands?~~ done (rule_transect)

### Phase D: Rule-agnostic emergence metrics
1. ~~Damage spreading (Derrida): twin grids, 1-cell perturbation, track Hamming divergence~~ done
2. ~~Spacetime complexity: Shannon entropy of block densities + temporal autocorrelation~~ done
3. ~~Shift cross-correlation: detect translating structures without rule-specific classification~~ done
4. Stimulus-response: perturb stabilized system, measure response locality

### Phase E: Critical surface mapping — done
1. ~~High-resolution transects (41 points, 96×96, 8-seed, 2000 steps)~~ done
2. ~~2D slices: spawn[2]×spawn[3] and spawn[3]×spawn[6] at 21×21~~ done
3. ~~Multi-seed averaging (4-8 seeds per measurement point)~~ done
4. ~~Position-seeded probabilistic Derrida (splitmix64 hash of step×y×x)~~ done
5. ~~Critical surface search: 1000 random viable rules cataloged~~ done
6. ~~Parallelize transects, slices, critical search (std::thread::scope)~~ done

Key findings:
- GoL spreading_rate ≈ 1.074 at high resolution (96×96, 8 seeds); HighLife ≈ 1.078
  (nearly indistinguishable — transition at t≈0.525 adds B6)
- **Sharp phase transitions**: 2D slices show discontinuous Derrida jumps (0.4→0.8→0.9),
  suggesting first-order phase transitions in rule space, not smooth gradients
- **Complexity peaks at viability boundary**: The highest complexity scores (7.3-7.8)
  occur right where a rule barely supports life — the transition from extinction to
  sustained dynamics. The "edge of emergence" is literally the survival threshold.
- Two classes of critical rules discovered:
  - **GoL-like** (Derrida ≈ 1.00, e.g. critical-rule3): recognizable patterns
    (blinkers, blocks), low complexity (1.4-1.5), sparse (7-10% alive)
  - **Novel** (Derrida ≈ 1.08-1.11, e.g. critical-rule7): no classified patterns,
    higher complexity (3.8-4.1), dense (25-27%), spawn[0]=0.91 + broad survival
- Criticality alone doesn't predict pattern formation: true criticality (λ≈1.0) with
  GoL-like rules produces familiar objects, while slightly supercritical novel rules
  produce unclassified but structured dynamics
- 2D slice spawn[2]×spawn[3] (GoL keep): 4 quadrants — ordered (both low), edge
  (one high), chaotic (both high). Complexity peak at the border between extinct
  and sustained (spawn[3]≈0.10, spawn[2]≈0.75-0.95).
- GoL→HighLife transect: complexity decreases monotonically from 4.5 to 2.5 as
  HighLife character increases — B6 adds chaos without adding structure
- **1000-rule critical surface survey** (96×96, 8 seeds, 2000 steps, ~20 min parallel):
  - 617/1000 pass mean-field filter, 573 have measurable Derrida signal
  - Phase distribution: 132 ordered (23%), **421 critical (73%)**, 20 chaotic (3.5%)
  - 422 rules score ≥ 80 criticality; avg complexity 2.79, max complexity **9.58**
  - Top-10 most critical rules (score 98-99) have λ ∈ [0.96, 1.03], low alive (6-22%)
  - **Criticality is the dominant regime**: 73% of viable rules are near-critical,
    suggesting that the critical surface is not a thin boundary but the bulk of
    viable rule space. Rules that sustain life are overwhelmingly near-critical.
  - Higher complexity (3-4.4) appears at slightly sub-critical spreading rates (λ≈0.96)

### Phase F: Deeper exploration (next)
1. Test novel critical rules for pattern formation (still lifes, oscillators, ships)
2. Longer simulations (10K+ steps) to check for methuselah-like transients
3. Compare narrative richness of novel critical rules vs GoL
4. Search for rules that produce higher complexity than GoL (complexity > 5)
5. Map critical surface in higher dimensions (vary 3+ parameters simultaneously)
6. Use parallelized search to run 10K+ sample surveys with finer mean-field binning
