use crate::{Backend, Capabilities, Output, Source};
use anyhow::Result;
use image::RgbaImage;
use std::path::{Path, PathBuf};
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
/// Desktop control API. Implementations own their worker and shutdown policy.
pub trait Controller: Send {
    fn send(&self, command: Command) -> Result<()>;
    fn event(&self) -> Option<Event>;
    fn frame(&self) -> Option<Output>;
    fn capabilities(&self) -> &Capabilities;
    fn preview(&self, output: &Output, joints: bool, mirror: bool) -> RgbaImage;
    fn save(&self, path: &Path, output: &Output) -> Result<()>;
}
pub type Client = Box<dyn Controller>;
pub struct Options {
    pub models: PathBuf,
    pub source: Option<Source>,
    pub backend: Backend,
    pub sample_video: PathBuf,
}
