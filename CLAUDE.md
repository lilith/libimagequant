# libimagequant SIMD Optimization Project

Fork of ImageOptim/libimagequant exploring performance improvements with `wide` and `multiversed` crates.

## Key Finding: LLVM Auto-Vectorization

**The scalar blur code is already highly optimized by LLVM's auto-vectorizer.**

Benchmark results (1024×1024 image):
```
blur_max3/scalar:  88.9 µs  (11.8 Gelem/s)
blur_max3/simd:    818  µs  (1.28 Gelem/s)  <- 9x SLOWER
```

The naive manual SIMD implementation using `wide::u8x16` with copy-based loads was significantly slower than the scalar code. LLVM is generating excellent vectorized code for the simple min/max operations without any manual intervention.

**Lesson learned:** Don't assume manual SIMD will be faster. Always benchmark against the scalar baseline. Modern compilers are very good at auto-vectorization for simple patterns.

## Completed Work

### Phase 1: Infrastructure ✓
- Added `wide 0.8` and `multiversed 0.1` dependencies
- Set up Criterion benchmarks (`benches/simd_bench.rs`)
- Updated clippy allows for pre-existing issues

### Phase 2: Blur Operations - Reverted
- Implemented manual SIMD using `wide::u8x16`
- Benchmarked and found it was 9x slower than scalar
- Reverted to scalar (LLVM auto-vectorizes effectively)
- Added comparison benchmarks to document finding

### Phase 3: Safe SIMD and Unsafe Reduction ✓
- Replaced hand-written SSE/NEON intrinsics in `f_pixel::diff()` with safe `wide::f32x4`
  - 27% faster than original (6.05ms → 4.76ms for 512×512)
- Removed `HistSortTmp` union, replaced with plain `u32` field
- Removed `RGBAInt` union, replaced with `rgb::bytemuck::cast` for RGBA↔u32
- Net reduction of ~70 lines while improving both safety and performance

## Unsafe Code Audit

### Eliminated unsafe:
- `pal.rs`: SSE/NEON intrinsics → safe `wide::f32x4` (27% faster)
- `hist.rs`: `HistSortTmp` and `RGBAInt` unions → plain types + bytemuck
- `rows.rs`: Row access for contiguous buffers now uses safe slice indexing
- `mediancut.rs`: Removed unnecessary `get_unchecked` (3 bounds checks per partition is negligible)

### Gated behind `_internal_c_ffi` feature (C API only):
- `rows.rs:PixelsSource::RowPointers`: Raw pointer row access for C-provided images
- `rows.rs:set_memory_ownership`: C memory ownership transfer
- `seacow.rs`: C-owned memory management, raw pointer row iteration
- `image.rs:new_fn`: C callback-based image creation

### Required unsafe (idiomatic patterns):
- `rows.rs`, `remap.rs`, `quant.rs`: MaybeUninit handling for uninitialized buffers
- `seacow.rs`: `unsafe impl Send/Sync` for pointer wrappers (Rust #93367 workaround)
- `lib.rs`: FFI math functions for `no_std` mode (not available in core)

## Benchmark Comparison vs Upstream (2026-01-18)

Run on: Linux WSL2 (x86_64)
Upstream: ImageOptim/libimagequant @ 26edfc4

### bench_simple.rs (512×512, speed=5, 30 iterations)

| Operation          | Upstream  | This Fork | Notes |
|--------------------|-----------|-----------|-------|
| quantize/512x512   | ~33ms     | ~33ms     | Parity |
| remap/dither_1.0   | ~10ms     | ~10ms     | Parity |
| histogram/512x512  | ~5ms      | ~5ms      | Parity |

**Result: Performance parity with upstream while eliminating unsafe for pure Rust users.**

### Criterion Benchmarks (detailed)

```
quantize/256x256        time: 1.45 ms   (45.2 Melem/s)
quantize/512x512        time: 6.05 ms   (43.3 Melem/s)
quantize/1024x1024      time: 25.0 ms   (41.9 Melem/s)

quantize_speed/speed_1  time: 6.37 ms
quantize_speed/speed_5  time: 6.33 ms
quantize_speed/speed_10 time: 1.45 ms

remap/no_dither         time: 0.24 ms   (1086 Melem/s)
remap/dither_1.0        time: 5.44 ms   (48.2 Melem/s)

histogram/256x256       time: 1.53 ms   (43.0 Melem/s)
histogram/512x512       time: 6.68 ms   (39.2 Melem/s)
histogram/1024x1024     time: 28.0 ms   (37.4 Melem/s)

blur_max3/scalar/1024   time: 88.9 µs   (11.8 Gelem/s)
blur_min3/scalar/1024   time: 88.6 µs   (11.8 Gelem/s)
```

## Remaining Optimization Opportunities

### Completed Optimizations

The `f_pixel::diff()` function in `pal.rs` was rewritten to use safe `wide::f32x4` instead of hand-written SSE/NEON intrinsics, resulting in 27% faster performance.

### Potential Future Targets

1. **Histogram hash operations** - complex enough that LLVM may not vectorize
2. **Error diffusion in dithering** - sequential dependencies limit auto-vectorization
3. **K-means color averaging** - complex weighted sums

### Not Good Candidates

- Simple min/max/add operations on u8 arrays (LLVM handles these well)
- Operations with complex control flow
- Small fixed-size operations (overhead dominates)

## Commands

```bash
# Run tests
cargo test

# Quick benchmark comparison (for comparing against upstream)
cargo run --release --example bench_simple

# Run Criterion benchmarks
cargo bench --bench simd_bench

# Run blur comparison specifically
cargo bench --bench simd_bench -- blur

# Check clippy (lib and tests only)
cargo clippy --lib --tests -- -D warnings
```

## Known Issues

(None)

## FEEDBACK.md

See [FEEDBACK.md](./FEEDBACK.md) for user feedback log.
