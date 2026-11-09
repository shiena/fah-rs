use crate::{Frame, Output};
use anyhow::{Result, bail};
pub use fah_core::runtime::Inference;
use std::path::Path;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Auto,
    Cuda,
    DirectMl,
    Cpu,
    Vulkan,
    Metal,
}
impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cuda => "cuda",
            Self::DirectMl => "directml",
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
            Self::Metal => "metal",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "cuda" => Ok(Self::Cuda),
            "directml" => Ok(Self::DirectMl),
            "cpu" => Ok(Self::Cpu),
            "vulkan" => Ok(Self::Vulkan),
            "metal" => Ok(Self::Metal),
            _ => bail!("Unknown backend: {value}"),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub cuda: bool,
}

pub trait InferenceFactory: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    /// Ordered automatic choices supported by this platform.
    fn automatic_backends(&self) -> &[Backend] {
        &[Backend::Cpu]
    }
    fn create(&self, models: &Path, backend: Backend) -> Result<Box<dyn Inference + Send>>;
    fn cpu_fallback(
        &self,
        models: &Path,
        _requested: Backend,
    ) -> Result<Box<dyn Inference + Send>> {
        self.create(models, Backend::Cpu)
    }
}
/// Synchronous host-facing session; no capture device, thread, or window is required.
pub trait TrackingSession: Send {
    fn process(&mut self, frame: Frame, reset: bool) -> Result<Output>;
    fn backend(&self) -> Backend;
    fn warning(&self) -> &str;
}
