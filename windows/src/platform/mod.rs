pub mod ort;
pub mod video;
use anyhow::Result;
use fah_api::{Client, FrameSource, InputFactory, Source};
use std::sync::Arc;
pub struct Inputs;
impl InputFactory for Inputs {
    fn cameras(&self) -> Result<Vec<String>> {
        video::cameras()
    }
    fn open(&self, source: &Source) -> Result<Box<dyn FrameSource>> {
        match source {
            Source::Image(path) => Ok(Box::new(fah_input::StillImage::open(path)?)),
            _ => Ok(Box::new(video::Stream::open(source)?)),
        }
    }
}
pub fn controller() -> Client {
    Box::new(fah_application::Worker::new(
        Arc::new(crate::inference::Backends),
        Arc::new(Inputs),
    ))
}
/// Create a synchronous session for hosts that own capture and scheduling.
pub fn session(
    models: &std::path::Path,
    backend: fah_api::Backend,
) -> Result<Box<dyn fah_api::TrackingSession>> {
    Ok(Box::new(fah_application::Session::open(
        models,
        backend,
        Arc::new(crate::inference::Backends),
    )?))
}
/// Release process-wide Vulkan resources before unloading the native backend.
///
/// # Safety
/// All clients, sessions, and inference threads must have been dropped or joined.
pub unsafe fn shutdown() {
    // Call only after all application clients and synchronous sessions are dropped.
    unsafe {
        fah_ncnn::runtime::shutdown();
    }
}
