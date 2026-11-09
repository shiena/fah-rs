//! Stable, GUI-free host boundary. See engines/include/fah_engine.h for pointer contracts.
#![allow(clippy::missing_safety_doc)]
use anyhow::{Result, anyhow, ensure};
use fah_api::{Backend, Frame, TrackingSession};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{CStr, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    ptr,
    sync::{
        Arc, Mutex, OnceLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

const ABI: u32 = 1;
const OK: i32 = 0;
const ERROR: i32 = 1;
const BUFFER_TOO_SMALL: i32 = 2;
thread_local! { static ERROR_TEXT: RefCell<String> = const { RefCell::new(String::new()) }; }
type Session = Arc<Mutex<Box<dyn TrackingSession>>>;
static SESSIONS: OnceLock<Mutex<HashMap<u64, Session>>> = OnceLock::new();
static RESULTS: OnceLock<Mutex<HashMap<u64, StoredResult>>> = OnceLock::new();
static LIFECYCLE: RwLock<()> = RwLock::new(());
static NEXT: AtomicU64 = AtomicU64::new(1);
fn sessions() -> &'static Mutex<HashMap<u64, Session>> {
    SESSIONS.get_or_init(Default::default)
}
fn results() -> &'static Mutex<HashMap<u64, StoredResult>> {
    RESULTS.get_or_init(Default::default)
}
fn id() -> Result<u64> {
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| anyhow!("Handle space exhausted"))
}
fn boundary(f: impl FnOnce() -> Result<()>) -> i32 {
    let outcome = catch_unwind(AssertUnwindSafe(f));
    let message = match outcome {
        Ok(Ok(())) => {
            ERROR_TEXT.with(|s| s.borrow_mut().clear());
            return OK;
        }
        Ok(Err(e)) => format!("{e:#}"),
        Err(_) => "Internal panic; discard the affected session".into(),
    };
    ERROR_TEXT.with(|s| *s.borrow_mut() = message);
    ERROR
}
#[repr(C)]
pub struct Config {
    pub abi_version: u32,
    pub struct_size: u32,
    pub models_utf8: *const c_char,
    pub native_library_utf8: *const c_char,
    pub backend: u32,
}
#[repr(C)]
pub struct Image {
    pub data: *const u8,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub reset: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Hand {
    pub right: u32,
    pub score: f32,
    pub presence: f32,
    pub side: f32,
    pub joints2d: [[f32; 2]; 21],
    pub joints3d: [[f32; 3]; 21],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ResultData {
    pub abi_version: u32,
    pub struct_size: u32,
    pub width: u32,
    pub height: u32,
    pub backend: u32,
    pub hand_count: u32,
    pub frame: u64,
    pub detector_ms: f64,
    pub pose_ms: f64,
    pub total_ms: f64,
    pub hands: [Hand; 2],
}
struct StoredResult {
    data: ResultData,
    json: Vec<u8>,
}
fn backend(n: u32) -> Result<Backend> {
    Ok(match n {
        0 => Backend::Auto,
        1 => Backend::Cpu,
        2 => Backend::Vulkan,
        3 => Backend::Cuda,
        4 => Backend::DirectMl,
        5 => Backend::Metal,
        _ => return Err(anyhow!("Unknown backend")),
    })
}
fn backend_number(b: Backend) -> u32 {
    match b {
        Backend::Auto => 0,
        Backend::Cpu => 1,
        Backend::Vulkan => 2,
        Backend::Cuda => 3,
        Backend::DirectMl => 4,
        Backend::Metal => 5,
    }
}
unsafe fn string<'a>(p: *const c_char) -> Result<&'a str> {
    ensure!(!p.is_null(), "Missing UTF-8 string");
    Ok(unsafe { CStr::from_ptr(p) }.to_str()?)
}
fn open(config: &Config) -> Result<Box<dyn TrackingSession>> {
    let models = Path::new(unsafe { string(config.models_utf8)? });
    let selected = backend(config.backend)?;
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(fah_application::Session::open(
            models,
            selected,
            Arc::new(fah_windows::inference::Backends),
        )?))
    }
    #[cfg(target_os = "macos")]
    {
        let library = Path::new(unsafe { string(config.native_library_utf8)? });
        fah_apple_platform::session(fah_apple_platform::Native::load(library)?, models, selected)
    }
    #[cfg(target_os = "android")]
    {
        Ok(Box::new(fah_application::Session::open(
            models,
            selected,
            Arc::new(fah_ncnn::adapter::Backends),
        )?))
    }
    #[cfg(target_os = "ios")]
    {
        fah_apple_platform::session(fah_apple_platform::Native::linked(), models, selected)
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "android",
        target_os = "ios"
    )))]
    {
        let _ = (models, selected);
        anyhow::bail!("Platform adapter not enabled")
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn fah_engine_abi_version() -> u32 {
    ABI
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fah_engine_create(config: *const Config, out: *mut u64) -> i32 {
    boundary(|| {
        let _guard = LIFECYCLE
            .read()
            .map_err(|_| anyhow!("Lifecycle lock poisoned"))?;
        ensure!(!out.is_null(), "Missing output handle");
        unsafe {
            *out = 0;
        }
        ensure!(!config.is_null(), "Missing config");
        let config = unsafe { &*config };
        ensure!(
            config.abi_version == ABI && config.struct_size as usize == size_of::<Config>(),
            "Config ABI/size mismatch"
        );
        let session = open(config)?;
        let handle = id()?;
        sessions()
            .lock()
            .map_err(|_| anyhow!("Session registry poisoned"))?
            .insert(handle, Arc::new(Mutex::new(session)));
        unsafe {
            *out = handle;
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn fah_engine_destroy(handle: u64) -> i32 {
    boundary(|| {
        let _guard = LIFECYCLE
            .read()
            .map_err(|_| anyhow!("Lifecycle lock poisoned"))?;
        let session = sessions()
            .lock()
            .map_err(|_| anyhow!("Session registry poisoned"))?
            .remove(&handle)
            .ok_or_else(|| anyhow!("Invalid session handle"))?;
        drop(session);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fah_engine_process(
    handle: u64,
    image: *const Image,
    out: *mut u64,
) -> i32 {
    boundary(|| {
        let _guard = LIFECYCLE
            .read()
            .map_err(|_| anyhow!("Lifecycle lock poisoned"))?;
        ensure!(!out.is_null(), "Missing result handle");
        unsafe {
            *out = 0;
        }
        ensure!(!image.is_null(), "Missing image");
        let image = unsafe { &*image };
        ensure!(image.reset <= 1, "Invalid reset flag");
        ensure!(
            !image.data.is_null() && image.byte_len <= isize::MAX as usize,
            "Invalid image buffer"
        );
        // Validate dimensions/stride before constructing a slice from host memory.
        ensure!(
            (1..=8192).contains(&image.width) && (1..=8192).contains(&image.height),
            "Invalid image dimensions"
        );
        let row = image.width as usize * 4;
        ensure!(image.stride >= row, "Invalid image stride");
        let needed = image
            .stride
            .checked_mul(image.height as usize - 1)
            .and_then(|n| n.checked_add(row))
            .ok_or_else(|| anyhow!("Image size overflow"))?;
        ensure!(needed <= image.byte_len, "Image buffer too short");
        let session = sessions()
            .lock()
            .map_err(|_| anyhow!("Session registry poisoned"))?
            .get(&handle)
            .cloned()
            .ok_or_else(|| anyhow!("Invalid session handle"))?;
        let frame = Frame::from_rgba(
            unsafe { std::slice::from_raw_parts(image.data, needed) },
            image.width,
            image.height,
            image.stride,
            Instant::now(),
        )?;
        let output = session
            .lock()
            .map_err(|_| anyhow!("Session poisoned; destroy and recreate it"))?
            .process(frame, image.reset != 0)?;
        let result = store_output(output.result)?;
        let result_id = id()?;
        results()
            .lock()
            .map_err(|_| anyhow!("Result registry poisoned"))?
            .insert(result_id, result);
        unsafe {
            *out = result_id;
        }
        Ok(())
    })
}
fn store_output(v: serde_json::Value) -> Result<StoredResult> {
    let number = |k: &str| -> Result<f64> {
        v[k].as_f64()
            .filter(|x| x.is_finite())
            .ok_or_else(|| anyhow!("Invalid result field: {k}"))
    };
    let mut data = ResultData {
        abi_version: ABI,
        struct_size: size_of::<ResultData>() as u32,
        width: number("width")? as u32,
        height: number("height")? as u32,
        backend: backend_number(Backend::parse(v["backend"].as_str().unwrap_or(""))?),
        frame: v["frame"]
            .as_u64()
            .ok_or_else(|| anyhow!("Invalid frame index"))?,
        detector_ms: number("detector_ms")?,
        pose_ms: number("pose_ms")?,
        total_ms: number("total_ms")?,
        ..Default::default()
    };
    let hands = v["hands"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing hands"))?;
    ensure!(hands.len() <= 2, "Too many hands");
    data.hand_count = hands.len() as u32;
    for (out, hand) in data.hands.iter_mut().zip(hands) {
        out.right = u32::from(
            hand["right"]
                .as_bool()
                .ok_or_else(|| anyhow!("Invalid handedness"))?,
        );
        for (field, dst) in [
            ("score", &mut out.score),
            ("presence", &mut out.presence),
            ("side", &mut out.side),
        ] {
            *dst = hand[field]
                .as_f64()
                .ok_or_else(|| anyhow!("Missing hand score"))? as f32;
            ensure!(dst.is_finite(), "Invalid hand score");
        }
        out.joints2d = serde_json::from_value(hand["joints2d"].clone())?;
        out.joints3d = serde_json::from_value(hand["joints3d"].clone())?;
        ensure!(
            out.joints2d
                .iter()
                .flatten()
                .chain(out.joints3d.iter().flatten())
                .all(|v| v.is_finite()),
            "Invalid joints"
        );
    }
    let mut json = serde_json::to_vec(&v)?;
    json.push(0);
    Ok(StoredResult { data, json })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fah_engine_result_data(
    handle: u64,
    out: *mut ResultData,
    size: usize,
) -> i32 {
    boundary(|| {
        ensure!(
            !out.is_null() && size == size_of::<ResultData>(),
            "Result buffer size mismatch"
        );
        let results = results()
            .lock()
            .map_err(|_| anyhow!("Result registry poisoned"))?;
        let result = results
            .get(&handle)
            .ok_or_else(|| anyhow!("Invalid result handle"))?;
        unsafe {
            ptr::write(out, result.data);
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fah_engine_result_json(
    handle: u64,
    buffer: *mut c_char,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    let mut short = false;
    let status = boundary(|| {
        ensure!(!required.is_null(), "Missing required size");
        unsafe {
            *required = 0;
        }
        let results = results()
            .lock()
            .map_err(|_| anyhow!("Result registry poisoned"))?;
        let result = results
            .get(&handle)
            .ok_or_else(|| anyhow!("Invalid result handle"))?;
        unsafe {
            *required = result.json.len();
        }
        if capacity < result.json.len() {
            short = true;
            return Ok(());
        }
        ensure!(!buffer.is_null(), "Missing JSON buffer");
        unsafe {
            ptr::copy_nonoverlapping(result.json.as_ptr(), buffer.cast(), result.json.len());
        }
        Ok(())
    });
    if status == OK && short {
        BUFFER_TOO_SMALL
    } else {
        status
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn fah_engine_result_destroy(handle: u64) -> i32 {
    boundary(|| {
        results()
            .lock()
            .map_err(|_| anyhow!("Result registry poisoned"))?
            .remove(&handle)
            .ok_or_else(|| anyhow!("Invalid result handle"))?;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fah_engine_last_error(buffer: *mut c_char, capacity: usize) -> usize {
    ERROR_TEXT.with(|s| {
        let s = s.borrow();
        let needed = s.len() + 1;
        if !buffer.is_null() && capacity > 0 {
            let n = s.len().min(capacity - 1);
            unsafe {
                ptr::copy_nonoverlapping(s.as_ptr(), buffer.cast(), n);
                *buffer.add(n) = 0;
            }
        }
        needed
    })
}
#[cfg(test)]
mod tests;

/// Hosts must call this after joining all tasks and destroying all handles.
#[unsafe(no_mangle)]
pub extern "C" fn fah_engine_shutdown() -> i32 {
    boundary(|| {
        let _guard = LIFECYCLE
            .write()
            .map_err(|_| anyhow!("Lifecycle lock poisoned"))?;
        ensure!(
            sessions()
                .lock()
                .map_err(|_| anyhow!("Session registry poisoned"))?
                .is_empty(),
            "Destroy all sessions before shutdown"
        );
        ensure!(
            results()
                .lock()
                .map_err(|_| anyhow!("Result registry poisoned"))?
                .is_empty(),
            "Destroy all results before shutdown"
        );
        #[cfg(target_os = "windows")]
        unsafe {
            fah_windows::platform::shutdown();
        }
        Ok(())
    })
}
