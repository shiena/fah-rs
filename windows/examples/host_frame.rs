//! A host-owned frame loop without GPUI, FFmpeg, or the desktop worker.
use anyhow::{Context, Result};
use fah_api::{Backend, Frame};
use std::{path::Path, time::Instant};

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let models = args
        .get(1)
        .context("Usage: host_frame MODELS IMAGE [BACKEND]")?;
    let path = args.get(2).context("Missing image path")?;
    let backend = Backend::parse(args.get(3).map(String::as_str).unwrap_or("cpu"))?;
    let mut session = fah_windows::platform::session(Path::new(models), backend)?;
    let image = image::open(path)?.thumbnail(1280, 1280).to_rgba8();
    let output = session.process(
        Frame {
            image,
            captured: Instant::now(),
        },
        true,
    )?;
    println!("{}", serde_json::to_string(&output.result)?);
    Ok(())
}
fn main() -> Result<()> {
    let result = run();
    // The host-owned session is dropped even when processing returns an error.
    unsafe {
        fah_windows::platform::shutdown();
    }
    result
}
