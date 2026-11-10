#![cfg(any(target_os = "linux", target_os = "android"))]
use anyhow::{bail, Context, Result};
use fah_api::{Backend, Capabilities, Frame, Inference, InferenceFactory, Output, TrackingSession};
use fah_core::runtime::Tensor;
use fah_litert::{Model, TensorSpec};
#[cfg(target_os = "linux")]
use std::ffi::CStr;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Shared Linux/Android policy: ncnn Vulkan first, LiteRT pose plus ncnn detector on CPU.
pub struct Backends;
impl InferenceFactory for Backends {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn automatic_backends(&self) -> &[Backend] {
        &[Backend::Vulkan, Backend::Cpu]
    }
    fn create(&self, models: &Path, backend: Backend) -> Result<Box<dyn Inference + Send>> {
        match backend {
            Backend::Cpu => Ok(Box::new(Cpu::open(models)?)),
            Backend::Vulkan => fah_ncnn::adapter::Backends.create(models, backend),
            _ => bail!("Unsupported ncnn/LiteRT backend: {}", backend.name()),
        }
    }
}
fn library_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("FAH_LITERT_LIBRARY") {
        return Ok(path.into());
    }
    // Android can load libraries directly from an APK, so dladdr is not a filesystem path.
    #[cfg(target_os = "android")]
    return Ok("libLiteRt.so".into());
    #[cfg(target_os = "linux")]
    {
        // Resolve relative to this loaded module, not the host executable or cwd.
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        let found =
            unsafe { libc::dladdr(library_path as *const () as *const libc::c_void, &mut info) };
        if found != 0 && !info.dli_fname.is_null() {
            let file = unsafe { CStr::from_ptr(info.dli_fname) }.to_str()?;
            let module = Path::new(file).canonicalize()?;
            return Ok(module
                .parent()
                .context("Missing native module directory")?
                .join("libLiteRt.so"));
        }
        bail!("Cannot locate libLiteRt.so; set FAH_LITERT_LIBRARY to its absolute path")
    }
}
pub const POSE_INPUTS: &[TensorSpec<'static>] = &[
    TensorSpec {
        name: "serving_default_args_0",
        shape: &[1, 3, 320, 320],
    },
    TensorSpec {
        name: "serving_default_args_1",
        shape: &[1, 400, 99],
    },
    TensorSpec {
        name: "serving_default_args_2",
        shape: &[1, 3],
    },
];
pub const POSE_OUTPUTS: &[TensorSpec<'static>] = &[
    TensorSpec {
        name: "serving_default_output_0_output",
        shape: &[1, 461, 2],
    },
    TensorSpec {
        name: "serving_default_output_1_output",
        shape: &[1, 461, 3],
    },
    TensorSpec {
        name: "serving_default_output_2_output",
        shape: &[1, 461],
    },
    TensorSpec {
        name: "serving_default_output_3_output",
        shape: &[1, 461],
    },
    TensorSpec {
        name: "serving_default_output_4_output",
        shape: &[1, 1],
    },
    TensorSpec {
        name: "serving_default_output_5_output",
        shape: &[1, 2],
    },
];
struct Cpu {
    pose: Model,
    detector: fah_ncnn::runtime::Runtime,
}
impl Cpu {
    fn open(models: &Path) -> Result<Self> {
        let pose = Model::open(
            &library_path()?,
            &models.join("litert/pose.tflite"),
            POSE_INPUTS,
            POSE_OUTPUTS,
        )
        .context("CPU inference requires libLiteRt.so and models/litert/pose.tflite")?;
        let detector = fah_ncnn::runtime::Runtime::detector(&models.to_string_lossy())
            .map_err(anyhow::Error::msg)?;
        Ok(Self { pose, detector })
    }
}
impl Inference for Cpu {
    fn detect(&mut self, image: &Tensor) -> fah_core::Result<Tensor> {
        self.detector.detect(image)
    }
    fn pose(
        &mut self,
        image: &Tensor,
        rays: &Tensor,
        condition: &Tensor,
    ) -> fah_core::Result<Vec<Tensor>> {
        let outputs = self
            .pose
            .run(&[&image.data, &rays.data, &condition.data])
            .map_err(|e| format!("{e:#}"))?;
        let shapes = [(2, 461), (3, 461), (461, 1), (461, 1), (1, 1), (2, 1)];
        Ok(outputs
            .into_iter()
            .zip(shapes)
            .map(|(data, (w, h))| Tensor::new(w, h, 1, data))
            .collect())
    }
}
pub fn session(models: &Path, backend: Backend) -> Result<Box<dyn TrackingSession>> {
    Ok(Box::new(Session(fah_application::Session::open(
        models,
        backend,
        Arc::new(Backends),
    )?)))
}
struct Session(fah_application::Session);
impl TrackingSession for Session {
    fn process(&mut self, frame: Frame, reset: bool) -> Result<Output> {
        let mut output = self.0.process(frame, reset)?;
        output.result["pose_engine"] = if self.0.backend() == Backend::Cpu {
            "litert"
        } else {
            "ncnn"
        }
        .into();
        output.result["detector_engine"] = "ncnn".into();
        Ok(output)
    }
    fn backend(&self) -> Backend {
        self.0.backend()
    }
    fn warning(&self) -> &str {
        self.0.warning()
    }
}
