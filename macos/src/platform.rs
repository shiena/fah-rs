use anyhow::{Context, Result, bail, ensure};
use fah_api::*;
use fah_apple::FahMetalCallbacks;
use fah_core::runtime::Tensor;
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr,
    sync::Arc,
    time::Instant,
};

type Create = unsafe extern "C" fn(*const c_char, *mut c_char) -> *mut c_void;
type Detect = unsafe extern "C" fn(*mut c_void, *const f32, *mut f32, *mut c_char) -> i32;
type Pose = unsafe extern "C" fn(
    *mut c_void,
    *const f32,
    *const f32,
    *const f32,
    *mut f32,
    *mut c_char,
) -> i32;
type Destroy = unsafe extern "C" fn(*mut c_void);
type Cameras = unsafe extern "C" fn(*mut c_char) -> *mut c_char;
type Open = unsafe extern "C" fn(i32, *const c_char, *mut c_char) -> *mut c_void;
type Read = unsafe extern "C" fn(
    *mut c_void,
    *mut *mut u8,
    *mut usize,
    *mut u32,
    *mut u32,
    *mut c_char,
) -> i32;

pub struct Native {
    create: Create,
    detect: Detect,
    pose: Pose,
    destroy: Destroy,
    cameras: Cameras,
    open: Open,
    read: Read,
    close: Destroy,
    free: Destroy,
    // Swift/Objective-C class registration outlives individual adapters.
    // Keep the module loaded until process exit, including queued capture teardown.
    _library: std::mem::ManuallyDrop<Library>,
}
impl Native {
    pub fn load(path: &Path) -> Result<Arc<Self>> {
        unsafe {
            let library =
                Library::new(path).with_context(|| format!("Cannot load {}", path.display()))?;
            let version: unsafe extern "C" fn() -> u32 = *library.get(b"fah_macos_abi_version")?;
            ensure!(version() == 1, "Unsupported Apple native ABI");
            Ok(Arc::new(Self {
                create: *library.get(b"fah_macos_runtime_create")?,
                detect: *library.get(b"fah_macos_detect")?,
                pose: *library.get(b"fah_macos_pose")?,
                destroy: *library.get(b"fah_macos_runtime_destroy")?,
                cameras: *library.get(b"fah_macos_cameras")?,
                open: *library.get(b"fah_macos_input_create")?,
                read: *library.get(b"fah_macos_input_read")?,
                close: *library.get(b"fah_macos_input_destroy")?,
                free: *library.get(b"fah_macos_free")?,
                _library: std::mem::ManuallyDrop::new(library),
            }))
        }
    }
}
fn error(buffer: &[c_char; 1024]) -> String {
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
struct OwnedRuntime {
    inner: Box<dyn Inference + Send>,
    _native: Arc<Native>,
}
impl Inference for OwnedRuntime {
    fn detect(&mut self, image: &Tensor) -> fah_core::Result<Tensor> {
        self.inner.detect(image)
    }
    fn pose(
        &mut self,
        image: &Tensor,
        rays: &Tensor,
        condition: &Tensor,
    ) -> fah_core::Result<Vec<Tensor>> {
        self.inner.pose(image, rays, condition)
    }
}
struct Backends(Arc<Native>);
impl InferenceFactory for Backends {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn automatic_backends(&self) -> &[Backend] {
        &[Backend::Metal]
    }
    fn create(&self, models: &Path, backend: Backend) -> Result<Box<dyn Inference + Send>> {
        ensure!(
            backend == Backend::Metal,
            "Apple adapter supports Metal only"
        );
        let grid = fah_core::read_floats(models.join("uv_grid.f32")).map_err(anyhow::Error::msg)?;
        let path = CString::new(models.to_string_lossy().as_bytes())?;
        let mut message = [0; 1024];
        let context = unsafe { (self.0.create)(path.as_ptr(), message.as_mut_ptr()) };
        ensure!(!context.is_null(), "{}", error(&message));
        let callbacks = FahMetalCallbacks {
            abi_version: 1,
            context,
            detect: Some(self.0.detect),
            pose: Some(self.0.pose),
            destroy: Some(self.0.destroy),
        };
        let inner = unsafe { fah_apple::metal_runtime(callbacks, grid) }?;
        Ok(Box::new(OwnedRuntime {
            inner,
            _native: self.0.clone(),
        }))
    }
}
struct Inputs(Arc<Native>);
impl InputFactory for Inputs {
    fn cameras(&self) -> Result<Vec<String>> {
        let mut message = [0; 1024];
        let pointer = unsafe { (self.0.cameras)(message.as_mut_ptr()) };
        ensure!(!pointer.is_null(), "{}", error(&message));
        let result = serde_json::from_slice(unsafe { CStr::from_ptr(pointer).to_bytes() });
        unsafe { (self.0.free)(pointer.cast()) };
        Ok(result?)
    }
    fn open(&self, source: &Source) -> Result<Box<dyn FrameSource>> {
        let (kind, path) = match source {
            Source::Image(path) => return Ok(Box::new(fah_input::StillImage::open(path)?)),
            Source::Camera(name) => (0, name.clone()),
            Source::Video(path) | Source::SampleVideo(path) => {
                (1, path.to_string_lossy().into_owned())
            }
        };
        let path = CString::new(path)?;
        let mut message = [0; 1024];
        let context = unsafe { (self.0.open)(kind, path.as_ptr(), message.as_mut_ptr()) };
        ensure!(!context.is_null(), "{}", error(&message));
        Ok(Box::new(Stream {
            context,
            native: self.0.clone(),
            ended: false,
        }))
    }
}
struct Stream {
    context: *mut c_void,
    native: Arc<Native>,
    ended: bool,
}
// Swift owns its capture queue and locks the single latest-frame slot.
unsafe impl Send for Stream {}
impl Drop for Stream {
    fn drop(&mut self) {
        unsafe { (self.native.close)(self.context) }
    }
}
impl FrameSource for Stream {
    fn next_frame(&mut self) -> Result<Option<Frame>> {
        let mut pointer = ptr::null_mut();
        let mut count = 0;
        let mut width = 0;
        let mut height = 0;
        let mut message = [0; 1024];
        let status = unsafe {
            (self.native.read)(
                self.context,
                &mut pointer,
                &mut count,
                &mut width,
                &mut height,
                message.as_mut_ptr(),
            )
        };
        match status {
            0 => Ok(None),
            2 => {
                self.ended = true;
                Ok(None)
            }
            1 => {
                ensure!(
                    !pointer.is_null() && count <= isize::MAX as usize,
                    "Invalid native frame"
                );
                let result = Frame::from_rgba(
                    unsafe { std::slice::from_raw_parts(pointer, count) },
                    width,
                    height,
                    width as usize * 4,
                    Instant::now(),
                );
                unsafe { (self.native.free)(pointer.cast()) };
                Ok(Some(result?))
            }
            _ => bail!("{}", error(&message)),
        }
    }
    fn finished(&mut self) -> Result<bool> {
        Ok(self.ended)
    }
}
pub fn controller(native: Arc<Native>) -> Client {
    Box::new(fah_application::Worker::new(
        Arc::new(Backends(native.clone())),
        Arc::new(Inputs(native)),
    ))
}
