//! Rule analysis utilities for the probabilistic CA rule space.
//!
//! Provides mean-field pre-filtering to analytically discard rules that
//! inevitably produce extinction or saturation, plus rule classification
//! and generation helpers for MAP-Elites over rule space.

/// Maximum neighbor count for standard Moore neighborhood (8 cells × weight 1).
pub const TABLE_SIZE: usize = 9;

/// Mean-field expected density after one step, given current density ρ.
///
/// For a Moore neighborhood with density ρ, the probability of having
/// exactly k alive neighbors is Binomial(8, ρ). The new density is:
///   ρ' = (1-ρ) Σ_k B(k;8,ρ) spawn[k]  +  ρ Σ_k B(k;8,ρ) keep[k]
pub fn mean_field_step(spawn: &[f32; TABLE_SIZE], keep: &[f32; TABLE_SIZE], rho: f64) -> f64 {
    let mut rho_new = 0.0;
    for k in 0..TABLE_SIZE {
        let pk = binom_pmf(8, k, rho);
        rho_new += (1.0 - rho) * pk * spawn[k] as f64;
        rho_new += rho * pk * keep[k] as f64;
    }
    rho_new
}

/// Mean-field behavior classification.
#[derive(Clone, Debug, PartialEq)]
pub enum MeanFieldClass {
    /// Density always decays toward 0 — rule produces extinction.
    Decays,
    /// Density always grows toward 1 — rule produces saturation.
    Grows,
    /// Has a stable interior fixed point at approximately this density.
    Stable(f64),
}

/// Classify a rule's mean-field behavior by scanning ρ ∈ (0, 1).
///
/// Returns `Stable(ρ*)` if there's a density where ρ' crosses from above ρ
/// to below (a stable fixed point). Returns `Decays` if ρ' < ρ everywhere,
/// `Grows` if ρ' > ρ everywhere.
pub fn mean_field_classify(spawn: &[f32; TABLE_SIZE], keep: &[f32; TABLE_SIZE]) -> MeanFieldClass {
    const N: usize = 500;
    let mut ever_above = false;
    let mut ever_below = false;
    let mut best_crossing = None;

    let mut prev_diff = f64::NAN;
    for i in 1..N {
        let rho = i as f64 / N as f64;
        let rho_new = mean_field_step(spawn, keep, rho);
        let diff = rho_new - rho;

        if diff > 1e-12 {
            ever_above = true;
        }
        if diff < -1e-12 {
            ever_below = true;
        }

        // Stable fixed point: ρ' crosses from above to below.
        if !prev_diff.is_nan() && prev_diff > 0.0 && diff <= 0.0 {
            // Bisect for precision
            let mut lo = (i - 1) as f64 / N as f64;
            let mut hi = rho;
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                let d = mean_field_step(spawn, keep, mid) - mid;
                if d > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            best_crossing = Some((lo + hi) / 2.0);
        }
        prev_diff = diff;
    }

    if let Some(rho_star) = best_crossing {
        MeanFieldClass::Stable(rho_star)
    } else if ever_above && !ever_below {
        MeanFieldClass::Grows
    } else {
        MeanFieldClass::Decays
    }
}

/// Generate B3/S23 (Game of Life) rule tables.
pub fn b3s23() -> ([f32; TABLE_SIZE], [f32; TABLE_SIZE]) {
    let mut spawn = [0.0f32; TABLE_SIZE];
    let mut keep = [0.0f32; TABLE_SIZE];
    spawn[3] = 1.0;
    keep[2] = 1.0;
    keep[3] = 1.0;
    (spawn, keep)
}

/// Generate B36/S23 (HighLife) rule tables.
pub fn b36s23() -> ([f32; TABLE_SIZE], [f32; TABLE_SIZE]) {
    let mut spawn = [0.0f32; TABLE_SIZE];
    let mut keep = [0.0f32; TABLE_SIZE];
    spawn[3] = 1.0;
    spawn[6] = 1.0;
    keep[2] = 1.0;
    keep[3] = 1.0;
    (spawn, keep)
}

/// Generate B2/S (Seeds) rule tables.
pub fn b2s() -> ([f32; TABLE_SIZE], [f32; TABLE_SIZE]) {
    let mut spawn = [0.0f32; TABLE_SIZE];
    let keep = [0.0f32; TABLE_SIZE];
    spawn[2] = 1.0;
    (spawn, keep)
}

/// Generate B3678/S34678 (Day & Night) rule tables.
pub fn b3678s34678() -> ([f32; TABLE_SIZE], [f32; TABLE_SIZE]) {
    let mut spawn = [0.0f32; TABLE_SIZE];
    let mut keep = [0.0f32; TABLE_SIZE];
    for &k in &[3, 6, 7, 8] {
        spawn[k] = 1.0;
    }
    for &k in &[3, 4, 6, 7, 8] {
        keep[k] = 1.0;
    }
    (spawn, keep)
}

/// Parse a Bn/Sm notation string into rule tables.
/// E.g. "B3/S23" → spawn[3]=1.0, keep[2]=1.0, keep[3]=1.0.
pub fn parse_rule_string(s: &str) -> Option<([f32; TABLE_SIZE], [f32; TABLE_SIZE])> {
    let mut spawn = [0.0f32; TABLE_SIZE];
    let mut keep = [0.0f32; TABLE_SIZE];
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let b_part = parts[0].strip_prefix('B').or_else(|| parts[0].strip_prefix('b'))?;
    let s_part = parts[1].strip_prefix('S').or_else(|| parts[1].strip_prefix('s'))?;
    for ch in b_part.chars() {
        let k = ch.to_digit(10)? as usize;
        if k >= TABLE_SIZE {
            return None;
        }
        spawn[k] = 1.0;
    }
    for ch in s_part.chars() {
        let k = ch.to_digit(10)? as usize;
        if k >= TABLE_SIZE {
            return None;
        }
        keep[k] = 1.0;
    }
    Some((spawn, keep))
}

/// Generalized mean-field classification for any neighborhood size n.
/// Works for weighted kernels (treating the max weight sum as n).
pub fn mean_field_classify_general(spawn: &[f32], keep: &[f32], n: usize) -> MeanFieldClass {
    const STEPS: usize = 500;
    let mut ever_above = false;
    let mut ever_below = false;
    let mut best_crossing = None;

    let mut prev_diff = f64::NAN;
    for i in 1..STEPS {
        let rho = i as f64 / STEPS as f64;
        let mut rho_new = 0.0f64;
        for k in 0..=n {
            let pk = binom_pmf(n, k, rho);
            let s = spawn.get(k).copied().unwrap_or(0.0);
            let kk = keep.get(k).copied().unwrap_or(0.0);
            rho_new += (1.0 - rho) * pk * s as f64;
            rho_new += rho * pk * kk as f64;
        }
        let diff = rho_new - rho;

        if diff > 1e-12 {
            ever_above = true;
        }
        if diff < -1e-12 {
            ever_below = true;
        }

        if !prev_diff.is_nan() && prev_diff > 0.0 && diff <= 0.0 {
            let mut lo = (i - 1) as f64 / STEPS as f64;
            let mut hi = rho;
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                let mut mid_new = 0.0f64;
                for k in 0..=n {
                    let pk = binom_pmf(n, k, mid);
                    let s = spawn.get(k).copied().unwrap_or(0.0);
                    let kk = keep.get(k).copied().unwrap_or(0.0);
                    mid_new += (1.0 - mid) * pk * s as f64;
                    mid_new += mid * pk * kk as f64;
                }
                if mid_new - mid > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            best_crossing = Some((lo + hi) / 2.0);
        }
        prev_diff = diff;
    }

    if let Some(rho_star) = best_crossing {
        MeanFieldClass::Stable(rho_star)
    } else if ever_above && !ever_below {
        MeanFieldClass::Grows
    } else {
        MeanFieldClass::Decays
    }
}

/// Check whether a set of HumanRules has a non-trivial mean-field fixed point.
/// Parses the kernel to determine the max weight sum, then runs mean-field analysis.
/// Returns `true` if the rule is worth simulating (has a stable interior density).
pub fn is_viable(rules: &crate::sim::HumanRules) -> bool {
    let weight_sum = match kernel_weight_sum(rules) {
        Some(s) if s > 0 => s as usize,
        _ => return true, // can't analyze → don't filter
    };

    let table_size = weight_sum + 1;
    let mut spawn = vec![0.0f32; table_size];
    let mut keep = vec![0.0f32; table_size];
    for (&w, &p) in rules.spawn.iter() {
        if (w as usize) < table_size {
            spawn[w as usize] = p;
        }
    }
    for (&w, &p) in rules.keep.iter() {
        if (w as usize) < table_size {
            keep[w as usize] = p;
        }
    }

    matches!(
        mean_field_classify_general(&spawn, &keep, weight_sum),
        MeanFieldClass::Stable(_)
    )
}

fn kernel_weight_sum(rules: &crate::sim::HumanRules) -> Option<u32> {
    let mut sum = 0u32;
    for line in &rules.kernel {
        for ch in line.chars() {
            match ch {
                '1'..='9' => sum += ch as u32 - '0' as u32,
                'X' | ' ' | '0' => {}
                _ => return None,
            }
        }
    }
    Some(sum)
}

fn binom_pmf(n: usize, k: usize, p: f64) -> f64 {
    if k > n {
        return 0.0;
    }
    let mut coeff = 1.0f64;
    for i in 0..k {
        coeff *= (n - i) as f64 / (i + 1) as f64;
    }
    coeff * p.powi(k as i32) * (1.0 - p).powi((n - k) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gol_has_stable_fixed_point() {
        let (spawn, keep) = b3s23();
        match mean_field_classify(&spawn, &keep) {
            MeanFieldClass::Stable(rho) => {
                assert!(rho > 0.01 && rho < 0.5, "GoL fixed point {rho} out of range");
            }
            other => panic!("Expected Stable, got {:?}", other),
        }
    }

    #[test]
    fn empty_rule_decays() {
        let spawn = [0.0f32; TABLE_SIZE];
        let keep = [0.0f32; TABLE_SIZE];
        assert_eq!(mean_field_classify(&spawn, &keep), MeanFieldClass::Decays);
    }

    #[test]
    fn all_spawn_grows() {
        let spawn = [1.0f32; TABLE_SIZE];
        let keep = [1.0f32; TABLE_SIZE];
        assert_eq!(mean_field_classify(&spawn, &keep), MeanFieldClass::Grows);
    }

    #[test]
    fn seeds_has_fixed_point() {
        // B2/S: no keep rules, but enough births at ~24% density to
        // maintain the population in mean-field equilibrium.
        let (spawn, keep) = b2s();
        match mean_field_classify(&spawn, &keep) {
            MeanFieldClass::Stable(rho) => {
                assert!(rho > 0.1 && rho < 0.5, "Seeds fixed point {rho} out of range");
            }
            other => panic!("Expected Stable, got {:?}", other),
        }
    }

    #[test]
    fn day_and_night_has_fixed_point() {
        let (spawn, keep) = b3678s34678();
        match mean_field_classify(&spawn, &keep) {
            MeanFieldClass::Stable(rho) => {
                // Day & Night is symmetric around 0.5
                assert!(
                    (rho - 0.5).abs() < 0.1,
                    "Day & Night fixed point {rho} should be near 0.5"
                );
            }
            other => panic!("Expected Stable, got {:?}", other),
        }
    }

    #[test]
    fn highlife_has_fixed_point() {
        let (spawn, keep) = b36s23();
        match mean_field_classify(&spawn, &keep) {
            MeanFieldClass::Stable(rho) => {
                assert!(rho > 0.01 && rho < 0.5, "HighLife fixed point {rho} out of range");
            }
            other => panic!("Expected Stable, got {:?}", other),
        }
    }

    #[test]
    fn parse_rule_string_works() {
        let (spawn, keep) = parse_rule_string("B3/S23").unwrap();
        let (ref_spawn, ref_keep) = b3s23();
        assert_eq!(spawn, ref_spawn);
        assert_eq!(keep, ref_keep);

        let (spawn, keep) = parse_rule_string("B36/S23").unwrap();
        let (ref_spawn, ref_keep) = b36s23();
        assert_eq!(spawn, ref_spawn);
        assert_eq!(keep, ref_keep);

        assert!(parse_rule_string("invalid").is_none());
    }

    #[test]
    fn is_viable_filters_correctly() {
        use crate::sim::HumanRules;

        // B3/S23 should be viable
        let mut rules = HumanRules {
            kernel: vec!["111".into(), "1X1".into(), "111".into()],
            ..Default::default()
        };
        rules.spawn.insert(3, 1.0);
        rules.keep.insert(2, 1.0);
        rules.keep.insert(3, 1.0);
        assert!(is_viable(&rules), "B3/S23 must be viable");

        // Empty rules (no spawn, no keep) should not be viable
        let empty = HumanRules {
            kernel: vec!["111".into(), "1X1".into(), "111".into()],
            ..Default::default()
        };
        assert!(!is_viable(&empty), "Empty rules must not be viable");

        // All-spawn all-keep should not be viable (saturates)
        let mut all = HumanRules {
            kernel: vec!["111".into(), "1X1".into(), "111".into()],
            ..Default::default()
        };
        for i in 0..=8 {
            all.spawn.insert(i, 1.0);
            all.keep.insert(i, 1.0);
        }
        assert!(!is_viable(&all), "Saturating rules must not be viable");
    }
}
