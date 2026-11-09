//! C boundary between Swift Metal execution and the shared Rust tracking session.
use anyhow::{bail, ensure, Result};
use fah_api::{Backend, Capabilities, Frame, Inference, InferenceFactory, TrackingSession};
use fah_core::runtime::Tensor;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    ptr,
    sync::{Arc, Mutex},
    time::Instant,
};

const DETECT_LEN: usize = 6 * 8400;
const POSE_LEN: usize = 922 + 1383 + 461 + 461 + 1 + 2;
const ERROR_LEN: usize = 1024;
type Detect = unsafe extern "C" fn(*mut c_void, *const f32, *mut f32, *mut c_char) -> i32;
type Pose = unsafe extern "C" fn(
    *mut c_void,
    *const f32,
    *const f32,
    *const f32,
    *mut f32,
    *mut c_char,
) -> i32;

#[repr(C)]
pub struct FahMetalCallbacks {
    pub abi_version: u32,
    pub context: *mut c_void,
    pub detect: Option<Detect>,
    pub pose: Option<Pose>,
    pub destroy: Option<unsafe extern "C" fn(*mut c_void)>,
}
struct Native(FahMetalCallbacks);
// The ABI requires thread-independent contexts; the mutex serializes callbacks.
unsafe impl Send for Native {}
impl Drop for Native {
    fn drop(&mut self) {
        unsafe { (self.0.destroy.unwrap())(self.0.context) }
    }
}
struct Runtime(Arc<Mutex<Native>>, Arc<Vec<f32>>);
/// Wrap a native Metal network for the shared application worker.
///
/// # Safety
/// A valid v1 callback context is transferred to Rust, including on grid errors.
/// Callbacks must follow fah_apple.h; invalid tables do not transfer ownership.
pub unsafe fn metal_runtime(
    callbacks: FahMetalCallbacks,
    grid: Vec<f32>,
) -> Result<Box<dyn Inference + Send>> {
    ensure!(
        callbacks.abi_version == 1
            && callbacks.detect.is_some()
            && callbacks.pose.is_some()
            && callbacks.destroy.is_some(),
        "Invalid Metal callback ABI"
    );
    let native = Arc::new(Mutex::new(Native(callbacks)));
    ensure!(
        grid.len() == 800 && grid.iter().all(|v| v.is_finite()),
        "Invalid camera grid"
    );
    Ok(Box::new(Runtime(native, Arc::new(grid))))
}
fn completed(status: i32, error: &[c_char; ERROR_LEN], output: &[f32]) -> fah_core::Result<()> {
    if status != 0 {
        let bytes: Vec<u8> = error
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect();
        return Err(format!(
            "Metal callback failed ({status}): {}",
            String::from_utf8_lossy(&bytes)
        ));
    }
    if output.iter().any(|v| !v.is_finite()) {
        return Err("Non-finite Metal output".into());
    }
    Ok(())
}
fn camera(rays: &[f32], grid: &[f32]) -> fah_core::Result<[f32; 4]> {
    if rays.len() != 400 * 99 || grid.len() != 800 {
        return Err("Invalid camera rays or grid".into());
    }
    let mut result = [0.; 4];
    for axis in 0..2 {
        let span = grid[798 + axis] - grid[axis];
        if !span.is_finite() || span.abs() < 1e-6 {
            return Err("Degenerate camera grid".into());
        }
        let scale = (rays[399 * 99 + axis] - rays[axis]) / span;
        let offset = rays[axis] - grid[axis] * scale;
        if !scale.is_finite() || !offset.is_finite() {
            return Err("Non-finite camera rays".into());
        }
        for point in 0..400 {
            let value = rays[point * 99 + axis];
            if !value.is_finite() || (grid[point * 2 + axis] * scale + offset - value).abs() > 1e-5
            {
                return Err("Metal requires affine camera rays".into());
            }
        }
        result[axis * 2] = scale;
        result[axis * 2 + 1] = offset;
    }
    Ok(result)
}

impl Inference for Runtime {
    fn detect(&mut self, image: &Tensor) -> fah_core::Result<Tensor> {
        if image.data.len() != 3 * 640 * 640 {
            return Err("Invalid detector input".into());
        }
        let mut output = vec![f32::NAN; DETECT_LEN];
        let mut error = [0; ERROR_LEN];
        let native = self.0.lock().map_err(|e| e.to_string())?;
        let status = unsafe {
            (native.0.detect.unwrap())(
                native.0.context,
                image.data.as_ptr(),
                output.as_mut_ptr(),
                error.as_mut_ptr(),
            )
        };
        completed(status, &error, &output)?;
        Ok(Tensor::new(8400, 6, 1, output))
    }
    fn pose(
        &mut self,
        image: &Tensor,
        rays: &Tensor,
        condition: &Tensor,
    ) -> fah_core::Result<Vec<Tensor>> {
        if image.data.len() != 3 * 320 * 320 || rays.data.len() != 400 * 99 {
            return Err("Invalid pose input".into());
        }
        if condition.data.len() != 3 || condition.data.iter().any(|v| !v.is_finite()) {
            return Err("Invalid CLIFF condition".into());
        }
        let ray = camera(&rays.data, &self.1)?;
        let mut output = vec![f32::NAN; POSE_LEN];
        let mut error = [0; ERROR_LEN];
        let native = self.0.lock().map_err(|e| e.to_string())?;
        let status = unsafe {
            (native.0.pose.unwrap())(
                native.0.context,
                image.data.as_ptr(),
                ray.as_ptr(),
                condition.data.as_ptr(),
                output.as_mut_ptr(),
                error.as_mut_ptr(),
            )
        };
        completed(status, &error, &output)?;
        let mut offset = 0;
        Ok([(2, 461), (3, 461), (1, 461), (1, 461), (1, 1), (2, 1)]
            .into_iter()
            .map(|(w, h)| {
                let data = output[offset..offset + w * h].to_vec();
                offset += w * h;
                Tensor::new(w, h, 1, data)
            })
            .collect())
    }
}
struct Factory(Arc<Mutex<Native>>, Arc<Vec<f32>>);
impl InferenceFactory for Factory {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn automatic_backends(&self) -> &[Backend] {
        &[Backend::Metal]
    }
    fn create(&self, _: &Path, backend: Backend) -> Result<Box<dyn Inference + Send>> {
        if backend != Backend::Metal {
            bail!("Apple Metal adapter has no CPU fallback");
        }
        Ok(Box::new(Runtime(self.0.clone(), self.1.clone())))
    }
}
pub struct FahSession(Mutex<Box<dyn TrackingSession>>);
fn owned(text: String) -> *mut c_char {
    CString::new(text.replace('\0', "\\0")).unwrap().into_raw()
}
unsafe fn boundary<T>(error: *mut *mut c_char, f: impl FnOnce() -> Result<T>) -> Option<T> {
    if !error.is_null() {
        *error = ptr::null_mut();
    }
    let result = catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("Rust tracking panicked")));
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            if !error.is_null() {
                *error = owned(format!("{e:#}"));
            }
            None
        }
    }
}

/// # Safety
/// All pointers must follow the ownership and callback rules in fah_apple.h.
#[no_mangle]
pub unsafe extern "C" fn fah_apple_create(
    models: *const c_char,
    callbacks: FahMetalCallbacks,
    error: *mut *mut c_char,
) -> *mut FahSession {
    boundary(error, || {
        ensure!(
            callbacks.abi_version == 1
                && callbacks.detect.is_some()
                && callbacks.pose.is_some()
                && callbacks.destroy.is_some(),
            "Invalid Metal callback ABI"
        );
        let native = Arc::new(Mutex::new(Native(callbacks)));
        ensure!(!models.is_null(), "Missing model directory");
        let path = CStr::from_ptr(models).to_str()?;
        let session = fah_application::Session::open(
            Path::new(path),
            Backend::Metal,
            Arc::new(Factory(
                native,
                Arc::new(
                    fah_core::read_floats(Path::new(path).join("uv_grid.f32"))
                        .map_err(anyhow::Error::msg)?,
                ),
            )),
        )?;
        Ok(Box::into_raw(Box::new(FahSession(Mutex::new(Box::new(
            session,
        ))))))
    })
    .unwrap_or(ptr::null_mut())
}
/// # Safety
/// Session must be live; rgba must cover bytes bytes; error must be writable or null.
#[no_mangle]
pub unsafe extern "C" fn fah_apple_process(
    session: *mut FahSession,
    rgba: *const u8,
    bytes: usize,
    width: u32,
    height: u32,
    stride: usize,
    reset: i32,
    error: *mut *mut c_char,
) -> *mut c_char {
    boundary(error, || {
        ensure!(
            !session.is_null() && !rgba.is_null() && bytes <= isize::MAX as usize,
            "Invalid frame pointers"
        );
        let frame = Frame::from_rgba(
            std::slice::from_raw_parts(rgba, bytes),
            width,
            height,
            stride,
            Instant::now(),
        )?;
        let mut session = (*session)
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Session mutex poisoned"))?;
        let output = session.process(frame, reset != 0)?;
        Ok(owned(serde_json::to_string(&output.result)?))
    })
    .unwrap_or(ptr::null_mut())
}
/// # Safety
/// Pass a live session once, after all outstanding process calls have returned.
#[no_mangle]
pub unsafe extern "C" fn fah_apple_destroy(session: *mut FahSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}
/// # Safety
/// Pass a string returned by this library once, or null.
#[no_mangle]
pub unsafe extern "C" fn fah_apple_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(CString::from_raw(text));
    }
}

#[cfg(test)]
mod tests;
