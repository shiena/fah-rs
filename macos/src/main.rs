#[cfg(target_os = "macos")]
use fah_apple_platform as platform;
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Build fah-macos on macOS");
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use fah_api::*;
    use std::{
        path::{Path, PathBuf},
        sync::Mutex,
        time::{Duration, Instant},
    };
    struct Diagnostics {
        inner: Client,
        path: PathBuf,
        count: Mutex<usize>,
    }
    impl Controller for Diagnostics {
        fn send(&self, c: Command) -> anyhow::Result<()> {
            self.inner.send(c)
        }
        fn event(&self) -> Option<Event> {
            self.inner.event()
        }
        fn frame(&self) -> Option<Output> {
            let output = self.inner.frame();
            if let Some(ref frame) = output {
                let mut count = self.count.lock().unwrap();
                if *count < 300 {
                    use std::io::Write;
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&self.path)
                    {
                        let _ = writeln!(file, "{}", frame.result);
                    }
                    *count += 1;
                }
            }
            output
        }
        fn capabilities(&self) -> &Capabilities {
            self.inner.capabilities()
        }
        fn preview(&self, o: &Output, j: bool, m: bool) -> image::RgbaImage {
            self.inner.preview(o, j, m)
        }
        fn save(&self, p: &Path, o: &Output) -> anyhow::Result<()> {
            self.inner.save(p, o)
        }
    }
    let arguments: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        arguments
            .iter()
            .position(|x| x == flag)
            .and_then(|i| arguments.get(i + 1))
            .cloned()
    };
    let executable = std::env::current_exe()?;
    let defaults: serde_json::Value = std::fs::read(
        executable
            .parent()
            .unwrap()
            .join("../Resources/defaults.json"),
    )
    .ok()
    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    .unwrap_or_default();
    let models = PathBuf::from(
        value("--models")
            .or_else(|| defaults["models"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "models/apple".into()),
    );
    let library = value("--native").map(PathBuf::from).unwrap_or_else(|| {
        executable
            .parent()
            .unwrap()
            .join("../Frameworks/libFAHAppleNative.dylib")
    });
    let native = platform::Native::load(&library)?;
    let mut client = platform::controller(native);
    if let Some(path) = value("--log") {
        client = Box::new(Diagnostics {
            inner: client,
            path: path.into(),
            count: Mutex::new(0),
        });
    }
    let mut source = value("--input").map(|path| Source::file(path.into()));
    if arguments.iter().any(|s| s == "--camera") {
        client.send(Command::RefreshCameras)?;
        let start = Instant::now();
        loop {
            if let Some(Event::Cameras(devices)) = client.event() {
                let devices = devices.map_err(anyhow::Error::msg)?;
                let name = devices
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("No camera"))?;
                source = Some(Source::Camera(name.clone()));
                break;
            }
            anyhow::ensure!(
                start.elapsed() < Duration::from_secs(10),
                "Camera enumeration timeout"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if arguments.iter().any(|s| s == "--smoke") {
        let source = source.ok_or_else(|| anyhow::anyhow!("Supply --input FILE or --camera"))?;
        client.send(Command::Start(Request {
            id: 1,
            models,
            backend: Backend::Metal,
            source,
        }))?;
        let count: usize = value("--frames").unwrap_or_else(|| "1".into()).parse()?;
        let start = Instant::now();
        let mut processed = 0;
        while processed < count {
            if let Some(Event::Error(_, error)) = client.event() {
                anyhow::bail!("{error}")
            }
            if let Some(output) = client.frame() {
                println!("{}", output.result);
                processed += 1;
            }
            anyhow::ensure!(start.elapsed() < Duration::from_secs(120), "Smoke timeout");
            std::thread::sleep(Duration::from_millis(20));
        }
        return Ok(());
    }
    fah_gui::run(
        Options {
            models,
            source,
            backend: Backend::Auto,
            sample_video: value("--sample-video")
                .or_else(|| defaults["sample_video"].as_str().map(str::to_owned))
                .map(PathBuf::from)
                .unwrap_or_else(|| "models/sample.mp4".into()),
        },
        client,
    )
}
