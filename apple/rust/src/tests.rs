use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Context {
    drops: Arc<AtomicUsize>,
    fail: bool,
}
unsafe extern "C" fn detect(
    context: *mut c_void,
    _: *const f32,
    output: *mut f32,
    error: *mut c_char,
) -> i32 {
    if (*(context as *mut Context)).fail {
        ptr::copy_nonoverlapping(c"simulated Metal error".as_ptr(), error, 22);
        return -1;
    }
    std::slice::from_raw_parts_mut(output, DETECT_LEN).fill(0.);
    0
}
unsafe extern "C" fn pose(
    _: *mut c_void,
    _: *const f32,
    _: *const f32,
    _: *const f32,
    output: *mut f32,
    _: *mut c_char,
) -> i32 {
    // Distinct values prove the flattened six-output contract is unpacked correctly.
    let mut offset = 0;
    for (index, count) in [922, 1383, 461, 461, 1, 2].into_iter().enumerate() {
        std::slice::from_raw_parts_mut(output.add(offset), count).fill(index as f32);
        offset += count;
    }
    0
}
unsafe extern "C" fn destroy(context: *mut c_void) {
    let context = Box::from_raw(context as *mut Context);
    context.drops.fetch_add(1, Ordering::SeqCst);
}
fn callbacks(drops: Arc<AtomicUsize>, fail: bool) -> FahMetalCallbacks {
    FahMetalCallbacks {
        abi_version: 1,
        context: Box::into_raw(Box::new(Context { drops, fail })).cast(),
        detect: Some(detect),
        pose: Some(pose),
        destroy: Some(destroy),
    }
}
#[test]
fn ffi_session_processes_owned_padded_frames_and_releases_context() {
    let models = tempfile::tempdir().unwrap();
    fah_core::write_floats(models.path().join("uv_grid.f32"), &vec![0.; 800]).unwrap();
    let path = CString::new(models.path().to_str().unwrap()).unwrap();
    for fail in [false, true] {
        let drops = Arc::new(AtomicUsize::new(0));
        unsafe {
            let mut error = ptr::null_mut();
            let session =
                fah_apple_create(path.as_ptr(), callbacks(drops.clone(), fail), &mut error);
            assert!(!session.is_null() && error.is_null());
            let pixels = [0u8; 24];
            let result = fah_apple_process(
                session,
                pixels.as_ptr(),
                pixels.len(),
                2,
                2,
                12,
                1,
                &mut error,
            );
            if fail {
                assert!(result.is_null() && !error.is_null());
                // GPU errors cannot silently advertise successful CPU inference.
                assert!(CStr::from_ptr(error)
                    .to_str()
                    .unwrap()
                    .contains("CPU fallback"));
                fah_apple_string_free(error);
            } else {
                assert!(error.is_null());
                let value: serde_json::Value =
                    serde_json::from_slice(CStr::from_ptr(result).to_bytes()).unwrap();
                assert_eq!(value["backend"], "metal");
                assert_eq!(value["cpu_fallback"], false);
                assert_eq!(value["hands"].as_array().unwrap().len(), 0);
                fah_apple_string_free(result);
            }
            fah_apple_destroy(session);
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn initialization_failure_destroys_transferred_context() {
    let models = tempfile::tempdir().unwrap();
    let path = CString::new(models.path().to_str().unwrap()).unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    unsafe {
        let mut error = ptr::null_mut();
        assert!(
            fah_apple_create(path.as_ptr(), callbacks(drops.clone(), false), &mut error).is_null()
        );
        assert!(!error.is_null());
        fah_apple_string_free(error);
    }
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn desktop_adapter_releases_context_when_grid_is_invalid() {
    let drops = Arc::new(AtomicUsize::new(0));
    let result = unsafe { metal_runtime(callbacks(drops.clone(), false), vec![]) };
    assert!(result.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
fn grid() -> Vec<f32> {
    (0..400)
        .flat_map(|p| [(p % 20) as f32 * 16. + 7.5, (p / 20) as f32 * 16. + 7.5])
        .collect()
}
fn rays(grid: &[f32], transform: [f32; 4]) -> Vec<f32> {
    let mut values = vec![0.; 400 * 99];
    for p in 0..400 {
        values[p * 99] = grid[p * 2] * transform[0] + transform[1];
        values[p * 99 + 1] = grid[p * 2 + 1] * transform[2] + transform[3];
        values[p * 99 + 2] = 1.;
    }
    values
}
#[test]
fn camera_transform_uses_rays_independently_of_cliff_condition() {
    let grid = grid();
    for transform in [[0.0018, 0.12, 0.0022, -0.18], [-0.003, 1., 0.002, -0.4]] {
        let mut rays = rays(&grid, transform);
        let actual = camera(&rays, &grid).unwrap();
        for i in 0..4 {
            assert!((actual[i] - transform[i]).abs() < 1e-6);
        }
        rays[99] += 0.1;
        assert!(camera(&rays, &grid).is_err());
    }
}
#[test]
fn pose_callback_unpacking_preserves_all_six_outputs() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut runtime = Runtime(
        Arc::new(Mutex::new(Native(callbacks(drops.clone(), false)))),
        Arc::new(grid()),
    );
    let output = runtime
        .pose(
            &Tensor::new(320, 320, 3, vec![0.; 3 * 320 * 320]),
            &Tensor::new(99, 400, 1, rays(&grid(), [0.002, 0., 0.002, 0.])),
            &Tensor::new(3, 1, 1, vec![0., 0., 1.]),
        )
        .unwrap();
    assert_eq!(
        output.iter().map(|t| (t.w, t.h)).collect::<Vec<_>>(),
        [(2, 461), (3, 461), (1, 461), (1, 461), (1, 1), (2, 1)]
    );
    for (index, tensor) in output.iter().enumerate() {
        assert!(tensor.data.iter().all(|&v| v == index as f32));
    }
    drop(runtime);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
