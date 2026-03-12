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
