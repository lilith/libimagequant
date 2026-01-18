use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use imagequant::_bench::{liq_max3, liq_max3_scalar_ref, liq_min3, liq_min3_scalar_ref};
use imagequant::*;

fn generate_test_image(width: usize, height: usize) -> Vec<RGBA> {
    (0..width * height)
        .map(|i| {
            RGBA::new(
                (i % 256) as u8,
                ((i * 7) % 256) as u8,
                ((i * 13) % 256) as u8,
                255,
            )
        })
        .collect()
}

fn bench_quantize(c: &mut Criterion) {
    let mut group = c.benchmark_group("quantize");

    for (width, height) in [(256, 256), (512, 512), (1024, 1024)] {
        let pixels = generate_test_image(width, height);
        let size = width * height;

        group.throughput(Throughput::Elements(size as u64));
        group.bench_function(format!("{width}x{height}"), |b| {
            b.iter(|| {
                let mut attr = Attributes::new();
                attr.set_speed(5).unwrap();
                let mut img = attr
                    .new_image(black_box(&pixels[..]), width, height, 0.0)
                    .unwrap();
                attr.quantize(black_box(&mut img)).unwrap()
            })
        });
    }

    group.finish();
}

fn bench_quantize_speed_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("quantize_speed");

    let width = 512;
    let height = 512;
    let pixels = generate_test_image(width, height);

    for speed in [1, 3, 5, 8, 10] {
        group.bench_function(format!("speed_{speed}"), |b| {
            b.iter(|| {
                let mut attr = Attributes::new();
                attr.set_speed(speed).unwrap();
                let mut img = attr
                    .new_image(black_box(&pixels[..]), width, height, 0.0)
                    .unwrap();
                attr.quantize(black_box(&mut img)).unwrap()
            })
        });
    }

    group.finish();
}

fn bench_remap(c: &mut Criterion) {
    let mut group = c.benchmark_group("remap");

    let width = 512;
    let height = 512;
    let pixels = generate_test_image(width, height);

    // Prepare quantized result
    let mut attr = Attributes::new();
    attr.set_speed(10).unwrap();
    let mut img = attr.new_image(&pixels[..], width, height, 0.0).unwrap();
    let mut res = attr.quantize(&mut img).unwrap();

    group.throughput(Throughput::Elements((width * height) as u64));

    group.bench_function("no_dither", |b| {
        res.set_dithering_level(0.0).unwrap();
        let mut img = attr.new_image(&pixels[..], width, height, 0.0).unwrap();
        b.iter(|| res.remapped(black_box(&mut img)).unwrap())
    });

    group.bench_function("dither_1.0", |b| {
        res.set_dithering_level(1.0).unwrap();
        let mut img = attr.new_image(&pixels[..], width, height, 0.0).unwrap();
        b.iter(|| res.remapped(black_box(&mut img)).unwrap())
    });

    group.finish();
}

fn bench_histogram(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram");

    for (width, height) in [(256, 256), (512, 512), (1024, 1024)] {
        let pixels = generate_test_image(width, height);
        let size = width * height;

        group.throughput(Throughput::Elements(size as u64));
        group.bench_function(format!("{width}x{height}"), |b| {
            b.iter(|| {
                let attr = Attributes::new();
                let mut img = attr
                    .new_image(black_box(&pixels[..]), width, height, 0.0)
                    .unwrap();
                let mut hist = Histogram::new(&attr);
                hist.add_image(&attr, black_box(&mut img)).unwrap();
                hist
            })
        });
    }

    group.finish();
}

fn bench_blur_simd_vs_scalar(c: &mut Criterion) {
    let mut group = c.benchmark_group("blur_max3");

    for size in [256, 512, 1024] {
        let src: Vec<u8> = (0..size * size)
            .map(|i| ((i * 17 + i / 3) % 256) as u8)
            .collect();
        let mut dst = vec![0u8; size * size];

        group.throughput(Throughput::Elements((size * size) as u64));

        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |b, &size| {
            b.iter(|| {
                liq_max3_scalar_ref(black_box(&src), black_box(&mut dst), size, size);
            })
        });

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, &size| {
            b.iter(|| {
                liq_max3(black_box(&src), black_box(&mut dst), size, size);
            })
        });
    }

    group.finish();

    let mut group = c.benchmark_group("blur_min3");

    for size in [256, 512, 1024] {
        let src: Vec<u8> = (0..size * size)
            .map(|i| ((i * 17 + i / 3) % 256) as u8)
            .collect();
        let mut dst = vec![0u8; size * size];

        group.throughput(Throughput::Elements((size * size) as u64));

        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |b, &size| {
            b.iter(|| {
                liq_min3_scalar_ref(black_box(&src), black_box(&mut dst), size, size);
            })
        });

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, &size| {
            b.iter(|| {
                liq_min3(black_box(&src), black_box(&mut dst), size, size);
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_quantize,
    bench_quantize_speed_levels,
    bench_remap,
    bench_histogram,
    bench_blur_simd_vs_scalar
);
criterion_main!(benches);
