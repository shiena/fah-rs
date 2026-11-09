use image::RgbaImage;
use serde_json::Value;
#[derive(Clone)]
pub struct Output {
    pub id: u64,
    pub image: RgbaImage,
    pub result: Value,
    pub latency_ms: f64,
}
