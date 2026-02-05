//! SIMD-optimized color difference using archmage.
//!
//! Provides token-gated SIMD operations for f_pixel::diff().

use crate::pal::f_pixel;

#[cfg(target_arch = "x86_64")]
use archmage::{arcane, Desktop64, SimdToken};

#[cfg(target_arch = "x86_64")]
use magetypes::simd::f32x4;

/// SIMD token for x86_64 with AVX2+FMA.
/// Re-exported for use by other modules.
#[cfg(target_arch = "x86_64")]
pub use archmage::Desktop64 as SimdToken64;

/// Try to summon a SIMD token for the current CPU.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn summon_token() -> Option<SimdToken64> {
    SimdToken64::summon()
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
pub fn summon_token() -> Option<()> {
    None
}

/// Compute perceptual color difference with AVX2 optimizations.
/// Call this when you have a token available.
#[cfg(target_arch = "x86_64")]
#[arcane]
#[inline(always)]
pub fn diff_simd(_token: Desktop64, a: &f_pixel, b: &f_pixel) -> f32 {
    // Use bytemuck to safely cast ARGBF to [f32; 4]
    let arr_a: [f32; 4] = rgb::bytemuck::cast(a.0);
    let arr_b: [f32; 4] = rgb::bytemuck::cast(b.0);

    let px = f32x4::from_array(_token, arr_a);
    let py = f32x4::from_array(_token, arr_b);

    let alpha_diff = f32x4::splat(_token, b.a - a.a);
    let onblack = px - py;
    let onwhite = onblack + alpha_diff;
    let max_sq = (onblack * onblack).max(onwhite * onwhite);
    let arr = max_sq.to_array();
    arr[1] + arr[2] + arr[3]
}

/// Scalar fallback for non-x86_64 or CPUs without AVX2.
#[inline(always)]
pub fn diff_scalar(a: &f_pixel, b: &f_pixel) -> f32 {
    let alpha_diff = b.a - a.a;
    let black_r = a.r - b.r;
    let black_g = a.g - b.g;
    let black_b = a.b - b.b;
    let white_r = black_r + alpha_diff;
    let white_g = black_g + alpha_diff;
    let white_b = black_b + alpha_diff;

    (black_r * black_r).max(white_r * white_r)
        + (black_g * black_g).max(white_g * white_g)
        + (black_b * black_b).max(white_b * white_b)
}

/// Dispatch to best available implementation (with per-call token check).
/// Use diff_simd() directly when you have a token for better performance.
#[inline(always)]
pub fn diff(a: &f_pixel, b: &f_pixel) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if let Some(token) = Desktop64::summon() {
        return diff_simd(token, a, b);
    }
    diff_scalar(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pal::ARGBF;

    fn make_pixel(a: f32, r: f32, g: f32, b: f32) -> f_pixel {
        f_pixel::from(ARGBF { a, r, g, b })
    }

    #[test]
    fn test_diff_matches_scalar() {
        let cases = [
            (make_pixel(0.5, 0.2, 0.3, 0.4), make_pixel(0.6, 0.3, 0.4, 0.5)),
            (make_pixel(0.0, 0.0, 0.0, 0.0), make_pixel(1.0, 1.0, 1.0, 1.0)),
            (make_pixel(1.0, 0.5, 0.5, 0.5), make_pixel(0.5, 0.5, 0.5, 0.5)),
        ];

        for (a, b) in &cases {
            let scalar = diff_scalar(a, b);
            let dispatch = diff(a, b);
            assert!(
                (scalar - dispatch).abs() < 1e-5,
                "Mismatch: scalar={scalar}, dispatch={dispatch}"
            );
        }
    }

    #[test]
    fn test_diff_simd_with_token() {
        let cases = [
            (make_pixel(0.5, 0.2, 0.3, 0.4), make_pixel(0.6, 0.3, 0.4, 0.5)),
            (make_pixel(0.0, 0.0, 0.0, 0.0), make_pixel(1.0, 1.0, 1.0, 1.0)),
        ];

        #[cfg(target_arch = "x86_64")]
        if let Some(token) = summon_token() {
            for (a, b) in &cases {
                let scalar = diff_scalar(a, b);
                let simd = diff_simd(token, a, b);
                assert!(
                    (scalar - simd).abs() < 1e-5,
                    "Mismatch: scalar={scalar}, simd={simd}"
                );
            }
        }
    }
}
