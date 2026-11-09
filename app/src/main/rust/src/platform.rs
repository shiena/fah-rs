use anyhow::Result;
use fah_api::{Backend, TrackingSession};
pub use fah_ncnn::adapter::Backends;
use std::{path::Path, sync::Arc};

/// Create a session without an Activity, camera, GUI, or worker thread.
pub fn session(models: &Path, backend: Backend) -> Result<Box<dyn TrackingSession>> {
    Ok(Box::new(fah_application::Session::open(
        models,
        backend,
        Arc::new(Backends),
    )?))
}
