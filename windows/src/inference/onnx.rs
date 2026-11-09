use crate::platform::ort::{cuda_libraries, load, runtime_path, wide};
use anyhow::{Context, Result, bail, ensure};
use fah_api::Backend;
use fah_core::runtime::{Inference, Tensor};
use libloading::Library;
use ort_sys::*;
use std::{
    ffi::{CStr, CString},
    path::Path,
    ptr,
};
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
impl Runtime {
    pub fn new(models: &Path, backend: Backend) -> Result<Self> {
        if backend == Backend::Cuda {
            cuda_libraries()?;
        }
        ensure!(
            matches!(backend, Backend::Cuda | Backend::DirectMl | Backend::Cpu),
            "Invalid ONNX backend"
        );
        let path = runtime_path(backend)?;
        // Resolve dependencies beside the explicitly selected runtime first.
        let library = load(&path).with_context(|| {
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
                    let wide: Vec<u16> = wide(&prefix);
                    checked(api, (api.EnableProfiling)(options.ptr, wide.as_ptr()))?;
                }
                let mut session = Handle::new(api.ReleaseSession);
                let path = models.join(name);
                let wide: Vec<u16> = wide(path.as_os_str());
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
    fn detect(&mut self, image: &Tensor) -> fah_core::Result<Tensor> {
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
    ) -> fah_core::Result<Vec<Tensor>> {
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
pub fn check(models: &Path, backend: Backend, runs: usize, fixture: &Path) -> Result<()> {
    ensure!((1..=1000).contains(&runs), "runs must be 1..1000");
    let mut net = Runtime::new(models, backend)?;
    let read = |name: &str| fah_core::read_floats(fixture.join(name)).map_err(anyhow::Error::msg);
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
