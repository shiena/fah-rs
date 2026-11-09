use anyhow::Result;
use image::RgbaImage;
use std::{path::PathBuf, time::Instant};
#[derive(Clone, Debug)]
pub enum Source {
    Image(PathBuf),
    Video(PathBuf),
    SampleVideo(PathBuf),
    Camera(String),
}
impl Source {
    pub fn label(&self) -> String {
        match self {
            Self::Camera(name) => name.clone(),
            Self::Image(path) | Self::Video(path) | Self::SampleVideo(path) => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        }
    }
    pub fn file(path: PathBuf) -> Self {
        match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" | "jpg" | "jpeg" | "webp" | "bmp" => Self::Image(path),
            _ => Self::Video(path),
        }
    }
}
pub struct Frame {
    pub image: RgbaImage,
    pub captured: Instant,
}
/// A host may provide frames without opening a camera or creating a GUI.
pub trait FrameSource: Send {
    fn next_frame(&mut self) -> Result<Option<Frame>>;
    fn finished(&mut self) -> Result<bool>;
}
pub trait InputFactory: Send + Sync {
    fn cameras(&self) -> Result<Vec<String>>;
    fn open(&self, source: &Source) -> Result<Box<dyn FrameSource>>;
}
