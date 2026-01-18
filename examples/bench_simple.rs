use std::time::Instant;

fn main() {
    let width = 512;
    let height = 512;
    let mut pixels: Vec<imagequant::RGBA> = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            pixels.push(imagequant::RGBA::new(
                (x * 255 / width) as u8,
                (y * 255 / height) as u8,
                ((x + y) * 128 / (width + height)) as u8,
                255,
            ));
        }
    }

    let mut attr = imagequant::new();
    attr.set_speed(5).unwrap();

    // Warmup
    for _ in 0..5 {
        let mut img = attr.new_image(&pixels[..], width, height, 0.0).unwrap();
        let mut res = attr.quantize(&mut img).unwrap();
        res.set_dithering_level(1.0).unwrap();
        let _ = res.remapped(&mut img).unwrap();
    }

    let iterations = 30;

    let start = Instant::now();
    for _ in 0..iterations {
        let mut img = attr.new_image(&pixels[..], width, height, 0.0).unwrap();
        let _ = attr.quantize(&mut img).unwrap();
    }
    println!("quantize/512x512: {:?}", start.elapsed() / iterations);

    let mut img = attr.new_image(&pixels[..], width, height, 0.0).unwrap();
    let mut res = attr.quantize(&mut img).unwrap();
    res.set_dithering_level(1.0).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = res.remapped(&mut img).unwrap();
    }
    println!("remap/dither_1.0: {:?}", start.elapsed() / iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let mut hist = imagequant::Histogram::new(&attr);
        let mut img = attr.new_image(&pixels[..], width, height, 0.0).unwrap();
        hist.add_image(&attr, &mut img).unwrap();
    }
    println!("histogram/512x512: {:?}", start.elapsed() / iterations);
}
