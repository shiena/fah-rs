pub mod onnx;
use anyhow::{Result, bail};
use fah_api::{Backend, Capabilities, Inference, InferenceFactory};
use std::path::Path;
pub struct Backends;
impl InferenceFactory for Backends {
    fn automatic_backends(&self) -> &[Backend] {
        &[Backend::Cuda, Backend::DirectMl, Backend::Cpu]
    }
    fn cpu_fallback(&self, models: &Path, requested: Backend) -> Result<Box<dyn Inference + Send>> {
        if requested == Backend::Vulkan {
            return fah_ncnn::runtime::Runtime::new(&models.to_string_lossy(), false, false, true)
                .map(|r| Box::new(r) as Box<dyn Inference + Send>)
                .map_err(anyhow::Error::msg);
        }
        self.create(models, Backend::Cpu)
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cuda: crate::platform::ort::cuda_libraries().is_ok(),
        }
    }
    fn create(&self, models: &Path, backend: Backend) -> Result<Box<dyn Inference + Send>> {
        validate_models(models, backend)?;
        if backend == Backend::Vulkan {
            return fah_ncnn::runtime::Runtime::new(&models.to_string_lossy(), true, false, true)
                .map(|r| Box::new(r) as Box<dyn Inference + Send>)
                .map_err(anyhow::Error::msg);
        }
        Ok(Box::new(onnx::Runtime::new(models, backend)?))
    }
}
pub fn validate_models(directory: &Path, backend: Backend) -> Result<()> {
    let files: &[&str] = if backend == Backend::Vulkan {
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
