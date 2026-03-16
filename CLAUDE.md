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
