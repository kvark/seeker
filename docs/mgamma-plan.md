# Seeker M-γ: continuous, mass- and energy-conserving substrate

> **The big change.** Retire the discrete CA lineage (M-α / M-β) and rebuild on **Flow-Lenia**,
> then add an **energy economy** on top so that selection becomes *intrinsic* (a consequence of
> the physics) rather than *imposed* (a fitness function you wrote). Computed and rendered on
> `blade-graphics`.

Naming follows the existing M-letter scheme: this is **M-γ-0** and its successors.

---

## 1. Why jump, and why now

- **The discrete substrate is the source of your measurement pain.** The movement-detection
  black box, the glider-vs-oscillator ambiguity, "what is so special about moving" — these are
  artifacts of GoL, not of your analysis. In a continuous substrate, velocity is a plain
  observable (center-of-mass drift), self-repair is native, and the genotype→phenotype map is
  smooth enough that mutation (and gradients) can climb it.
- **M-β was converging on Genelife.** "GoL + heritable genome + density regulation +
  non-totalistic rules + activity statistics" already exists (Packard & McCaskill, *Artificial
  Life* 30(3), 2024). Extending M-β means reinventing it. Jumping substrate is the way to do
  something new instead.
- **Flow-Lenia hands you an evolutionary substrate for free.** Strict mass conservation +
  *parameter localization* means rule parameters are carried by the matter itself, so
  heterogeneous rules coexist, compete, and mix in one world. It already exhibits
  self-replicating, migratory, ecosystem-like multi-species behavior.
- **The gap you actually fill.** Flow-Lenia's own authors name *food consumption as an intrinsic
  selective pressure* as future work, and flag "detecting agency/cognition" and "demonstrating
  intrinsic evolutionary processes" as open. That is exactly the energy layer below. So M-γ is
  **Flow-Lenia + a metabolism/energy economy** — a real, motivated gap, endorsed by the
  substrate's authors, not a reimplementation.

---

## 2. Substrate spec — Flow-Lenia core

Multi-channel, continuous state, continuous time.

- **State.** `C` channels of matter concentration `A_i(x)`.
- **Affinity.** Per channel, convolve a radial kernel and apply a Lenia growth mapping:
  `U_i = growth(K_i * A)`.
- **Flow.**
  ```
  F_i = (1 - α)·∇U_i  −  α·∇A_Σ
  ```
  where `A_Σ = Σ_i A_i` is total local mass, `∇U_i` is the affinity gradient (drives pattern
  formation), and `−∇A_Σ` is a diffusion/regulation term that stops all matter collapsing to a
  point. `α(x) ∈ [0,1]` ramps toward the mass-gradient term as `A_Σ` approaches a critical mass
  `θ_A` (use exponent `n > 1` so affinity dominates over a wider mass range). Gradients via Sobel.
- **Transport.** Move matter along `F` using **reintegration tracking** (Moroz 2020): a
  semi-Lagrangian scheme that moves each cell's mass to a distribution centered at `p + dt·F` and
  sums contributions landing on the same cell → exact total-mass conservation. Sits at the
  frontier between continuous CA and particle systems.
- **Parameter localization.** A parameter field `P(x) ∈ Θ` (kernel weights, growth means/σ,
  radii, …) advects *with* the mass. When multiple sources land on a cell, aggregate by
  mass-weighted average or softmax over fluxes. This is what makes it multi-species.
- **Hard constraint to remember.** Reintegration tracking needs `|F|·dt` bounded (order of a few
  cells); too-large flow gets clipped → mass loss or altered dynamics. See §6.

---

## 3. The new part — energy economy (the M-γ contribution)

This is the research bet **and the undesigned part**. Below is a v0 to build and then revise —
treat it as a hypothesis, not a spec.

- **Energy field `E(x)`** — scalar, separate from matter ("food").
- **Sources.** Localized renewable sources `S(x)` inject energy at rate `r`, capped (your
  hydrothermal-vent intuition). Renewable → sustained ecosystems; finite → succession then death.
- **Diffusion.** `E` diffuses (Laplacian) so exploitable gradients form.
- **Metabolism / coupling.**
  - Growth is *gated* by local energy: scale the positive part of growth by a saturating `g(E)`.
  - Realized growth *consumes* energy: `ΔE −= k·max(ΔA, 0)`.
  - Maintenance drains energy proportional to mass: staying organized costs `c·A` per step.
- **Death & recycling (the interesting fork).** Strict mass conservation forbids deleting mass.
  Options:
  - **(a) Break conservation locally** when energy-starved (mass decays). Simplest; loses the
    clean conservation law.
  - **(b) Closed loop — recommended.** Energy-starved matter converts to an inert **detritus**
    channel `D`; detritus decays back into `E` over time. Now matter is conserved across
    `{live channels + detritus}` and energy *cycles*. This is a real nutrient cycle: you get
    death + recycling without abandoning conservation, and the world can sustain an ecosystem
    instead of burning down to ash. More work, but it's the version worth wanting.

**Why this matters:** creatures whose localized parameters let them find, hold, and convert
energy persist and spread; others starve. **Selection is now a consequence of the economy, not a
fitness function you wrote.** That is the entire reason to do this — it's the fix for the
"designer-in-the-loop" problem that M-β couldn't escape.

**Open design questions (need your call — see §9):** multiplicative vs additive energy gating;
renewable vs finite sources; one energy field or per-channel; detritus recycling rate. These
change the *regime* qualitatively; decide after watching v0.

---

## 4. Engineering plan on `blade-graphics`

Exact Blade API deferred to you (you wrote it). This is the architecture.

**Layout.**
- Fields as storage textures (`rgba32f` and/or multiple targets), **double-buffered (ping-pong)**:
  matter (`C` channels), parameters (as many components as `Θ` needs), energy, detritus.
- WGSL compute shaders dispatched over the grid; `ShaderData`-style bindings.

**Per-timestep pipeline (compute passes):**
1. **Potential/affinity** — convolve kernel(s) with matter → `U_i`. Direct convolution in a
   compute shader, `O(R²)` per cell. Prototype at **256²–512²**, kernel radius `R ≈ 13–20`.
   FFT convolution is a *later* optimization if `R` or grid grows.
2. **Growth + energy gate** — apply growth mapping to `U`, gate by `g(E)` → gated affinity.
3. **Gradients** — Sobel on affinity and on total mass → `∇U`, `∇A_Σ`; assemble `F` with `α(x)`.
4. **Transport** — reintegration-tracking pass moving matter *and* parameters along `F`.
   **Highest-risk kernel (see §6).** Prefer a **gather** formulation to avoid scatter-add atomics.
5. **Energy update** — diffuse `E`, apply sources / consumption / maintenance; update detritus
   and recycling.
6. **Swap buffers.**

**Visualization.** Render matter (channels → color: e.g. mass → luminance, dominant parameter
cluster → hue) with an optional energy overlay. Real-time viz is a Seeker value, and Blade's
graphics path makes the render trivial alongside the compute.

**Blade tradeoff (honest).**
- *Pro:* full control, minimal abstraction, dogfoods your stack; stressing Blade's compute +
  interop path may surface improvements to Blade itself.
- *Con:* thinner ecosystem and tooling than wgpu; fewer eyes on compute-specific corner cases.
  You've lost time to driver/ASPM issues before — a Blade compute quirk would be yours to fix.
- *Fallback rule:* if a needed feature (subgroup ops, the atomics you want, timestamp queries for
  profiling) is missing or buggy in Blade, **don't sink days into it** — prototype that one piece
  in wgpu, keep the science moving, port back later. The deliverable is the emergence result, not
  a proof that Blade can do it.

---

## 5. Milestones

Gate each on a **measured** property, not eyeballing (ties to the harness, §7-F1).

- **M-γ-0** — vanilla Flow-Lenia on Blade, single species. Mass conserved to `ε` numerically;
  real-time at 512²; reproduce a known spatially-localized pattern (SLP).
- **M-γ-1** — parameter localization + multi-species. ✅ CPU reference done
  (`src/flow_lenia.rs` `enable_genome`/`paint_genome`/`seed_species`/`mu_stats`,
  `examples/species.rs`). Growth genome `(μ, σ)` localized into a per-cell field advected
  with the mass by mass-weighted average (same reintegration-tracking scatter). **Coexistence
  confirmed:** three distinct-μ species persist 600 steps, μ variance flat (~2.8e-4), mass
  conserved ~2e-6. **Mixing confirmed, modest:** blend fraction 0 → ~1% where structures merge —
  the low-pass average is real but the default static-SLP regime limits contact; strong mixing
  needs motile species (F2 tuning, not hand-tuning). **v0 decisions:** mass-weighted averaging
  (softmax/quantized inheritance deferred — the homogenization risk barely bites while spots are
  static); single matter channel; only `(μ, σ)` localized, kernel still global.
- **M-γ-2** — energy economy v0 (§3). ✅ CPU reference done (`src/flow_lenia.rs`
  `EnergyParams`/`enable_energy`, `examples/energy.rs`). Does energy competition change
  *which* patterns persist? **Yes, measured:** from one seed, a fed world tracks pure
  Flow-Lenia (9 blobs, activity ~1.1e-3, peak speed ~0.9) while a starved world collapses
  and freezes (4 blobs, activity ~1e-4, peak speed ~0.01); mass conserved to ~1e-6 in
  both. First real test of intrinsic selection — passed. (Ordering note: built before
  M-γ-1 parameter localization, at the human's request. Coupling is single-species for now;
  per-parameter selection lands once genotypes advect with the mass.)
- **M-γ-3** — closed-loop detritus recycling. Look for sustained, non-collapsing ecosystems.

---

## 6. Risks & failure modes (read before coding)

1. **Reintegration tracking is fiddly on GPU.** Scatter-add races, the `|F|·dt` bound (exceed it
   → mass leaks or clip alters dynamics), boundary handling. Mitigate: gather formulation;
   clamp/subcycle large flows; unit-test conservation every step. **Consider MaCE**
   (arXiv:2507.12306, 2025) — a simpler, more robust mass-conserving update with no
   flow-magnitude conditions — as an alternative foundation. It may spare you the worst kernel.
2. **The energy layer may do nothing interesting** (or just damp everything). It's the undesigned
   part. Keep energy **toggleable** so every experiment can A/B against pure Flow-Lenia, and
   budget iteration on the coupling.
3. **Parameter mixing homogenizes the gene pool.** Mass-weighted averaging is a low-pass filter
   on parameters → diversity collapses → the evolution you wanted dies. Watch parameter-field
   variance over time; consider softmax / quantized inheritance to keep species distinct.
4. **The epistemic trap returns.** If you hand-tune until it "looks alive," you've smuggled
   yourself back in as the selector. **Build the harness before heavy tuning.** Randomized,
   non-designed seeds + intrinsic metrics are the discipline that keeps the result real.
5. **Perf.** Direct convolution × `C` channels × large `R` × advection can stall interactivity at
   1024². Get correctness at 256²/512² first; FFT-convolve and fuse passes only if needed.

---

## 7. Followups (the phased program)

- **F1 — Measurement harness (do this early, maybe before M-γ-2).** GPU reductions for total
  mass/energy, mass-distribution entropy, connected-component count/sizes (blob detection),
  velocity distribution; CPU-side **Bedau–Packard evolutionary activity statistics** + a
  novelty/diversity metric; optional frame export for an **ASAL-style VLM interestingness
  oracle**. Reusable standalone crate. This is what lets you *make claims* instead of vibes.
- **F2 — Outer-loop search.** 🟡 In progress (`src/search.rs`, `examples/search.rs`,
  `examples/motility.rs`). MAP-Elites illumination over a 7-gene Flow-Lenia rule genome,
  harness `RunSummary` as behavior descriptors, batched parallel evaluation from a shared
  fixed soup. Two objectives: `Liveness` (concentration × activity) and `Motility`
  (concentration × speed). **The motility objective closed the M-γ-1 loop:** M-γ-1 showed
  localized genomes coexist but barely mix in the static default regime; rather than
  hand-tune a mover (the designer trap), F2 *searches* for one — a coherent glider (mean
  speed ~0.36, peak ~7, ~1.3 blobs) — and dropping it into a two-species world lifts mixing
  to ~5% blend vs ~0% for the static rule. Next: energy-parameter axes (M-γ-2), and
  ASAL/E&E-style VLM-goal search. GPU throughput (batch many worlds) is the unfair
  advantage; the pure-CPU search stays at modest grid/horizon and leans on parallelism.
- **F3 — Ablation science (the actual contribution).** Toggle mass conservation, energy, sources,
  parameter localization, state continuity (discretize), dimensionality (2D↔3D); measure the
  effect on *sustained novelty*. This converts intuition into **necessity claims** — the thing
  that advances the field rather than producing another pretty demo.
- **F4 — Signal systems / proto-intelligence (north star).** Once organisms have internal state +
  environment coupling, measure **predictive information / transfer entropy** (environment →
  organism) and **empowerment**; watch for anticipatory behavior. Never demonstrated de novo, but
  the metrics are definable now, so you can at least watch for it honestly.

---

## 8. Proactive ideas / forks worth a look

- **Meganeura backend for the search phase.** Flow-Lenia is convolution + Sobel + gather +
  pointwise ops — exactly Meganeura's primitives. Building the update on Meganeura (for the
  headless batch search, alongside Blade for the interactive/viz path) buys **autodiff →
  differentiable Flow-Lenia → gradient-based creature/rule search** (cf. Sensorimotor Lenia), not
  just evolutionary. Natural split: Blade = interactive + render, Meganeura = differentiable batch
  search. Dogfoods both. You asked for Blade, so Blade is primary; this is an F2 option.
- **Particle Lenia** (Mordvintsev 2022, the particle analog of Flow-Lenia) as an alternative/
  cheaper substrate for the ecosystem experiments — fewer DOF, and "an organism" is far easier to
  individuate, which matters a lot for the F4 intelligence metrics. Worth a spike.
- **MaCE over reintegration tracking** (see §6.1) — simpler conservation, possibly the better
  foundation. Cheap to evaluate before committing to the hard kernel.
- **Name.** Keeping M-γ for the model. If the energy variant wants its own handle, a Greek
  compound in your usual style (something on *metabolē* / *trophē*). Optional.

---

## 9. Open decisions I need from you

1. **Energy coupling:** multiplicative gate on growth vs additive; renewable vs finite sources.
   ~~(v0 default: multiplicative + renewable.)~~ **Decided for M-γ-2 v0:** multiplicative gate
   `g(E)=E/(E+K)` applied to the *whole* affinity field (not just its positive part — in
   Flow-Lenia the affinity gradient drives transport, so gating the whole thing throttles the
   organizing flow and lets anti-crowding disperse unfed matter); renewable sources; one shared
   energy field. Revisit additive/per-channel/gate-floor after M-γ-1 + F2.
2. **Death model:** break strict conservation (simple) vs closed-loop detritus recycling
   (recommended, more work). **Deferred to M-γ-3:** M-γ-2 has *no* mass-destroying death —
   starvation removes the affinity flow (matter disperses, "death by dispersal") but mass is
   still conserved. Detritus recycling is the M-γ-3 deliverable.
3. **Advection:** port reintegration tracking, or start on MaCE's simpler scheme?
4. **Backend split:** Blade-only for now, or stand up a Meganeura path for differentiable search
   at F2?
5. **Harness timing:** minimal F1 before M-γ-2, or after? (Recommend before.)

---

## References

- Plantec, Hamon, Etcheverry, Oudeyer, Moulin-Frier et al. — *Flow-Lenia: Towards open-ended
  evolution in cellular automata through mass conservation and parameter localization*
  (arXiv:2212.07906).
- Chan, B. — *Lenia: Biology of Artificial Life* (2019).
- Moroz, M. — *Reintegration tracking* (2020).
- *MaCE: General Mass Conserving Dynamics for Cellular Automata* (arXiv:2507.12306, 2025).
- Mordvintsev et al. — *Particle Lenia* (2022).
- Kumar, Stanley et al. — *Automating the Search for Artificial Life with Foundation Models*
  (ASAL) (arXiv:2412.17799, 2024).
- Bedau, M. & Packard, N. — evolutionary activity statistics.
- Packard, N. & McCaskill, J. — *Open-Endedness in Genelife* (*Artificial Life* 30(3), 2024).
- *Expedition & Expansion (E&E)* — VLM-guided novelty search in Flow-Lenia.
