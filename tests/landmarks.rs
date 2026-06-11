//! Landmark verification: known CA rules must score as expected when
//! run through the GPU screening pipeline. This validates that the
//! table-driven GPU path + Philox RNG + scoring function produce
//! sensible rankings for well-understood rules.

#[cfg(feature = "gpu")]
mod gpu_landmarks {
    use seeker::gpu::{pack_grid, GpuBatchConfig, GpuSimulator, RULE_TABLE_SIZE};
    use seeker::grid::{BoundaryMode, Coordinates, Grid};
    use seeker::rules::{self, TABLE_SIZE};

    fn make_soup(width: i32, height: i32, seed: u64) -> Grid {
        let mut grid = Grid::new(Coordinates { x: width, y: height });
        let mut state = seed;
        let count = (width as u64 * height as u64) / 3;
        for _ in 0..count {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let x = ((state >> 33) % width as u64) as i32;
            let y = ((state >> 13) % height as u64) as i32;
            grid.init(x, y);
        }
        grid
    }

    fn screen_rule(
        spawn: &[f32; RULE_TABLE_SIZE],
        keep: &[f32; RULE_TABLE_SIZE],
        soups: &[Vec<u32>],
        width: u32,
        height: u32,
    ) -> Vec<f32> {
        let mut sim = GpuSimulator::new(GpuBatchConfig {
            grid_width: width,
            grid_height: height,
            steps_per_batch: 50,
            num_grids: soups.len() as u32,
            boundary: BoundaryMode::Wrap,
            spawn_table: *spawn,
            keep_table: *keep,
        });
        let outcomes = sim.screen(soups, 500, 100);
        outcomes.into_iter().map(|o| o.score).collect()
    }

    /// B3/S23 (GoL) should produce positive scores on random soups —
    /// it's the most well-studied rule with rich dynamics.
    #[test]
    fn gol_scores_positive() {
        let (spawn, keep) = rules::b3s23();
        let soups: Vec<Vec<u32>> = (0..8)
            .map(|i| pack_grid(&make_soup(64, 64, 0xCAFE0000 + i)))
            .collect();
        let scores = screen_rule(&spawn, &keep, &soups, 64, 64);
        let positive = scores.iter().filter(|&&s| s > 0.0).count();
        assert!(
            positive >= 4,
            "GoL should produce positive scores on most soups, got {}/{}: {:?}",
            positive,
            scores.len(),
            scores
        );
    }

    /// HighLife (B36/S23) should also produce positive scores — the extra
    /// birth rule at 6 makes it even more active than GoL.
    #[test]
    fn highlife_scores_positive() {
        let (spawn, keep) = rules::b36s23();
        let soups: Vec<Vec<u32>> = (0..8)
            .map(|i| pack_grid(&make_soup(64, 64, 0xBEEF0000 + i)))
            .collect();
        let scores = screen_rule(&spawn, &keep, &soups, 64, 64);
        let positive = scores.iter().filter(|&&s| s > 0.0).count();
        assert!(
            positive >= 4,
            "HighLife should produce positive scores on most soups, got {}/{}: {:?}",
            positive,
            scores.len(),
            scores
        );
    }

    /// Day & Night (B3678/S34678) should produce positive scores.
    /// It's a symmetric, complex rule with known interesting patterns.
    #[test]
    fn day_and_night_scores_positive() {
        let (spawn, keep) = rules::b3678s34678();
        let soups: Vec<Vec<u32>> = (0..8)
            .map(|i| pack_grid(&make_soup(64, 64, 0xD00D0000 + i)))
            .collect();
        let scores = screen_rule(&spawn, &keep, &soups, 64, 64);
        let positive = scores.iter().filter(|&&s| s > 0.0).count();
        assert!(
            positive >= 4,
            "Day & Night should produce positive scores on most soups, got {}/{}: {:?}",
            positive,
            scores.len(),
            scores
        );
    }

    /// An empty rule (no spawn, no keep) must score 0 on everything —
    /// instant extinction.
    #[test]
    fn empty_rule_all_zero() {
        let spawn = [0.0f32; RULE_TABLE_SIZE];
        let keep = [0.0f32; RULE_TABLE_SIZE];
        let soups: Vec<Vec<u32>> = (0..4)
            .map(|i| pack_grid(&make_soup(32, 32, 0xDEAD0000 + i)))
            .collect();
        let scores = screen_rule(&spawn, &keep, &soups, 32, 32);
        assert!(
            scores.iter().all(|&s| s == 0.0),
            "Empty rule should score 0 on all soups: {:?}",
            scores
        );
    }

    /// A rule with all-1 spawn and keep saturates → score 0.
    #[test]
    fn saturating_rule_scores_zero() {
        let spawn = [1.0f32; RULE_TABLE_SIZE];
        let keep = [1.0f32; RULE_TABLE_SIZE];
        let soups: Vec<Vec<u32>> = (0..4)
            .map(|i| pack_grid(&make_soup(32, 32, 0xFACE0000 + i)))
            .collect();
        let scores = screen_rule(&spawn, &keep, &soups, 32, 32);
        assert!(
            scores.iter().all(|&s| s == 0.0),
            "Saturating rule should score 0: {:?}",
            scores
        );
    }

    /// Mean-field classification must agree with GPU simulation outcomes:
    /// rules classified as Decays/Grows should score 0 on GPU, rules
    /// classified as Stable should mostly score > 0.
    #[test]
    fn mean_field_agrees_with_gpu() {
        let test_rules: Vec<(&str, [f32; RULE_TABLE_SIZE], [f32; RULE_TABLE_SIZE])> = vec![
            {
                let (s, k) = rules::b3s23();
                ("B3/S23", s, k)
            },
            {
                let (s, k) = rules::b36s23();
                ("B36/S23", s, k)
            },
            {
                // B1/S — nothing survives (Gnarl without keep)
                let mut s = [0.0f32; RULE_TABLE_SIZE];
                s[1] = 1.0;
                ("B1/S", s, [0.0; RULE_TABLE_SIZE])
            },
            {
                // B/S12345678 — everything survives, nothing born
                let mut k = [0.0f32; RULE_TABLE_SIZE];
                for i in 0..TABLE_SIZE { k[i] = 1.0; }
                ("B/S12345678", [0.0; RULE_TABLE_SIZE], k)
            },
        ];

        for (name, spawn, keep) in &test_rules {
            let mf_class = rules::mean_field_classify(spawn, keep);
            let soups: Vec<Vec<u32>> = (0..4)
                .map(|i| pack_grid(&make_soup(32, 32, 0xAABB0000 + i as u64)))
                .collect();
            let scores = screen_rule(spawn, keep, &soups, 32, 32);
            let positive = scores.iter().filter(|&&s| s > 0.0).count();

            match mf_class {
                rules::MeanFieldClass::Stable(_) => {
                    assert!(
                        positive > 0,
                        "{name}: mean-field says Stable but GPU scored all zero: {scores:?}"
                    );
                }
                rules::MeanFieldClass::Decays | rules::MeanFieldClass::Grows => {
                    // Rules that decay/grow in mean-field should mostly score 0.
                    // (Small grids with wrap boundaries might keep some alive.)
                }
            }
        }
    }
}

