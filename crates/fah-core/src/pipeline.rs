use crate::{
    read_floats,
    runtime::{Inference, Tensor},
    write_floats, Result,
};
use serde_json::{json, Value};
use std::time::Instant;
#[derive(Clone, Copy, Debug)]
struct Box2 {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    score: f32,
    right: bool,
}
fn iou(a: Box2, b: Box2) -> f32 {
    let area =
        (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.) * (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.);
    area / ((a.x1 - a.x0) * (a.y1 - a.y0) + (b.x1 - b.x0) * (b.y1 - b.y0) - area).max(1.)
}
fn elapsed(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.
}
struct Image<'a> {
    data: &'a [u8],
    width: usize,
    height: usize,
    stride: usize,
}
impl Image<'_> {
    fn warp(&self, size: usize, sx: f32, ox: f32, sy: f32, oy: f32, border: f32) -> Tensor {
        let mut data = vec![0.; size * size * 3];
        for y in 0..size {
            for x in 0..size {
                let px = sx * x as f32 + ox;
                let py = sy * y as f32 + oy;
                let ix = px.floor() as i32;
                let iy = py.floor() as i32;
                let dx = px - ix as f32;
                let dy = py - iy as f32;
                for c in 0..3 {
                    let mut value = 0.;
                    for j in 0..2 {
                        for k in 0..2 {
                            let xx = ix + k;
                            let yy = iy + j;
                            let sample = if xx >= 0
                                && xx < self.width as i32
                                && yy >= 0
                                && yy < self.height as i32
                            {
                                self.data[yy as usize * self.stride + xx as usize * 4 + c] as f32
                            } else {
                                border
                            };
                            value += sample
                                * (if k == 1 { dx } else { 1. - dx })
                                * (if j == 1 { dy } else { 1. - dy });
                        }
                    }
                    data[c * size * size + y * size + x] = value / 255.;
                }
            }
        }
        Tensor::new(size, size, 3, data)
    }
}
type Factory = Box<dyn Fn(bool, bool) -> Result<Box<dyn Inference + Send>> + Send>;
pub struct Pipeline {
    runtime: Box<dyn Inference + Send>,
    factory: Factory,
    directory: String,
    gpu: bool,
    gpu_backend: &'static str,
    fallback: bool,
    grid: Vec<f32>,
    previous: Vec<Box2>,
    frame: u32,
    width: usize,
    height: usize,
}
impl Pipeline {
    pub fn new(directory: &str, gpu: bool, half: bool, factory: Factory) -> Result<Self> {
        let runtime = factory(gpu, half)?;
        let grid = read_floats(format!("{directory}/uv_grid.f32"))?;
        if grid.len() != 800 || grid.iter().any(|v| !v.is_finite()) {
            return Err("Invalid camera grid".into());
        }
        Ok(Self {
            runtime,
            factory,
            directory: directory.into(),
            gpu,
            gpu_backend: "gpu",
            fallback: false,
            grid,
            previous: vec![],
            frame: 0,
            width: 0,
            height: 0,
        })
    }
    pub fn with_gpu_backend(mut self, backend: &'static str) -> Self {
        self.gpu_backend = backend;
        self
    }
    pub fn process(
        &mut self,
        rgba: &[u8],
        width: usize,
        height: usize,
        stride: usize,
        reset: bool,
    ) -> Result<String> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || stride < width * 4
            || stride.checked_mul(height).is_none_or(|n| rgba.len() < n)
        {
            return Err("Invalid RGBA dimensions or stride".into());
        }
        let image = Image {
            data: rgba,
            width,
            height,
            stride,
        };
        let start = Instant::now();
        match self.process_frame(&image, reset, 0.) {
            Ok(value) => Ok(value.to_string()),
            Err(_) if self.gpu => {
                self.runtime = (self.factory)(false, false)?;
                self.gpu = false;
                self.fallback = true;
                self.process_frame(&image, true, elapsed(start))
                    .map(|v| v.to_string())
            }
            Err(e) => Err(e),
        }
    }
    fn process_frame(&mut self, image: &Image, reset: bool, recovery: f64) -> Result<Value> {
        let start = Instant::now();
        let (width, height) = (image.width, image.height);
        if reset || width != self.width || height != self.height {
            self.previous.clear();
            self.frame = 0;
        }
        self.width = width;
        self.height = height;
        let detection = self.previous.is_empty() || self.frame.is_multiple_of(10);
        self.frame = self.frame.wrapping_add(1);
        let (mut det_ms, mut pose_ms) = (0., 0.);
        let boxes = if detection {
            let t = Instant::now();
            let scale = 640. / width.max(height) as f32;
            let pad_x = (640 - (width as f32 * scale).round() as i32) / 2;
            let pad_y = (640 - (height as f32 * scale).round() as i32) / 2;
            let input = image.warp(
                640,
                1. / scale,
                (0.5 - pad_x as f32) / scale - 0.5,
                1. / scale,
                (0.5 - pad_y as f32) / scale - 0.5,
                114.,
            );
            let out = self.runtime.detect(&input)?;
            if out.h != 6 || out.c != 1 || out.data.len() != out.w * 6 {
                return Err("Unexpected detector output shape".into());
            }
            let mut candidates = vec![];
            for i in 0..out.w {
                let at = |r| out.data[r * out.w + i];
                let (left, right) = (at(4), at(5));
                let score = left.max(right);
                if ![at(0), at(1), at(2), at(3), left, right]
                    .iter()
                    .all(|v| v.is_finite())
                {
                    return Err("Non-finite detector result".into());
                }
                if score < 0.05 {
                    continue;
                }
                let cx = (at(0) - pad_x as f32) / scale;
                let cy = (at(1) - pad_y as f32) / scale;
                let bw = at(2) / scale;
                let bh = at(3) / scale;
                let b = Box2 {
                    x0: (cx - bw * 0.5).clamp(0., (width - 1) as f32),
                    y0: (cy - bh * 0.5).clamp(0., (height - 1) as f32),
                    x1: (cx + bw * 0.5).clamp(0., (width - 1) as f32),
                    y1: (cy + bh * 0.5).clamp(0., (height - 1) as f32),
                    score,
                    right: right > left,
                };
                if b.x1 - b.x0 > 4. && b.y1 - b.y0 > 4. {
                    candidates.push(b);
                }
            }
            candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
            let mut boxes = vec![];
            for b in candidates {
                if !boxes.iter().any(|&kept| iou(b, kept) > 0.45) {
                    boxes.push(b);
                }
                if boxes.len() == 2 {
                    break;
                }
            }
            det_ms = elapsed(t);
            boxes
        } else {
            self.previous.clone()
        };
        self.previous.clear();
        let mut hands = vec![];
        // Intrinsics are estimated until calibrated camera intrinsics are supplied.
        let focal = ((width * width + height * height) as f32).sqrt();
        let (kx, ky) = (width as f32 * 0.5, height as f32 * 0.5);
        for b in boxes {
            let t = Instant::now();
            let (cx, cy) = ((b.x0 + b.x1) * 0.5, (b.y0 + b.y1) * 0.5);
            let (mut bw, bh) = (b.x1 - b.x0, b.y1 - b.y0);
            if bh > bw {
                bw = bw.max(1.1 * bh / (4. / 3. * 0.9));
            }
            let size = (bw * 0.9 / 0.75).max(bh * 0.9).max(8.);
            let cx_mirror = if b.right { cx } else { width as f32 - 1. - cx };
            let kx_mirror = if b.right { kx } else { width as f32 - 1. - kx };
            let (left, top, step) = (cx_mirror - size * 0.5, cy - size * 0.5, size / 320.);
            let crop = image.warp(
                320,
                if b.right { step } else { -step },
                if b.right {
                    left
                } else {
                    width as f32 - 1. - left
                },
                step,
                top,
                0.,
            );
            let mut rays = Tensor::new(99, 400, 1, vec![0.; 99 * 400]);
            for p in 0..400 {
                let pos = [
                    (self.grid[2 * p] * step + left - kx_mirror) / focal,
                    (self.grid[2 * p + 1] * step + top - ky) / focal,
                    1.,
                ];
                let row = &mut rays.data[p * 99..(p + 1) * 99];
                for c in 0..3 {
                    row[c] = pos[c];
                    for j in 0..16 {
                        let phase = pos[c] * (1. + 31. * j as f32 / 15.) * std::f32::consts::PI;
                        row[3 + c * 16 + j] = phase.sin();
                        row[51 + c * 16 + j] = phase.cos();
                    }
                }
            }
            let condition = Tensor::new(
                3,
                1,
                1,
                vec![
                    (cx_mirror - kx_mirror) / focal,
                    (cy - ky) / focal,
                    size / focal,
                ],
            );
            let out = self.runtime.pose(&crop, &rays, &condition)?;
            pose_ms += elapsed(t);
            if out.len() != 6
                || out[0].w != 2
                || out[0].h != 461
                || out[1].w != 3
                || out[1].h != 461
                || out[4].data.is_empty()
                || out[5].data.len() < 2
            {
                return Err("Unexpected pose output shape".into());
            }
            if out.iter().flat_map(|m| &m.data).any(|v| !v.is_finite()) {
                write_floats(format!("{}/failure-img.f32", self.directory), &crop.data)?;
                write_floats(format!("{}/failure-rays.f32", self.directory), &rays.data)?;
                write_floats(
                    format!("{}/failure-cond.f32", self.directory),
                    &condition.data,
                )?;
                return Err("Non-finite pose result".into());
            }
            let sigmoid = |x: f32| 1. / (1. + (-x).exp());
            let (presence, side) = (sigmoid(out[5].data[0]), sigmoid(out[5].data[1]));
            let score = (b.score * presence).sqrt();
            if presence < 0.5 || score < 0.15 {
                continue;
            }
            let mut uv = vec![[0.; 2]; 461];
            let mut xyz = vec![[0.; 3]; 461];
            let tz = out[4].data[0].exp() * focal / size;
            let (mut tx, mut ty) = (0., 0.);
            for p in 0..461 {
                let x = out[0].data[p * 2] * size + left;
                uv[p] = [
                    if b.right { x } else { width as f32 - 1. - x },
                    out[0].data[p * 2 + 1] * size + top,
                ];
                xyz[p] = [
                    out[1].data[p * 3] * if b.right { 1. } else { -1. },
                    out[1].data[p * 3 + 1],
                    out[1].data[p * 3 + 2],
                ];
                tx += (uv[p][0] - kx) * (xyz[p][2] + tz) / focal - xyz[p][0];
                ty += (uv[p][1] - ky) * (xyz[p][2] + tz) / focal - xyz[p][1];
            }
            tx /= 461.;
            ty /= 461.;
            let mut next = Box2 {
                x0: width as f32,
                y0: height as f32,
                x1: 0.,
                y1: 0.,
                ..b
            };
            for p in &uv {
                next.x0 = next.x0.min(p[0]);
                next.y0 = next.y0.min(p[1]);
                next.x1 = next.x1.max(p[0]);
                next.y1 = next.y1.max(p[1]);
            }
            next.x0 = next.x0.clamp(0., (width - 1) as f32);
            next.x1 = next.x1.clamp(0., (width - 1) as f32);
            next.y0 = next.y0.clamp(0., (height - 1) as f32);
            next.y1 = next.y1.clamp(0., (height - 1) as f32);
            if next.x1 - next.x0 > 4. && next.y1 - next.y0 > 4. {
                self.previous.push(next);
            }
            let joints3d: Vec<_> = xyz[..21]
                .iter()
                .map(|p| [p[0] + tx, p[1] + ty, p[2] + tz])
                .collect();
            if uv
                .iter()
                .flatten()
                .chain(joints3d.iter().flatten())
                .any(|v| !v.is_finite())
            {
                return Err("Non-finite projected pose".into());
            }
            hands.push(json!({"right":b.right,"score":score,"presence":presence,"side":side,"joints2d":uv[..21],"joints3d":joints3d}));
        }
        Ok(
            json!({"backend":if self.gpu {self.gpu_backend} else {"cpu"},"cpu_fallback":self.fallback,"width":width,"height":height,"frame":self.frame,"detected":detection,"detector_ms":det_ms,"pose_ms":pose_ms,"recovery_ms":recovery,"total_ms":elapsed(start)+recovery,"hands":hands}),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Fake {
        fail: bool,
        calls: Arc<AtomicUsize>,
    }
    impl Inference for Fake {
        fn detect(&mut self, _: &Tensor) -> Result<Tensor> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("GPU failure".into());
            }
            Ok(Tensor::new(1, 6, 1, vec![320., 320., 200., 200., 0.1, 0.9]))
        }
        fn pose(&mut self, _: &Tensor, rays: &Tensor, condition: &Tensor) -> Result<Vec<Tensor>> {
            assert!(rays.data.iter().all(|v| v.is_finite()));
            assert!(condition.data[2] > 0.);
            let uv = (0..461)
                .flat_map(|p| [0.1 + (p % 2) as f32 * 0.8, 0.1 + (p % 3) as f32 * 0.4])
                .collect();
            Ok(vec![
                Tensor::new(2, 461, 1, uv),
                Tensor::new(3, 461, 1, vec![0.; 1383]),
                Tensor::new(1, 1, 1, vec![0.]),
                Tensor::new(1, 1, 1, vec![0.]),
                Tensor::new(1, 1, 1, vec![0.]),
                Tensor::new(2, 1, 1, vec![10., 10.]),
            ])
        }
    }
    fn pipeline(gpu: bool) -> (Pipeline, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let factory_calls = calls.clone();
        (
            Pipeline {
                runtime: Box::new(Fake {
                    fail: gpu,
                    calls: calls.clone(),
                }),
                factory: Box::new(move |gpu, half| {
                    assert!(!gpu && !half);
                    Ok(Box::new(Fake {
                        fail: false,
                        calls: factory_calls.clone(),
                    }))
                }),
                directory: String::new(),
                gpu,
                gpu_backend: "gpu",
                fallback: false,
                grid: vec![160.; 800],
                previous: vec![],
                frame: 0,
                width: 0,
                height: 0,
            },
            calls,
        )
    }
    fn frame(p: &mut Pipeline, reset: bool) -> Value {
        serde_json::from_str(
            &p.process(&vec![128; 64 * 64 * 4], 64, 64, 256, reset)
                .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn warp_handles_stride_borders_and_mirroring() {
        let image = Image {
            data: &[
                255, 0, 0, 255, 0, 255, 0, 255, 99, 99, 99, 99, 0, 0, 255, 255, 255, 255, 255, 255,
                99, 99, 99, 99,
            ],
            width: 2,
            height: 2,
            stride: 12,
        };
        let out = image.warp(2, 1., 0., 1., 0., 0.);
        assert_eq!(
            out.data,
            vec![1., 0., 0., 1., 0., 1., 0., 1., 0., 0., 1., 1.]
        );
        assert_eq!(
            image.warp(2, -1., 1., 1., 0., 0.).data,
            vec![0., 1., 1., 0., 1., 0., 1., 0., 0., 0., 1., 1.]
        );
        let average = image.warp(1, 1., 0.5, 1., 0.5, 0.);
        assert_eq!(average.data, vec![0.5; 3]);
        assert_eq!(
            image.warp(1, 1., -2., 1., 0., 114.).data,
            vec![114. / 255.; 3]
        );
    }
    #[test]
    fn tracking_redetects_every_ten_frames_and_resets() {
        let (mut pipeline, calls) = pipeline(false);
        for i in 1..=12 {
            let result = frame(&mut pipeline, false);
            assert_eq!(result["frame"], i);
            assert_eq!(result["detected"], i == 1 || i == 11);
            assert_eq!(result["hands"].as_array().unwrap().len(), 1);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(frame(&mut pipeline, true)["frame"], 1);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let changed: Value = serde_json::from_str(
            &pipeline
                .process(&vec![0; 32 * 64 * 4], 32, 64, 128, false)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(changed["frame"], 1);
        assert_eq!(changed["detected"], true);
    }
    #[test]
    fn gpu_failure_retries_once_and_keeps_cpu() {
        let (mut pipeline, calls) = pipeline(true);
        let result = frame(&mut pipeline, false);
        assert_eq!(result["backend"], "cpu");
        assert_eq!(result["cpu_fallback"], true);
        assert_eq!(result["frame"], 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let result = frame(&mut pipeline, false);
        assert_eq!(result["frame"], 2);
        assert_eq!(result["cpu_fallback"], true);
        assert_eq!(result["recovery_ms"], 0.);
    }
    #[test]
    fn invalid_image_is_rejected_before_inference() {
        let (mut pipeline, calls) = pipeline(true);
        assert!(pipeline.process(&[], 64, 64, 256, false).is_err());
        assert!(pipeline.process(&[0; 4], 0, 1, 4, false).is_err());
        assert!(pipeline.process(&[0; 4], 1, 1, 3, false).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn overlap_uses_intersection_over_union() {
        let a = Box2 {
            x0: 0.,
            y0: 0.,
            x1: 10.,
            y1: 10.,
            score: 1.,
            right: true,
        };
        assert_eq!(iou(a, a), 1.);
        assert_eq!(
            iou(
                a,
                Box2 {
                    x0: 10.,
                    x1: 20.,
                    ..a
                }
            ),
            0.
        );
        assert!(
            (iou(
                a,
                Box2 {
                    x0: 5.,
                    x1: 15.,
                    ..a
                }
            ) - 1. / 3.)
                .abs()
                < 1e-6
        );
    }
}
