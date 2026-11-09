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
impl Frame {
    /// Copy host RGBA8 rows into owned storage, excluding any row padding.
    pub fn from_rgba(
        bytes: &[u8],
        width: u32,
        height: u32,
        stride: usize,
        captured: Instant,
    ) -> Result<Self> {
        anyhow::ensure!(
            width > 0 && height > 0 && width <= 8192 && height <= 8192,
            "Invalid frame dimensions"
        );
        let row = width as usize * 4;
        anyhow::ensure!(stride >= row, "Invalid frame stride");
        let required = stride
            .checked_mul(height as usize - 1)
            .and_then(|n| n.checked_add(row))
            .ok_or_else(|| anyhow::anyhow!("Frame size overflow"))?;
        anyhow::ensure!(bytes.len() >= required, "Frame buffer is too short");
        let mut image = RgbaImage::new(width, height);
        for (y, target) in image.as_mut().chunks_exact_mut(row).enumerate() {
            target.copy_from_slice(&bytes[y * stride..y * stride + row]);
        }
        Ok(Self { image, captured })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_frame_excludes_padding_and_owns_its_pixels() {
        let mut pixels = vec![1, 2, 3, 255, 99, 99, 99, 99, 4, 5, 6, 255];
        let frame = Frame::from_rgba(&pixels, 1, 2, 8, Instant::now()).unwrap();
        pixels.fill(0);
        assert_eq!(frame.image.as_raw(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    }
    #[test]
    fn invalid_host_frame_is_rejected() {
        for (width, height, stride) in [
            (0, 1, 4),
            (1, 0, 4),
            (8193, 1, 32772),
            (1, 1, 3),
            (1, 2, 4),
            (1, 3, usize::MAX),
        ] {
            assert!(Frame::from_rgba(&[0; 4], width, height, stride, Instant::now()).is_err());
        }
    }
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
