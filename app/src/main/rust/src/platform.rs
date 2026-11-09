use anyhow::{bail, Result};
use fah_api::{Backend, Capabilities, Inference, InferenceFactory, TrackingSession};
use std::{path::Path, sync::Arc};

/// Android inference adapter; frames and scheduling belong to the host.
pub struct Backends;
impl InferenceFactory for Backends {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn automatic_backends(&self) -> &[Backend] {
        &[Backend::Vulkan, Backend::Cpu]
    }
    fn create(&self, models: &Path, backend: Backend) -> Result<Box<dyn Inference + Send>> {
        if !matches!(backend, Backend::Vulkan | Backend::Cpu) {
            bail!("Unsupported Android backend: {}", backend.name());
        }
        let runtime = fah_ncnn::runtime::Runtime::new(
            &models.to_string_lossy(),
            backend == Backend::Vulkan,
            false,
            true,
        )
        .map_err(anyhow::Error::msg)?;
        Ok(Box::new(runtime))
    }
}

/// Create a session without an Activity, camera, GUI, or worker thread.
pub fn session(models: &Path, backend: Backend) -> Result<Box<dyn TrackingSession>> {
    Ok(Box::new(fah_application::Session::open(
        models,
        backend,
        Arc::new(Backends),
    )?))
}
