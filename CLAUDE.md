# Seeker — Research Notes

## What This Project Is

Seeker is an experimental research project exploring how life emerges from simple
physics (QLUE — the Question of Life, Universe, and Everything). The goal is to
understand the **conditions for emergence**: what properties of a local physics
allow complex, structured, agent-like behavior to arise, and — the harder
question — how selection can become *intrinsic* to the physics rather than
imposed by a fitness function we wrote.

## The Pivot: M-γ — a continuous, mass- and energy-conserving substrate

Seeker began on **discrete cellular automata** (Game of Life and probabilistic
generalizations), building an "interestingness detector" and searching rule space
with it. That lineage (call it **M-α / M-β**) reached diminishing returns:

- The measurement pain — "what is movement," glider-vs-oscillator ambiguity,
  multi-component object recognition — was an artifact of the *discrete* substrate,
  not of the questions we care about.
- M-β was converging on prior art (Genelife: GoL + heritable genome + density
  regulation). Extending it meant reinventing existing work.

So Seeker is pivoting to **M-γ**: rebuild on **Flow-Lenia** (a continuous,
mass-conserving substrate with parameter localization), then add an **energy
economy** on top so that selection falls out of the physics. In a continuous
substrate, velocity is a plain observable (center-of-mass drift), self-repair is
native, and the genotype→phenotype map is smooth enough for mutation and gradients
to climb. The full plan lives in `docs/mgamma-plan.md`.

The **gap M-γ actually fills**: Flow-Lenia's own authors name *food consumption as
an intrinsic selective pressure* as future work. M-γ is Flow-Lenia + a
metabolism/energy economy — a motivated gap, not a reimplementation.

## Substrate spec — Flow-Lenia core

Multi-channel, continuous state. Per step:

1. **Affinity.** Convolve a radial (Lenia) kernel with each channel, then map
   through a Gaussian growth function → an affinity field `U_i ∈ [-1, 1]`. Growth
   is **not added** to the state; it only shapes where matter wants to flow.
2. **Flow.** `F_i = (1 - α)·∇U_i − α·∇A_Σ`, where `A_Σ` is total local mass and
   `α(x) = clamp((A_Σ/θ_A)^n, 0, 1)` ramps in a mass-regulation (anti-crowding)
   term as `A_Σ` approaches a critical mass `θ_A`. Gradients via Sobel.
3. **Transport.** Move matter along `F` with **reintegration tracking**: each
   cell's mass lands on a unit box centered at `p + dt·F`, split bilinearly across
   the four overlapped cells. The split weights sum to 1, so **total mass is
   conserved exactly**. Because mass is only moved, total mass is a constant of
   motion fixed by the initial condition; structure arises from redistribution.
4. **Parameter localization** (M-γ-1+). A parameter field advects *with* the mass,
   so heterogeneous rules coexist, compete, and mix in one world — this is what
   makes it multi-species.

## The new part — energy economy (the M-γ contribution)

The research bet, to build and revise (a hypothesis, not a spec):

- **Energy field `E(x)`** — scalar "food," separate from matter.
- **Sources.** Localized renewable sources inject energy at a capped rate. `E`
  diffuses so exploitable gradients form.
- **Metabolism.** Growth is *gated* by local energy; realized growth *consumes*
  energy; maintenance drains energy proportional to mass (staying organized costs).
- **Death & recycling (closed loop, recommended).** Energy-starved matter converts
  to an inert **detritus** channel that decays back into `E` over time. Matter is
  conserved across {live channels + detritus}; energy cycles. A real nutrient
  cycle — death and recycling without abandoning conservation.

**Why it matters:** creatures whose localized parameters let them find, hold, and
convert energy persist and spread; others starve. Selection becomes a consequence
of the economy, not a fitness function — the fix for the "designer-in-the-loop"
problem the discrete lineage couldn't escape.

## Architecture

Current (M-γ):

- `src/flow_lenia.rs` — Flow-Lenia CPU reference substrate. Multi-channel
  continuous field, ring-kernel convolution, Lenia growth, Sobel gradients, flow
  assembly, reintegration-tracking transport (bilinear scatter, exact mass
  conservation). This is the ground truth to validate a GPU port against. Also
  carries the **energy economy** (M-γ-2, `EnergyParams` + `enable_energy`): a
  toggleable scalar energy field with renewable sources, diffusion, a saturating
  growth gate `g(E)=E/(E+K)`, and consumption/maintenance costs. Off by default
  (pure Flow-Lenia is the A/B baseline); mass stays conserved when it's on. And
  **parameter localization** (M-γ-1, `enable_genome`): the growth genome `(μ, σ)`
  becomes a per-cell field that advects with the mass through the same transport
  (mass-weighted average on merge), so heterogeneous rules coexist and mix in one
  channel. Single-channel v0; toggleable; `mu_stats` exposes the gene-pool
  variance (homogenization watch).
- `src/harness.rs` — measurement harness (F1). Intrinsic metrics over a raw
  `&[f32]` field (substrate-agnostic — matter, energy, detritus, GPU readback):
  field stats (total, occupancy, Shannon entropy + a localization/concentration
  score, peak, variance), connected-component blob detection (toroidal
  8-connectivity, per-blob cell count / mass / circular-mean centroid), temporal
  activity (per-step L1 change), and a `Tracker` that matches blobs across frames
  to recover a velocity distribution. `measure_run` folds a run into a
  `RunSummary` behavior fingerprint for F2.
- `examples/flow_lenia.rs` — headless run: mass-drift + center-of-mass drift
  report, animated GIF export.
- `src/search.rs` — outer-loop search (F2). MAP-Elites illumination over the
  Flow-Lenia rule genome; harness metrics as behavior descriptors; batched
  parallel evaluation. Finds rules instead of hand-tuning them.
- `examples/measure.rs` — the harness in action: metric time series + run
  summary.
- `examples/search.rs` — F2 illumination: runs MAP-Elites, prints the behavior
  map and the elite rules in notable regions.
- `examples/energy.rs` — M-γ-2 A/B: identical seed run pure vs starved vs fed,
  harness fingerprints side by side, plus a two-panel (matter | energy) GIF.
- `examples/species.rs` — M-γ-1 multi-species: genome territories under a shared
  soup; tracks μ variance (coexistence) and blend fraction (mixing); GIF colors
  species by hue (μ) and mass by brightness.

Legacy (M-α / M-β discrete lineage — retained until M-γ subsumes their function,
then to be removed; all history is in git):

- `src/grid.rs`, `src/sim.rs` — discrete grid + probabilistic CA engine
- `src/lab.rs` — MAP-Elites evolutionary search
- `src/analysis.rs` — connected-component pattern classification
- `src/emergence.rs` — Derrida damage-spreading, spacetime complexity, directed
  critical-surface search
- `src/narrative.rs`, `src/rules.rs`, `src/gpu.rs`, `src/tui.rs`, `src/render.rs`,
  `src/main.rs` — event tracking, rule tables, discrete GPU shader, TUI, discrete
  binary
- `examples/transect.rs`, `examples/critical_search.rs`,
  `examples/critical_survey.rs` — emergence-metric experiments

## Milestones

Each is gated on a **measured** property, not eyeballing.

- **M-γ-0 — vanilla Flow-Lenia, single species.** ✅ CPU reference done. Mass
  conserved to `~2e-6` relative drift over 600 steps (f32 accumulation floor);
  seeded blobs self-organize into persistent localized structure. GPU port (Blade)
  and moving-SLP tuning are the open items — *tuning belongs to the search harness
  (F1/F2), not hand-tuning* (see §Discipline).
- **M-γ-1 — parameter localization + multi-species.** ✅ CPU done (`enable_genome`,
  per-cell growth `(μ, σ)` advected with the mass by mass-weighted average;
  `examples/species.rs`). **Coexistence confirmed:** three species with distinct μ
  persist together over 600 steps, mass-weighted μ variance flat (~2.8e-4), mass
  conserved to ~2e-6. **Mixing confirmed but modest:** blend fraction (mass at μ in
  *neither* seed) rises 0 → ~1% monotonically where structures merge — the
  mass-weighted average is real, but the default regime makes static, non-migrating
  spots, so contact is limited. Strong mixing wants motile species → an F2 tuning
  target, *not* hand-tuning (see §Discipline). Homogenization watch (risk #3): with
  static spots the low-pass barely bites; revisit softmax/quantized inheritance once
  species move and collide. **v0 scope:** single matter channel (per-channel
  genomes deferred); only `(μ, σ)` localized (kernel still global).
- **M-γ-2 — energy economy v0.** ✅ CPU done (`EnergyParams`, gate + sources +
  diffusion + upkeep). First real test of intrinsic selection — and it changes
  *which* patterns persist, measured through F1: from one seed, a fed world tracks
  pure Flow-Lenia (9 blobs, activity ~1.1e-3, peak speed ~0.9) while a starved
  world collapses and goes inert (4 blobs, activity ~1e-4, peak speed ~0.01);
  mass conserved to ~1e-6 in both. **v0 decisions made:** multiplicative gate on
  the *whole* affinity (its gradient drives transport, so gating it throttles the
  organizing flow and anti-crowding disperses the rest — "death by dispersal");
  renewable sources; single shared energy field; no mass-destroying death yet.
  Open: does the gate want a floor / a per-channel field; tune costs via F2, not
  by hand.
- **M-γ-3 — closed-loop detritus recycling.** Next: energy-starved matter converts
  to an inert detritus channel that decays back into `E`, so mass is conserved
  across {live + detritus} and the world recycles instead of freezing. Look for
  sustained, non-collapsing ecosystems.

## Followups (the phased program)

- **F1 — Measurement harness (early, maybe before M-γ-2).** 🟡 In progress. Done:
  intrinsic field metrics in `src/harness.rs` — total mass, occupancy, entropy +
  concentration, connected-component count/sizes, activity, and blob-tracked
  velocity distribution, folded into a `RunSummary` behavior fingerprint. Deferred:
  Bedau–Packard evolutionary activity statistics (need a heritable component to
  track → arrives with M-γ-1 parameter localization); optional VLM interestingness
  oracle. This is what lets us *make claims* instead of vibes.
- **F2 — Outer-loop search.** 🟡 In progress. `src/search.rs` — MAP-Elites
  illumination over a 7-gene Flow-Lenia rule genome (growth μ/σ, ring peak/width,
  dt, θ_A, ramp n); behavior descriptors = harness `RunSummary` (concentration ×
  activity); quality = a liveness score (persistent, dynamic, non-degenerate).
  Batched parallel evaluation via `thread::scope`. Every genome runs from the
  same fixed soup, so behavior reflects the rule. GPU throughput (batch many
  worlds) is the unfair advantage — pure-CPU scalar convolution is the cost floor,
  so the CPU search stays at modest grid/horizon and leans on parallelism.
  Energy-parameter axes arrive with M-γ-2.
- **F3 — Ablation science (the actual contribution).** Toggle mass conservation,
  energy, sources, parameter localization, state continuity, dimensionality;
  measure the effect on *sustained novelty*. Converts intuition into necessity
  claims.
- **F4 — Signal systems / proto-intelligence (north star).** Once organisms have
  internal state + environment coupling, measure predictive information / transfer
  entropy (environment → organism) and empowerment; watch for anticipatory
  behavior.

## Engineering notes

- **Compute on `blade-graphics`.** Fields as ping-pong storage textures; WGSL
  compute passes for convolution → growth+gate → gradients → transport → energy →
  swap. Prototype at 256²–512², kernel radius `R ≈ 13–20`. Direct convolution
  first; FFT-convolve only if `R`/grid grows. Prefer a **gather** transport
  formulation to avoid scatter-add atomics. Fallback rule: if a needed Blade
  feature is missing/buggy, prototype that one piece in wgpu and port back — the
  deliverable is the emergence result, not a proof Blade can do it.
- **Alternative transport: MaCE** (arXiv:2507.12306) — a simpler mass-conserving
  update with no flow-magnitude condition. Cheap to evaluate before committing to
  the reintegration-tracking GPU kernel.
- **Reintegration tracking bound.** `|F|·dt` must stay bounded (a few cells). The
  CPU reference clamps displacement for advection fidelity; mass conservation holds
  regardless (bilinear splat always conserves).

## Discipline (read before tuning)

The epistemic trap: if you hand-tune parameters until it "looks alive," you've
smuggled yourself back in as the selector — the exact problem M-γ exists to escape.
**Build the harness before heavy tuning.** Randomized, non-designed seeds +
intrinsic metrics are the discipline that keeps the result real. Keep the energy
layer **toggleable** so every experiment can A/B against pure Flow-Lenia.

## Risks

1. **Transport kernel is fiddly on GPU** (scatter races, the `|F|·dt` bound,
   boundaries). Mitigate with a gather formulation, clamp/subcycle, and a
   conservation unit-test every step. Consider MaCE.
2. **The energy layer may do nothing** (or damp everything). It's the undesigned
   part; keep it toggleable and budget iteration on the coupling.
3. **Parameter mixing homogenizes the gene pool** → diversity collapses. Watch
   parameter-field variance.
4. **Perf.** Direct convolution × channels × large `R` × advection can stall
   interactivity at 1024². Get correctness at 256²/512² first.

## Open decisions (for the human)

1. Energy coupling: multiplicative gate on growth vs additive; renewable vs finite
   sources. (v0 default: multiplicative + renewable.)
2. Death model: break strict conservation (simple) vs closed-loop detritus
   recycling (recommended, more work).
3. Advection: reintegration tracking vs MaCE's simpler scheme.
4. Backend: Blade-only, or stand up a Meganeura path for *differentiable*
   Flow-Lenia (gradient-based creature/rule search) at F2.
5. Harness timing: minimal F1 before M-γ-2 (recommended) or after.

## References

- Plantec et al. — *Flow-Lenia: Towards open-ended evolution in cellular automata
  through mass conservation and parameter localization* (arXiv:2212.07906).
- Chan, B. — *Lenia: Biology of Artificial Life* (2019).
- Moroz, M. — *Reintegration tracking* (2020).
- *MaCE: General Mass Conserving Dynamics for Cellular Automata* (arXiv:2507.12306).
- Mordvintsev et al. — *Particle Lenia* (2022).
- Kumar, Stanley et al. — *Automating the Search for Artificial Life with
  Foundation Models* (ASAL) (arXiv:2412.17799, 2024).
- Bedau, M. & Packard, N. — evolutionary activity statistics.
- Packard, N. & McCaskill, J. — *Open-Endedness in Genelife* (*Artificial Life*
  30(3), 2024).
