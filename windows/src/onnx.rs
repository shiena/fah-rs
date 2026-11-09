//! Each runtime owns its API table, sessions, and dynamically loaded library.
use anyhow::{Context, Result, bail, ensure};
use fah_rust::runtime::{Inference, Tensor};
use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library};
use ort_sys::*;
use std::{
    ffi::{CStr, CString},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Auto,
    Cuda,
    DirectMl,
    Cpu,
    Vulkan,
}
impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cuda => "cuda",
            Self::DirectMl => "directml",
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "cuda" => Ok(Self::Cuda),
            "directml" => Ok(Self::DirectMl),
            "cpu" => Ok(Self::Cpu),
            "vulkan" => Ok(Self::Vulkan),
            _ => bail!("Unknown backend: {value}"),
        }
    }
}
struct Handle<T> {
    ptr: *mut T,
    release: unsafe extern "system" fn(*mut T),
}
impl<T> Handle<T> {
    fn new(release: unsafe extern "system" fn(*mut T)) -> Self {
        Self {
            ptr: ptr::null_mut(),
            release,
        }
    }
}
impl<T> Drop for Handle<T> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { (self.release)(self.ptr) }
        }
    }
}

pub struct Runtime {
    // Drop all native objects before unloading their library.
    pose: Handle<OrtSession>,
    detector: Handle<OrtSession>,
    memory: Handle<OrtMemoryInfo>,
    _env: Handle<OrtEnv>,
    api: &'static OrtApi,
    _library: Library,
}
// The worker exclusively owns sessions and calls Run synchronously.
unsafe impl Send for Runtime {}
fn checked(api: &OrtApi, status: OrtStatusPtr) -> Result<()> {
    if !status.0.is_null() {
        unsafe {
            let message = CStr::from_ptr((api.GetErrorMessage)(status.0))
                .to_string_lossy()
                .into_owned();
            (api.ReleaseStatus)(status.0);
            bail!(message);
        }
    }
    Ok(())
}
pub fn runtime_path(backend: Backend) -> Result<PathBuf> {
    let key = if backend == Backend::Cuda {
        "FAH_ORT_CUDA"
    } else {
        "FAH_ORT_DML"
    };
    let adjacent = std::env::current_exe()?
        .parent()
        .unwrap()
        .join("runtimes")
        .join(if backend == Backend::Cuda {
            "cuda"
        } else {
            "directml"
        })
        .join("onnxruntime.dll");
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../third_party")
        .join(if backend == Backend::Cuda {
            "ort-cuda13-1.31.1/onnxruntime-win-x64-gpu_cuda13-1.31.1/lib/onnxruntime.dll"
        } else {
            "ort-directml-1.24.4/runtimes/win-x64/native/onnxruntime.dll"
        });
    let path = std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| {
        if adjacent.is_file() || !development.is_file() {
            adjacent
        } else {
            development
        }
    });
    path.canonicalize().with_context(|| {
        format!(
            "{} is missing. Set {key} to onnxruntime.dll; see windows/README.md",
            path.display()
        )
    })
}
impl Runtime {
    pub fn new(models: &Path, backend: Backend) -> Result<Self> {
        ensure!(
            matches!(backend, Backend::Cuda | Backend::DirectMl | Backend::Cpu),
            "Invalid ONNX backend"
        );
        let path = runtime_path(backend)?;
        // Resolve dependencies beside the explicitly selected runtime first.
        let library = unsafe { Library::load_with_flags(&path, LOAD_WITH_ALTERED_SEARCH_PATH) }
            .with_context(|| {
                format!(
                    "Cannot load {} (check runtime dependencies)",
                    path.display()
                )
            })?;
        unsafe {
            let get = library
                .get::<unsafe extern "system" fn() -> *const OrtApiBase>(b"OrtGetApiBase\0")?;
            let api_ptr = ((*get()).GetApi)(ORT_API_VERSION);
            ensure!(
                !api_ptr.is_null(),
                "ONNX Runtime must support API {ORT_API_VERSION}"
            );
            let api = &*api_ptr;
            let mut env = Handle::new(api.ReleaseEnv);
            checked(
                api,
                (api.CreateEnv)(
                    OrtLoggingLevel::ORT_LOGGING_LEVEL_WARNING,
                    c"FAH".as_ptr(),
                    &mut env.ptr,
                ),
            )?;
            checked(api, (api.DisableTelemetryEvents)(env.ptr))?;
            let mut options = Handle::new(api.ReleaseSessionOptions);
            checked(api, (api.CreateSessionOptions)(&mut options.ptr))?;
            checked(api, (api.SetIntraOpNumThreads)(options.ptr, 4))?;
            checked(
                api,
                (api.SetSessionGraphOptimizationLevel)(
                    options.ptr,
                    GraphOptimizationLevel::ORT_ENABLE_ALL,
                ),
            )?;
            if backend == Backend::Cuda {
                let mut cuda = Handle::new(api.ReleaseCUDAProviderOptions);
                checked(api, (api.CreateCUDAProviderOptions)(&mut cuda.ptr))?;
                let keys = [c"device_id".as_ptr(), c"use_tf32".as_ptr()];
                let values = [c"0".as_ptr(), c"0".as_ptr()];
                checked(
                    api,
                    (api.UpdateCUDAProviderOptions)(
                        cuda.ptr,
                        keys.as_ptr(),
                        values.as_ptr(),
                        keys.len(),
                    ),
                )?;
                checked(
                    api,
                    (api.SessionOptionsAppendExecutionProvider_CUDA_V2)(options.ptr, cuda.ptr),
                )?;
            } else if backend == Backend::DirectMl {
                checked(api, (api.DisableMemPattern)(options.ptr))?;
                checked(
                    api,
                    (api.SetSessionExecutionMode)(options.ptr, ExecutionMode::ORT_SEQUENTIAL),
                )?;
                let append = library.get::<unsafe extern "system" fn(
                    *mut OrtSessionOptions,
                    i32,
                ) -> OrtStatusPtr>(
                    b"OrtSessionOptionsAppendExecutionProvider_DML\0"
                )?;
                checked(api, append(options.ptr, 0))?;
            }
            let load = |name: &str| -> Result<Handle<OrtSession>> {
                if let Some(mut prefix) = std::env::var_os("FAH_ORT_PROFILE") {
                    prefix.push(format!("-{name}"));
                    let wide: Vec<u16> = prefix.encode_wide().chain(Some(0)).collect();
                    checked(api, (api.EnableProfiling)(options.ptr, wide.as_ptr()))?;
                }
                let mut session = Handle::new(api.ReleaseSession);
                let path = models.join(name);
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                checked(
                    api,
                    (api.CreateSession)(env.ptr, wide.as_ptr(), options.ptr, &mut session.ptr),
                )
                .with_context(|| format!("Loading {} ({})", path.display(), backend.name()))?;
                Ok(session)
            };
            let pose = load("pose.onnx")?;
            let detector = load("detector.onnx")?;
            let mut memory = Handle::new(api.ReleaseMemoryInfo);
            checked(
                api,
                (api.CreateCpuMemoryInfo)(
                    OrtAllocatorType::OrtArenaAllocator,
                    OrtMemType::OrtMemTypeDefault,
                    &mut memory.ptr,
                ),
            )?;
            drop(options);
            let mut runtime = Self {
                pose,
                detector,
                memory,
                _env: env,
                api,
                _library: library,
            };
            // Some CUDA dependencies are loaded only by the first Conv, not by CreateSession.
            runtime
                .detect(&Tensor::new(640, 640, 3, vec![0.; 640 * 640 * 3]))
                .map_err(anyhow::Error::msg)?;
            runtime
                .pose(
                    &Tensor::new(320, 320, 3, vec![0.5; 320 * 320 * 3]),
                    &Tensor::new(99, 400, 1, vec![0.; 99 * 400]),
                    &Tensor::new(3, 1, 1, vec![0., 0., 0.64]),
                )
                .map_err(anyhow::Error::msg)?;
            Ok(runtime)
        }
    }
    fn run(
        &self,
        session: *mut OrtSession,
        inputs: &[(&str, &[i64], &[f32])],
        shapes: &[(usize, usize)],
    ) -> Result<Vec<Tensor>> {
        let api = self.api;
        let mut values = Vec::new();
        let names: Vec<_> = inputs
            .iter()
            .map(|(name, _, _)| CString::new(*name).unwrap())
            .collect();
        unsafe {
            for (_, shape, data) in inputs {
                ensure!(
                    shape.iter().product::<i64>() as usize == data.len(),
                    "Invalid input shape"
                );
                let mut value = Handle::new(api.ReleaseValue);
                checked(
                    api,
                    (api.CreateTensorWithDataAsOrtValue)(
                        self.memory.ptr,
                        data.as_ptr().cast_mut().cast(),
                        std::mem::size_of_val(*data),
                        shape.as_ptr(),
                        shape.len(),
                        ONNXTensorElementDataType::ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT,
                        &mut value.ptr,
                    ),
                )?;
                values.push(value);
            }
            let inputs: Vec<_> = values.iter().map(|v| v.ptr.cast_const()).collect();
            let names: Vec<_> = names.iter().map(|n| n.as_ptr()).collect();
            let output_strings: Vec<_> = (0..shapes.len())
                .map(|i| CString::new(format!("out{i}")).unwrap())
                .collect();
            let output_names: Vec<_> = output_strings.iter().map(|n| n.as_ptr()).collect();
            let mut raw = vec![ptr::null_mut(); shapes.len()];
            let status = (api.Run)(
                session,
                ptr::null(),
                names.as_ptr(),
                inputs.as_ptr(),
                inputs.len(),
                output_names.as_ptr(),
                raw.len(),
                raw.as_mut_ptr(),
            );
            let outputs: Vec<_> = raw
                .into_iter()
                .map(|ptr| Handle {
                    ptr,
                    release: api.ReleaseValue,
                })
                .collect();
            checked(api, status)?;
            outputs
                .iter()
                .zip(shapes)
                .map(|(value, &(w, h))| {
                    let mut info = Handle::new(api.ReleaseTensorTypeAndShapeInfo);
                    checked(api, (api.GetTensorTypeAndShape)(value.ptr, &mut info.ptr))?;
                    let mut kind =
                        ONNXTensorElementDataType::ONNX_TENSOR_ELEMENT_DATA_TYPE_UNDEFINED;
                    checked(api, (api.GetTensorElementType)(info.ptr, &mut kind))?;
                    ensure!(
                        kind == ONNXTensorElementDataType::ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT,
                        "Expected FP32 output"
                    );
                    let mut length = 0;
                    checked(api, (api.GetTensorShapeElementCount)(info.ptr, &mut length))?;
                    ensure!(
                        length == w * h,
                        "Unexpected output size: {length}, expected {}",
                        w * h
                    );
                    let mut data = ptr::null_mut();
                    checked(api, (api.GetTensorMutableData)(value.ptr, &mut data))?;
                    ensure!(!data.is_null(), "Null output");
                    Ok(Tensor::new(
                        w,
                        h,
                        1,
                        std::slice::from_raw_parts(data.cast::<f32>(), length).to_vec(),
                    ))
                })
                .collect()
        }
    }
}
impl Inference for Runtime {
    fn detect(&mut self, image: &Tensor) -> fah_rust::Result<Tensor> {
        self.run(
            self.detector.ptr,
            &[("img", &[1, 3, 640, 640], &image.data)],
            &[(8400, 6)],
        )
        .map(|mut out| out.remove(0))
        .map_err(|e| format!("{e:#}"))
    }
    fn pose(
        &mut self,
        image: &Tensor,
        rays: &Tensor,
        condition: &Tensor,
    ) -> fah_rust::Result<Vec<Tensor>> {
        self.run(
            self.pose.ptr,
            &[
                ("img", &[1, 3, 320, 320], &image.data),
                ("rays", &[1, 400, 99], &rays.data),
                ("cond", &[1, 3], &condition.data),
            ],
            &[(2, 461), (3, 461), (461, 1), (461, 1), (1, 1), (2, 1)],
        )
        .map_err(|e| format!("{e:#}"))
    }
}
pub fn create(
    models: &Path,
    requested: Backend,
) -> Result<(fah_rust::pipeline::Pipeline, Backend, String)> {
    let choices: &[Backend] = if requested == Backend::Auto {
        &[Backend::Cuda, Backend::DirectMl, Backend::Cpu]
    } else {
        std::slice::from_ref(&requested)
    };
    let mut failures = Vec::new();
    for &backend in choices {
        let path = models.to_path_buf();
        let gpu = backend != Backend::Cpu;
        let pipeline = fah_rust::pipeline::Pipeline::new(
            &models.to_string_lossy(),
            gpu,
            false,
            Box::new(move |gpu, _| {
                let selected = if gpu { backend } else { Backend::Cpu };
                if backend == Backend::Vulkan {
                    fah_rust::runtime::Runtime::new(&path.to_string_lossy(), gpu, false, true)
                        .map(|r| Box::new(r) as Box<dyn Inference + Send>)
                } else {
                    Runtime::new(&path, selected)
                        .map(|r| Box::new(r) as Box<dyn Inference + Send>)
                        .map_err(|e| format!("{e:#}"))
                }
            }),
        );
        match pipeline {
            Ok(pipeline) => return Ok((pipeline, backend, failures.join("\n"))),
            Err(error) => failures.push(format!("{}: {error}", backend.name())),
        }
    }
    bail!("{}", failures.join("\n"))
}
pub fn check(models: &Path, backend: Backend, runs: usize, fixture: &Path) -> Result<()> {
    ensure!((1..=1000).contains(&runs), "runs must be 1..1000");
    let mut net = Runtime::new(models, backend)?;
    let read = |name: &str| fah_rust::read_floats(fixture.join(name)).map_err(anyhow::Error::msg);
    let img = Tensor::new(320, 320, 3, read("img.f32")?);
    let rays = Tensor::new(99, 400, 1, read("rays.f32")?);
    let cond = Tensor::new(3, 1, 1, read("cond.f32")?);
    let mut times = Vec::new();
    let mut outputs = Vec::new();
    for i in 0..runs + 2 {
        let start = std::time::Instant::now();
        outputs = net.pose(&img, &rays, &cond).map_err(anyhow::Error::msg)?;
        if i >= 2 {
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    times.sort_by(f64::total_cmp);
    let mut passed = true;
    let mut errors = Vec::new();
    for (i, out) in outputs.iter().enumerate() {
        let reference = read(&format!("reference{i}.f32"))?;
        ensure!(
            reference.len() == out.data.len(),
            "Reference shape mismatch"
        );
        let mut max = 0f32;
        for (&a, &b) in out.data.iter().zip(&reference) {
            ensure!(a.is_finite() && b.is_finite(), "Non-finite output");
            max = max.max((a - b).abs());
        }
        if (i == 0 && max * 320. > 0.5) || (i == 1 && max * 1000. > 1.) {
            passed = false;
        }
        errors.push(max);
    }
    println!(
        "{}",
        serde_json::json!({"backend":backend.name(),"runs":runs,"median_ms":(times[(runs-1)/2]+times[runs/2])*0.5,"max_errors":errors,"passed":passed})
    );
    ensure!(passed, "Numerical validation failed");
    Ok(())
}
