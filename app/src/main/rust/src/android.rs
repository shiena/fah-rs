use crate::{pipeline, Result};
use jni::{
    objects::{JObject, JString},
    sys::{jboolean, jobject, jstring, JNIEnv as RawEnv},
    JNIEnv,
};
use std::{ffi::c_void, ptr, sync::Mutex};
static PIPELINE: Mutex<Option<pipeline::Pipeline>> = Mutex::new(None);
#[repr(C)]
#[derive(Default)]
struct BitmapInfo {
    width: u32,
    height: u32,
    stride: u32,
    format: i32,
    flags: u32,
}
extern "C" {
    fn AndroidBitmap_getInfo(env: *mut RawEnv, bitmap: jobject, info: *mut BitmapInfo) -> i32;
    fn AndroidBitmap_lockPixels(env: *mut RawEnv, bitmap: jobject, pixels: *mut *mut c_void)
        -> i32;
    fn AndroidBitmap_unlockPixels(env: *mut RawEnv, bitmap: jobject) -> i32;
}
struct Pixels {
    env: *mut RawEnv,
    bitmap: jobject,
    data: *mut c_void,
}
impl Drop for Pixels {
    fn drop(&mut self) {
        unsafe {
            AndroidBitmap_unlockPixels(self.env, self.bitmap);
        }
    }
}
fn invoke(mut env: JNIEnv, f: impl FnOnce(&mut JNIEnv) -> Result<String>) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut env)));
    let value = result.unwrap_or_else(|_| Err("Native inference panicked".into()));
    match value {
        Ok(value) => match env.new_string(value) {
            Ok(s) => s.into_raw(),
            Err(e) => {
                let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
                ptr::null_mut()
            }
        },
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalStateException", e);
            ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_dev_fah_android_MainActivity_nativeInit(
    env: JNIEnv,
    _this: JObject,
    path: JString,
    gpu: jboolean,
) -> jstring {
    invoke(env, |env| {
        let directory: String = env.get_string(&path).map_err(|e| e.to_string())?.into();
        let mut state = PIPELINE.lock().map_err(|e| e.to_string())?;
        *state = Some(pipeline::create(&directory, gpu != 0, false)?);
        Ok(crate::runtime::gpu_info())
    })
}
#[no_mangle]
pub extern "system" fn Java_dev_fah_android_MainActivity_nativeProcess(
    env: JNIEnv,
    _this: JObject,
    bitmap: JObject,
    reset: jboolean,
) -> jstring {
    invoke(env, |env| {
        let mut state = PIPELINE.lock().map_err(|e| e.to_string())?;
        let pipeline = state.as_mut().ok_or("Models are not loaded")?;
        let raw = env.get_raw();
        let bitmap = bitmap.as_raw();
        let mut info = BitmapInfo::default();
        if unsafe { AndroidBitmap_getInfo(raw, bitmap, &mut info) } != 0 || info.format != 1 {
            return Err("Expected RGBA bitmap".into());
        }
        if info.width == 0
            || info.height == 0
            || info.width > 8192
            || info.height > 8192
            || info.stride < info.width * 4
        {
            return Err("Invalid bitmap dimensions".into());
        }
        let len = (info.stride as usize)
            .checked_mul(info.height as usize)
            .ok_or("Bitmap size overflow")?;
        let mut data = ptr::null_mut();
        if unsafe { AndroidBitmap_lockPixels(raw, bitmap, &mut data) } != 0 {
            return Err("Cannot lock bitmap".into());
        }
        let pixels = Pixels {
            env: raw,
            bitmap,
            data,
        };
        if pixels.data.is_null() {
            return Err("Null bitmap pixels".into());
        }
        let rgba = unsafe { std::slice::from_raw_parts(pixels.data.cast::<u8>(), len) };
        pipeline.process(
            rgba,
            info.width as usize,
            info.height as usize,
            info.stride as usize,
            reset != 0,
        )
    })
}
#[no_mangle]
pub extern "system" fn Java_dev_fah_android_MainActivity_nativeClose(
    mut env: JNIEnv,
    _this: JObject,
) {
    match PIPELINE.lock() {
        Ok(mut state) => *state = None,
        Err(_) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                "Native pipeline mutex poisoned",
            );
        }
    }
}
