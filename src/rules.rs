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
}
