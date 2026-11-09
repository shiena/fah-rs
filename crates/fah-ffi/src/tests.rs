use super::*;
use fah_api::Output;
use serde_json::json;
struct Fake;
impl TrackingSession for Fake {
    fn process(&mut self, frame: Frame, reset: bool) -> Result<Output> {
        ensure!(
            frame.image.as_raw() == &[1, 2, 3, 255, 4, 5, 6, 255],
            "Incorrect copied pixels"
        );
        Ok(Output {
            id: 0,
            image: frame.image,
            latency_ms: 0.,
            result: json!({"width":1,"height":2,"backend":"cpu","frame":1,"detector_ms":0.,"pose_ms":1.,"total_ms":1.,"hands":[],"reset":reset}),
        })
    }
    fn backend(&self) -> Backend {
        Backend::Cpu
    }
    fn warning(&self) -> &str {
        ""
    }
}
#[test]
fn host_lifecycle_and_caller_owned_results() {
    let handle = id().unwrap();
    sessions()
        .lock()
        .unwrap()
        .insert(handle, Arc::new(Mutex::new(Box::new(Fake))));
    let pixels = [1, 2, 3, 255, 99, 99, 99, 99, 4, 5, 6, 255];
    let image = Image {
        data: pixels.as_ptr(),
        byte_len: pixels.len(),
        width: 1,
        height: 2,
        stride: 8,
        reset: 1,
    };
    let mut result = 0;
    assert_eq!(
        unsafe { fah_engine_process(handle, &image, &mut result) },
        OK
    );
    assert_eq!(fah_engine_shutdown(), ERROR);
    assert_eq!(fah_engine_destroy(handle), OK);
    assert_eq!(fah_engine_destroy(handle), ERROR);
    assert_eq!(fah_engine_shutdown(), ERROR);
    let mut data = ResultData::default();
    assert_eq!(
        unsafe { fah_engine_result_data(result, &mut data, size_of::<ResultData>()) },
        OK
    );
    assert_eq!((data.width, data.height, data.hand_count), (1, 2, 0));
    let mut needed = 0;
    assert_eq!(
        unsafe { fah_engine_result_json(result, ptr::null_mut(), 0, &mut needed) },
        BUFFER_TOO_SMALL
    );
    let mut bytes = vec![77u8; needed];
    assert_eq!(
        unsafe {
            fah_engine_result_json(result, bytes.as_mut_ptr().cast(), needed - 1, &mut needed)
        },
        BUFFER_TOO_SMALL
    );
    assert!(bytes.iter().all(|&b| b == 77));
    assert_eq!(
        unsafe {
            fah_engine_result_json(result, bytes.as_mut_ptr().cast(), bytes.len(), &mut needed)
        },
        OK
    );
    assert_eq!(bytes[needed - 1], 0);
    let value: serde_json::Value = serde_json::from_slice(&bytes[..needed - 1]).unwrap();
    assert_eq!(value["reset"], true);
    assert_eq!(fah_engine_result_destroy(result), OK);
    assert_eq!(
        unsafe { fah_engine_result_data(result, &mut data, size_of::<ResultData>()) },
        ERROR
    );
}
#[test]
fn invalid_host_arguments_and_thread_local_errors() {
    let mut handle = 123;
    assert_eq!(
        unsafe { fah_engine_create(ptr::null(), &mut handle) },
        ERROR
    );
    assert_eq!(handle, 0);
    let mut message = [0i8; 128];
    unsafe {
        fah_engine_last_error(message.as_mut_ptr(), message.len());
    }
    let error = unsafe { CStr::from_ptr(message.as_ptr()) }
        .to_str()
        .unwrap()
        .to_owned();
    std::thread::spawn(|| {
        assert_eq!(fah_engine_destroy(0), ERROR);
    })
    .join()
    .unwrap();
    unsafe {
        fah_engine_last_error(message.as_mut_ptr(), message.len());
    }
    assert_eq!(
        unsafe { CStr::from_ptr(message.as_ptr()) }
            .to_str()
            .unwrap(),
        error
    );
    let image = Image {
        data: ptr::dangling(),
        byte_len: 1,
        width: 8193,
        height: 2,
        stride: usize::MAX,
        reset: 0,
    };
    assert_eq!(unsafe { fah_engine_process(0, &image, &mut handle) }, ERROR);
    assert_eq!(handle, 0);
}
#[test]
fn c_structure_layout_is_stable_on_64_bit_hosts() {
    assert_eq!(size_of::<Hand>(), 436);
    assert_eq!(size_of::<ResultData>(), 928);
    if size_of::<usize>() == 8 {
        assert_eq!(size_of::<Config>(), 32);
        assert_eq!(size_of::<Image>(), 40);
    }
}
#[test]
fn typed_results_preserve_joint_layout_and_reject_bad_shapes() {
    let mut value = json!({"width":640,"height":480,"backend":"metal","frame":7,"detector_ms":2.,"pose_ms":3.,"total_ms":5.,"hands":[{"right":true,"score":0.9,"presence":0.8,"side":0.7,"joints2d":vec![[12.,34.];21],"joints3d":vec![[0.1,0.2,0.3];21]}]});
    let stored = store_output(value.clone()).unwrap();
    assert_eq!(stored.data.backend, 5);
    assert_eq!(stored.data.hand_count, 1);
    assert_eq!(stored.data.hands[0].joints2d[20], [12., 34.]);
    assert_eq!(stored.data.hands[0].joints3d[20], [0.1, 0.2, 0.3]);
    assert_eq!(stored.data.hands[1].right, 0);
    value["hands"][0]["joints2d"] = json!([[1., 2.]]);
    assert!(store_output(value).is_err());
}
#[test]
fn abi_mismatch_and_panics_do_not_escape_the_boundary() {
    let config = Config {
        abi_version: 999,
        struct_size: size_of::<Config>() as u32,
        models_utf8: ptr::null(),
        native_library_utf8: ptr::null(),
        backend: 0,
    };
    let mut handle = 42;
    assert_eq!(unsafe { fah_engine_create(&config, &mut handle) }, ERROR);
    assert_eq!(handle, 0);
    assert_eq!(
        boundary(|| -> Result<()> { panic!("Mock backend panic") }),
        ERROR
    );
    let mut error = [0i8; 128];
    unsafe {
        fah_engine_last_error(error.as_mut_ptr(), error.len());
    }
    assert!(
        unsafe { CStr::from_ptr(error.as_ptr()) }
            .to_str()
            .unwrap()
            .contains("Internal panic")
    );
}
