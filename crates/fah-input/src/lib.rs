use anyhow::{Context, Result};
use fah_api::{Frame, FrameSource};
use std::{path::Path, time::Instant};
pub struct StillImage(Option<Frame>);
impl StillImage {
    pub fn open(path: &Path) -> Result<Self> {
        let image = image::open(path)
            .with_context(|| format!("画像を開けません: {}", path.display()))?
            .thumbnail(1280, 1280)
            .to_rgba8();
        Ok(Self(Some(Frame {
            image,
            captured: Instant::now(),
        })))
    }
}
impl FrameSource for StillImage {
    fn next_frame(&mut self) -> Result<Option<Frame>> {
        Ok(self.0.take())
    }
    fn finished(&mut self) -> Result<bool> {
        Ok(self.0.is_none())
    }
}
