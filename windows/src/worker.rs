use crate::media::{Frame, Input, Source, validate_models};
use crate::onnx::{self, Backend};
use fah_rust::pipeline::Pipeline;
use image::{Rgba, RgbaImage};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

pub struct Request {
    pub id: u64,
    pub models: PathBuf,
    pub backend: Backend,
    pub source: Source,
}
pub enum Command {
    Start(Request),
    Stop,
    Reset,
    RefreshCameras,
    Shutdown,
}
pub enum Event {
    Loading(u64),
    Ready(u64, String, String),
    Finished(u64),
    Error(u64, String),
    Cameras(Result<Vec<String>, String>),
}
pub struct Output {
    pub id: u64,
    pub image: RgbaImage,
    pub result: Value,
    pub latency_ms: f64,
}
pub struct Worker {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
    pub frame: Arc<Mutex<Option<Output>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new() -> Self {
        let (commands, rx) = mpsc::channel();
        let (tx, events) = mpsc::channel();
        let frame = Arc::new(Mutex::new(None));
        let slot = frame.clone();
        let thread = thread::spawn(move || run(rx, tx, slot));
        Self {
            commands,
            events,
            frame,
            thread: Some(thread),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn run(commands: Receiver<Command>, events: Sender<Event>, frame: Arc<Mutex<Option<Output>>>) {
    let mut pipeline: Option<Pipeline> = None;
    let mut loaded: Option<(PathBuf, Backend)> = None;
    let mut input: Option<Input> = None;
    let mut id = 0;
    let mut active_backend = Backend::Cpu;
    let mut warning = String::new();
    let mut reset = true;
    let mut last_frame = Instant::now();
    loop {
        match commands.recv_timeout(Duration::from_millis(5)) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Stop) => {
                input = None;
                *frame.lock().unwrap() = None;
            }
            Ok(Command::Reset) => reset = true,
            Ok(Command::RefreshCameras) => {
                let _ = events.send(Event::Cameras(
                    crate::media::cameras().map_err(|e| format!("{e:#}")),
                ));
            }
            Ok(Command::Start(request)) => {
                input = None;
                id = request.id;
                reset = true;
                *frame.lock().unwrap() = None;
                let _ = events.send(Event::Loading(id));
                let result = (|| -> anyhow::Result<Input> {
                    validate_models(&request.models, request.backend)?;
                    let key = (request.models.clone(), request.backend);
                    if loaded.as_ref() != Some(&key) {
                        pipeline = None;
                        loaded = None;
                        let (new_pipeline, selected, notice) =
                            onnx::create(&request.models, request.backend)?;
                        pipeline = Some(new_pipeline);
                        active_backend = selected;
                        warning = notice;
                        loaded = Some(key);
                    }
                    Input::open(&request.source)
                })();
                match result {
                    Ok(source) => {
                        input = Some(source);
                        last_frame = Instant::now();
                        let _ = events.send(Event::Ready(
                            id,
                            format!(
                                "{} / {}",
                                if active_backend == Backend::Vulkan {
                                    "ncnn"
                                } else {
                                    "ONNX"
                                },
                                active_backend.name()
                            ),
                            warning.clone(),
                        ));
                    }
                    Err(error) => {
                        let _ = events.send(Event::Error(id, format!("{error:#}")));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let Some(source) = input.as_mut() else {
            continue;
        };
        let result = (|| -> anyhow::Result<bool> {
            if let Some(Frame { image, captured }) = source.next()? {
                let json = pipeline
                    .as_mut()
                    .unwrap()
                    .process(
                        image.as_raw(),
                        image.width() as usize,
                        image.height() as usize,
                        image.width() as usize * 4,
                        reset,
                    )
                    .map_err(anyhow::Error::msg)?;
                let mut result: Value = serde_json::from_str(&json)?;
                if result["cpu_fallback"] != true {
                    result["backend"] = active_backend.name().into();
                }
                result["requested_backend"] = loaded.as_ref().unwrap().1.name().into();
                result["initialization_warning"] = warning.clone().into();
                result["initialization_fallback"] = (!warning.is_empty()).into();
                *frame.lock().unwrap() = Some(Output {
                    id,
                    image,
                    result,
                    latency_ms: captured.elapsed().as_secs_f64() * 1000.,
                });
                reset = false;
                last_frame = Instant::now();
            } else if last_frame.elapsed() > Duration::from_secs(20) {
                anyhow::bail!(
                    "入力フレームを取得できません。カメラの接続と他アプリでの使用状況を確認してください。"
                );
            }
            source.finished()
        })();
        match result {
            Ok(true) => {
                input = None;
                let _ = events.send(Event::Finished(id));
            }
            Ok(false) => {}
            Err(error) => {
                input = None;
                let _ = events.send(Event::Error(id, format!("{error:#}")));
            }
        }
    }
}
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
