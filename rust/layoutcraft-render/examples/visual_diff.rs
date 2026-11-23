//! Compare a reference PNG (Visio's `Page.Export`) with our render using the
//! conformance metric: both fitted on a 640x480 white canvas, per-pixel delta 48.
//!
//! usage: visual_diff <reference.png> <candidate.png> [side-by-side.png] [--trim]
//!
//! `--trim` crops near-white borders from both images first, so a render that is
//! cropped to its content compares fairly with one that has page margins.
//! prints: mismatch<TAB>mean_error
use image::{imageops, Rgb, RgbImage};

fn load(path: &str) -> RgbImage {
    let rgba = image::open(path).unwrap_or_else(|e| panic!("{path}: {e}")).to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let b = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, Rgb([b(p[0]), b(p[1]), b(p[2])]));
    }
    out
}

/// Crop the near-white border (any channel above 245 counts as white). An
/// all-white image is returned unchanged.
fn trim_white(image: &RgbImage) -> RgbImage {
    let (w, h) = (image.width(), image.height());
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (x, y, p) in image.enumerate_pixels() {
        if p[0] < 246 || p[1] < 246 || p[2] < 246 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 {
        return image.clone();
    }
    imageops::crop_imm(image, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

fn fit(image: &RgbImage, w: u32, h: u32) -> RgbImage {
    let s = (w as f64 / image.width() as f64).min(h as f64 / image.height() as f64);
    let nw = ((image.width() as f64 * s).round() as u32).clamp(1, w);
    let nh = ((image.height() as f64 * s).round() as u32).clamp(1, h);
    let resized = imageops::resize(image, nw, nh, imageops::FilterType::Triangle);
    let mut canvas = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
    imageops::overlay(&mut canvas, &resized, ((w - nw) / 2) as i64, ((h - nh) / 2) as i64);
    canvas
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: visual_diff <reference.png> <candidate.png> [side-by-side.png]");
        std::process::exit(2);
    }
    let (w, h) = (640u32, 480u32);
    let trim = args.iter().any(|a| a == "--trim");
    let prep = |path: &str| {
        let image = load(path);
        if trim { trim_white(&image) } else { image }
    };
    let reference = fit(&prep(&args[1]), w, h);
    let candidate = fit(&prep(&args[2]), w, h);
    let (mut bad, mut sum) = (0u64, 0u64);
    for (r, c) in reference.pixels().zip(candidate.pixels()) {
        let mut max = 0u8;
        for ch in 0..3 {
            let d = r[ch].abs_diff(c[ch]);
            max = max.max(d);
            sum += d as u64;
        }
        if max > 48 {
            bad += 1;
        }
    }
    let pixels = (w * h) as f64;
    println!("{:.4}\t{:.4}", bad as f64 / pixels, sum as f64 / (pixels * 3.0 * 255.0));
    if let Some(out) = args.get(3).filter(|a| a.as_str() != "--trim") {
        let mut sheet = RgbImage::from_pixel(w * 2 + 8, h, Rgb([200, 200, 200]));
        imageops::overlay(&mut sheet, &reference, 0, 0);
        imageops::overlay(&mut sheet, &candidate, (w + 8) as i64, 0);
        sheet.save(out).unwrap();
    }
}
