use anyhow::{Context, Result, bail};
use image::RgbaImage;
use std::{
    io::{BufRead, BufReader, Read},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Instant,
};
const CREATE_NO_WINDOW: u32 = 0x08000000;
pub const VIDEO_WIDTH: u32 = 640;
pub const VIDEO_HEIGHT: u32 = 480;

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
fn ffmpeg() -> Command {
    let path = std::env::var_os("FAH_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let local = std::env::current_exe()
                .unwrap_or_default()
                .with_file_name("ffmpeg.exe");
            if local.exists() {
                local
            } else {
                PathBuf::from("ffmpeg.exe")
            }
        });
    let mut cmd = Command::new(path);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
pub fn camera_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| line.contains("(video)"))
        .filter_map(|line| {
            let start = line.find('"')? + 1;
            let end = line.rfind('"')?;
            (end > start).then(|| line[start..end].to_owned())
        })
        .collect()
}
pub fn cameras() -> Result<Vec<String>> {
    let output = ffmpeg()
        .args([
            "-hide_banner",
            "-list_devices",
            "true",
            "-f",
            "dshow",
            "-i",
            "dummy",
        ])
        .stdin(Stdio::null())
        .output()
        .context("FFmpegが見つかりません。FFmpegをPATHに登録してください。")?;
    Ok(camera_names(&String::from_utf8_lossy(&output.stderr)))
}
#[derive(Default)]
struct StreamState {
    latest: Option<Frame>,
    ended: bool,
    error: Option<String>,
}
pub enum Input {
    Still(Option<Frame>),
    Stream(Stream),
}
impl Input {
    pub fn open(source: &Source) -> Result<Self> {
        if let Source::Image(path) = source {
            let image = image::open(path)
                .with_context(|| format!("画像を開けません: {}", path.display()))?
                .thumbnail(1280, 1280)
                .to_rgba8();
            return Ok(Self::Still(Some(Frame {
                image,
                captured: Instant::now(),
            })));
        }
        Ok(Self::Stream(Stream::open(source)?))
    }
    pub fn next(&mut self) -> Result<Option<Frame>> {
        match self {
            Self::Still(frame) => Ok(frame.take()),
            Self::Stream(stream) => {
                let mut state = stream.state.lock().unwrap();
                if let Some(error) = state.error.take() {
                    bail!(error);
                }
                Ok(state.latest.take())
            }
        }
    }
    pub fn finished(&mut self) -> Result<bool> {
        match self {
            Self::Still(frame) => Ok(frame.is_none()),
            Self::Stream(stream) => {
                if !stream.state.lock().unwrap().ended {
                    return Ok(false);
                }
                if stream.state.lock().unwrap().latest.is_some() {
                    return Ok(false);
                }
                if let Some(status) = stream.child.try_wait()? {
                    if !status.success() {
                        let message = stream.stderr.lock().unwrap().clone();
                        bail!("入力を終了しました: {message}");
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }
}
pub struct Stream {
    child: Child,
    state: Arc<Mutex<StreamState>>,
    stderr: Arc<Mutex<String>>,
    reader: Option<JoinHandle<()>>,
    error_reader: Option<JoinHandle<()>>,
}
impl Stream {
    fn open(source: &Source) -> Result<Self> {
        let mut cmd = ffmpeg();
        cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        match source {
            Source::Camera(name) => {
                cmd.args([
                    "-f",
                    "dshow",
                    "-rtbufsize",
                    "16M",
                    "-i",
                    &format!("video={name}"),
                ]);
            }
            Source::Video(path) | Source::SampleVideo(path) => {
                cmd.arg("-re").arg("-i").arg(path);
            }
            Source::Image(_) => unreachable!(),
        }
        let mut filter = String::new();
        if matches!(source, Source::SampleVideo(_)) {
            filter.push_str("crop=640:426:0:0,");
        }
        filter.push_str("scale=640:480:force_original_aspect_ratio=decrease,pad=640:480:(ow-iw)/2:(oh-ih)/2,setsar=1");
        cmd.args([
            "-an", "-sn", "-dn", "-vf", &filter, "-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1",
        ]);
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("FFmpegを起動できません。FFmpegをPATHに登録してください。")?;
        let mut stdout = child.stdout.take().unwrap();
        let error_output = child.stderr.take().unwrap();
        let state = Arc::new(Mutex::new(StreamState::default()));
        let shared = state.clone();
        let reader = thread::spawn(move || {
            loop {
                let mut bytes = vec![0; (VIDEO_WIDTH * VIDEO_HEIGHT * 4) as usize];
                if let Err(error) = stdout.read_exact(&mut bytes) {
                    let mut state = shared.lock().unwrap();
                    if error.kind() != std::io::ErrorKind::UnexpectedEof {
                        state.error = Some(error.to_string());
                    }
                    state.ended = true;
                    break;
                }
                let image = RgbaImage::from_raw(VIDEO_WIDTH, VIDEO_HEIGHT, bytes).unwrap();
                // Replace the pending frame instead of building a capture backlog.
                shared.lock().unwrap().latest = Some(Frame {
                    image,
                    captured: Instant::now(),
                });
            }
        });
        let stderr = Arc::new(Mutex::new(String::new()));
        let errors = stderr.clone();
        let error_reader = thread::spawn(move || {
            for line in BufReader::new(error_output)
                .lines()
                .map_while(std::result::Result::ok)
            {
                let mut error = errors.lock().unwrap();
                if error.len() > 8192 {
                    error.clear();
                }
                error.push_str(&line);
                error.push('\n');
            }
        });
        Ok(Self {
            child,
            state,
            stderr,
            reader: Some(reader),
            error_reader: Some(error_reader),
        })
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.error_reader.take() {
            let _ = reader.join();
        }
    }
}
pub fn validate_models(directory: &Path, backend: crate::onnx::Backend) -> Result<()> {
    let files: &[&str] = if backend == crate::onnx::Backend::Vulkan {
        &[
            "pose.ncnn.param",
            "pose.ncnn.bin",
            "detector.ncnn.param",
            "detector.ncnn.bin",
            "uv_grid.f32",
        ]
    } else {
        &["pose.onnx", "detector.onnx", "uv_grid.f32"]
    };
    for name in files {
        if !directory.join(name).is_file() {
            bail!("モデルがありません: {}", directory.join(name).display());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_only_video_devices() {
        let names = camera_names(
            "[dshow] \"USB camera\" (video)\n[dshow] Alternative name \"device-id\"\n[dshow] \"microphone\" (audio)\n[dshow] \"カメラ 2\" (video)",
        );
        assert_eq!(names, vec!["USB camera", "カメラ 2"]);
    }
    #[test]
    fn recognizes_image_extensions_without_case_sensitivity() {
        assert!(matches!(
            Source::file("sample.PNG".into()),
            Source::Image(_)
        ));
        assert!(matches!(
            Source::file("sample.mp4".into()),
            Source::Video(_)
        ));
    }
}
