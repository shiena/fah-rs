use crate::Result;
#[derive(Clone, Debug)]
pub struct Tensor {
    pub w: usize,
    pub h: usize,
    pub c: usize,
    pub data: Vec<f32>,
}
impl Tensor {
    pub fn new(w: usize, h: usize, c: usize, data: Vec<f32>) -> Self {
        Self { w, h, c, data }
    }
}
pub trait Inference {
    fn detect(&mut self, image: &Tensor) -> Result<Tensor>;
    fn pose(&mut self, image: &Tensor, rays: &Tensor, condition: &Tensor) -> Result<Vec<Tensor>>;
}
#[cfg(target_os = "android")]
mod native {
    use super::*;
    use std::ffi::{c_char, c_void, CStr, CString};
    #[repr(C)]
    struct View {
        data: *const f32,
        w: i32,
        h: i32,
        c: i32,
    }
    extern "C" {
        fn fah_runtime_create(
            path: *const c_char,
            gpu: bool,
            half: bool,
            detector: bool,
        ) -> *mut c_void;
        fn fah_runtime_destroy(runtime: *mut c_void);
        fn fah_error() -> *const c_char;
        fn fah_gpu_info() -> *const c_char;
        fn fah_coverage(runtime: *mut c_void) -> *const c_char;
        fn fah_pose(
            runtime: *mut c_void,
            image: *const f32,
            rays: *const f32,
            condition: *const f32,
            outputs: *mut View,
        ) -> bool;
        fn fah_detect(runtime: *mut c_void, image: *const f32, output: *mut View) -> bool;
    }
    pub struct Runtime(*mut c_void);
    // The runtime is only accessed under exclusive ownership or the JNI mutex.
    unsafe impl Send for Runtime {}
    unsafe fn string(p: *const c_char) -> String {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
    fn error() -> String {
        unsafe { string(fah_error()) }
    }
    pub fn gpu_info() -> String {
        unsafe { string(fah_gpu_info()) }
    }
    impl Runtime {
        pub fn new(path: &str, gpu: bool, half: bool, detector: bool) -> Result<Self> {
            let path = CString::new(path).map_err(|e| e.to_string())?;
            let ptr = unsafe { fah_runtime_create(path.as_ptr(), gpu, half, detector) };
            if ptr.is_null() {
                Err(error())
            } else {
                Ok(Self(ptr))
            }
        }
        pub fn coverage(&self) -> String {
            unsafe { string(fah_coverage(self.0)) }
        }
    }
    impl Drop for Runtime {
        fn drop(&mut self) {
            unsafe { fah_runtime_destroy(self.0) }
        }
    }
    unsafe fn copy(v: &View) -> Result<Tensor> {
        if v.data.is_null() || v.w <= 0 || v.h <= 0 || v.c <= 0 {
            return Err("Invalid native tensor".into());
        }
        let n = (v.w as usize)
            .checked_mul(v.h as usize)
            .and_then(|n| n.checked_mul(v.c as usize))
            .ok_or("Tensor overflow")?;
        Ok(Tensor::new(
            v.w as usize,
            v.h as usize,
            v.c as usize,
            std::slice::from_raw_parts(v.data, n).to_vec(),
        ))
    }
    impl Inference for Runtime {
        fn detect(&mut self, image: &Tensor) -> Result<Tensor> {
            if image.data.len() != 640 * 640 * 3 {
                return Err("Invalid detector input".into());
            }
            let mut out = View {
                data: std::ptr::null(),
                w: 0,
                h: 0,
                c: 0,
            };
            if !unsafe { fah_detect(self.0, image.data.as_ptr(), &mut out) } {
                return Err(error());
            }
            unsafe { copy(&out) }
        }
        fn pose(
            &mut self,
            image: &Tensor,
            rays: &Tensor,
            condition: &Tensor,
        ) -> Result<Vec<Tensor>> {
            if image.data.len() != 320 * 320 * 3
                || rays.data.len() != 99 * 400
                || condition.data.len() != 3
            {
                return Err("Invalid pose input".into());
            }
            let mut out: [View; 6] = std::array::from_fn(|_| View {
                data: std::ptr::null(),
                w: 0,
                h: 0,
                c: 0,
            });
            if !unsafe {
                fah_pose(
                    self.0,
                    image.data.as_ptr(),
                    rays.data.as_ptr(),
                    condition.data.as_ptr(),
                    out.as_mut_ptr(),
                )
            } {
                return Err(error());
            }
            out.iter().map(|v| unsafe { copy(v) }).collect()
        }
    }
}
#[cfg(target_os = "android")]
pub use native::*;
