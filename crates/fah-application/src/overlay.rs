use image::{Rgba, RgbaImage};
use serde_json::Value;
pub fn overlay(image: &RgbaImage, result: &Value) -> RgbaImage {
    let mut image = image.clone();
    let colors = [
        [255, 200, 87, 255],
        [255, 107, 107, 255],
        [106, 227, 182, 255],
        [98, 182, 255, 255],
        [206, 142, 255, 255],
    ];
    let fingers = [
        [0, 1, 2, 3, 4],
        [0, 5, 6, 7, 8],
        [0, 9, 10, 11, 12],
        [0, 13, 14, 15, 16],
        [0, 17, 18, 19, 20],
    ];
    if let Some(hands) = result["hands"].as_array() {
        for hand in hands {
            let Some(joints) = hand["joints2d"].as_array() else {
                continue;
            };
            let points: Vec<_> = joints
                .iter()
                .map(|p| p[0].as_f64().zip(p[1].as_f64()))
                .collect();
            if points.len() != 21 {
                continue;
            }
            for (finger, color) in fingers.iter().zip(colors) {
                for edge in finger.windows(2) {
                    if let (Some(a), Some(b)) = (points[edge[0]], points[edge[1]]) {
                        line(&mut image, a, b, Rgba(color));
                    }
                }
            }
            for p in points.into_iter().flatten() {
                dot(
                    &mut image,
                    p.0 as i32,
                    p.1 as i32,
                    Rgba([255, 255, 255, 255]),
                );
            }
        }
    }
    image
}
fn dot(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    for dy in -2..=2 {
        for dx in -2..=2 {
            if dx * dx + dy * dy <= 5
                && x + dx >= 0
                && y + dy >= 0
                && x + dx < image.width() as i32
                && y + dy < image.height() as i32
            {
                image.put_pixel((x + dx) as u32, (y + dy) as u32, color);
            }
        }
    }
}
fn line(image: &mut RgbaImage, a: (f64, f64), b: (f64, f64), color: Rgba<u8>) {
    if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
        return;
    }
    // Clip first so predictions outside the crop cannot create unbounded drawing work.
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut lo, mut hi) = (0f64, 1f64);
    for (p, q) in [
        (-dx, a.0),
        (dx, image.width() as f64 - 1. - a.0),
        (-dy, a.1),
        (dy, image.height() as f64 - 1. - a.1),
    ] {
        if p == 0. {
            if q < 0. {
                return;
            }
        } else {
            let r = q / p;
            if p < 0. {
                lo = lo.max(r);
            } else {
                hi = hi.min(r);
            }
        }
    }
    if lo > hi {
        return;
    }
    let n = ((dx.abs().max(dy.abs()) * (hi - lo)).ceil() as usize).max(1);
    for i in 0..=n {
        let t = lo + (hi - lo) * i as f64 / n as f64;
        dot(image, (a.0 + dx * t) as i32, (a.1 + dy * t) as i32, color);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clips_outside_joints() {
        let mut image = RgbaImage::new(16, 16);
        line(&mut image, (-1e12, 8.), (1e12, 8.), Rgba([255; 4]));
        assert_eq!(image.get_pixel(8, 8).0, [255; 4]);
        line(&mut image, (-100., -100.), (-20., -20.), Rgba([0; 4]));
        assert_eq!(image.get_pixel(8, 8).0, [255; 4]);
    }
}
