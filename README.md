# seeker
[![check](https://github.com/kvark/seeker/actions/workflows/check.yaml/badge.svg)](https://github.com/kvark/seeker/actions/workflows/check.yaml)

Experimental playground for seeking the answer to [QLUE](https://kvark.github.io/seeker/) — the Question of Life, Universe, and Everything: what local physics lets complex, agent-like behavior emerge, and how can selection become *intrinsic* to the physics rather than imposed by a fitness function?

![grid](etc/grid-shot.png)

## Direction: M-γ

Seeker is pivoting from discrete cellular automata (Game of Life and probabilistic generalizations — the **M-α / M-β** lineage) to **M-γ**: a continuous, mass-conserving substrate based on [Flow-Lenia](https://arxiv.org/abs/2212.07906), with an **energy economy** layered on top so that survival and selection fall out of the physics. In a continuous substrate velocity is a plain observable, self-repair is native, and the genotype→phenotype map is smooth. See [`docs/mgamma-plan.md`](docs/mgamma-plan.md) and `CLAUDE.md` for the full program.

## Current state

- `src/flow_lenia.rs` — Flow-Lenia CPU reference substrate: multi-channel continuous field, ring-kernel convolution, Lenia growth, Sobel-gradient flow, and reintegration-tracking transport that conserves total mass **exactly**. This is the ground truth for a later [blade-graphics](https://github.com/kvark/blade) GPU port.
- `examples/flow_lenia.rs` — headless demo reporting mass drift and center-of-mass drift, with animated GIF export.

```sh
cargo test flow_lenia                              # mass-conservation + localization tests
cargo run --release --example flow_lenia           # run + write data/flow-lenia.gif
```

Milestone status: **M-γ-0** (vanilla Flow-Lenia, single species) — CPU reference done, mass conserved to ~2e-6 relative drift over 600 steps; GPU port and search harness are next. See `CLAUDE.md` for milestones M-γ-1..3 and the F1–F4 followup program.

## Legacy (discrete lineage)

The discrete-CA engine, evolutionary search, interestingness detector, and emergence metrics (`sim`, `lab`, `analysis`, `emergence`, `narrative`, `rules`, `gpu`, `tui`) remain in-tree until M-γ subsumes their function. Feature-gated extras:

```sh
cargo run --features tui         # interactive discrete-CA TUI
cargo run --features gpu         # GPU-accelerated discrete search
```
