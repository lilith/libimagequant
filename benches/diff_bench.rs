use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use archmage::{Desktop64, SimdToken};

// Test data: simulate f_pixel ARGBF layout [a, r, g, b]
fn generate_pixels(n: usize) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let a: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            let f = i as f32 / n as f32;
            [0.5 + f * 0.2, f * 0.3, f * 0.5, f * 0.4]
        })
        .collect();
    let b: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            let f = (n - i) as f32 / n as f32;
            [0.6 - f * 0.1, f * 0.4, f * 0.3, f * 0.6]
        })
        .collect();
    (a, b)
}

/// Scalar implementation (reference)
#[inline(always)]
fn diff_scalar(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let alpha_diff = b[0] - a[0];
    let mut sum = 0.0f32;
    for i in 1..4 {
        let on_black = a[i] - b[i];
        let on_white = on_black + alpha_diff;
        sum += on_black.powi(2).max(on_white.powi(2));
    }
    sum
}

/// wide::f32x4 implementation (for comparison)
#[inline(always)]
fn diff_wide(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    use wide::f32x4;
    let px = f32x4::from(*a);
    let py = f32x4::from(*b);
    let alpha_diff = f32x4::splat(b[0] - a[0]);
    let onblack = px - py;
    let onwhite = onblack + alpha_diff;
    let max_sq = (onblack * onblack).max(onwhite * onwhite);
    let arr: [f32; 4] = max_sq.into();
    arr[1] + arr[2] + arr[3]
}

/// archmage/magetypes f32x4 implementation with token
#[cfg(target_arch = "x86_64")]
mod archmage_impl {
    use archmage::{arcane, Desktop64, SimdToken};
    use magetypes::simd::f32x4;

    #[arcane]
    #[inline(always)]
    pub fn diff_archmage(_token: Desktop64, a: &[f32; 4], b: &[f32; 4]) -> f32 {
        let px = f32x4::from_array(_token, *a);
        let py = f32x4::from_array(_token, *b);
        let alpha_diff = f32x4::splat(_token, b[0] - a[0]);
        let onblack = px - py;
        let onwhite = onblack + alpha_diff;
        let max_sq = (onblack * onblack).max(onwhite * onwhite);
        let arr = max_sq.to_array();
        arr[1] + arr[2] + arr[3]
    }

    /// Batch processing with archmage - single token summon for entire batch
    #[inline(never)]
    pub fn diff_batch_archmage(pixels_a: &[[f32; 4]], pixels_b: &[[f32; 4]]) -> f32 {
        let Some(token) = Desktop64::summon() else {
            return pixels_a.iter().zip(pixels_b).map(|(a, b)| super::diff_scalar(a, b)).sum();
        };
        pixels_a.iter().zip(pixels_b).map(|(a, b)| diff_archmage(token, a, b)).sum()
    }

    /// Per-call dispatch (current library behavior)
    #[inline(never)]
    pub fn diff_batch_percall(pixels_a: &[[f32; 4]], pixels_b: &[[f32; 4]]) -> f32 {
        pixels_a.iter().zip(pixels_b).map(|(a, b)| {
            if let Some(token) = Desktop64::summon() {
                diff_archmage(token, a, b)
            } else {
                super::diff_scalar(a, b)
            }
        }).sum()
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod archmage_impl {
    pub fn diff_batch_archmage(pixels_a: &[[f32; 4]], pixels_b: &[[f32; 4]]) -> f32 {
        pixels_a.iter().zip(pixels_b).map(|(a, b)| super::diff_scalar(a, b)).sum()
    }
    pub fn diff_batch_percall(pixels_a: &[[f32; 4]], pixels_b: &[[f32; 4]]) -> f32 {
        pixels_a.iter().zip(pixels_b).map(|(a, b)| super::diff_scalar(a, b)).sum()
    }
}

/// Batch processing with wide
#[inline(never)]
fn diff_batch_wide(pixels_a: &[[f32; 4]], pixels_b: &[[f32; 4]]) -> f32 {
    pixels_a.iter().zip(pixels_b).map(|(a, b)| diff_wide(a, b)).sum()
}

/// Batch processing with scalar
#[inline(never)]
fn diff_batch_scalar(pixels_a: &[[f32; 4]], pixels_b: &[[f32; 4]]) -> f32 {
    pixels_a.iter().zip(pixels_b).map(|(a, b)| diff_scalar(a, b)).sum()
}

fn bench_diff_implementations(c: &mut Criterion) {
    let mut group = c.benchmark_group("f_pixel_diff");

    for n in [1000, 10000, 100000] {
        let (pixels_a, pixels_b) = generate_pixels(n);

        group.throughput(Throughput::Elements(n as u64));

        group.bench_function(format!("scalar/{n}"), |b| {
            b.iter(|| diff_batch_scalar(black_box(&pixels_a), black_box(&pixels_b)))
        });

        group.bench_function(format!("wide/{n}"), |b| {
            b.iter(|| diff_batch_wide(black_box(&pixels_a), black_box(&pixels_b)))
        });

        group.bench_function(format!("archmage_batch/{n}"), |b| {
            b.iter(|| archmage_impl::diff_batch_archmage(black_box(&pixels_a), black_box(&pixels_b)))
        });

        group.bench_function(format!("archmage_percall/{n}"), |b| {
            b.iter(|| archmage_impl::diff_batch_percall(black_box(&pixels_a), black_box(&pixels_b)))
        });

        #[cfg(target_arch = "x86_64")]
        group.bench_function(format!("raw_intrinsics/{n}"), |b| {
            b.iter(|| raw_impl::diff_batch_raw(black_box(&pixels_a), black_box(&pixels_b)))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_diff_implementations);
criterion_main!(benches);

/// Raw intrinsics with #[arcane] - no magetypes
#[cfg(target_arch = "x86_64")]
mod raw_impl {
    use archmage::{arcane, Desktop64, SimdToken};
    use core::arch::x86_64::*;

    #[arcane]
    #[inline(always)]
    pub fn diff_raw(_token: Desktop64, a: &[f32; 4], b: &[f32; 4]) -> f32 {
        let pa = _mm_loadu_ps(a.as_ptr());
        let pb = _mm_loadu_ps(b.as_ptr());
        let alpha_diff = _mm_set1_ps(b[0] - a[0]);
        let onblack = _mm_sub_ps(pa, pb);
        let onwhite = _mm_add_ps(onblack, alpha_diff);
        let sq_black = _mm_mul_ps(onblack, onblack);
        let sq_white = _mm_mul_ps(onwhite, onwhite);
        let max_sq = _mm_max_ps(sq_black, sq_white);
        let arr: [f32; 4] = core::mem::transmute(max_sq);
        arr[1] + arr[2] + arr[3]
    }

    #[inline(never)]
    pub fn diff_batch_raw(pixels_a: &[[f32; 4]], pixels_b: &[[f32; 4]]) -> f32 {
        let Some(token) = Desktop64::summon() else {
            return pixels_a.iter().zip(pixels_b).map(|(a, b)| super::diff_scalar(a, b)).sum();
        };
        pixels_a.iter().zip(pixels_b).map(|(a, b)| diff_raw(token, a, b)).sum()
    }
}
