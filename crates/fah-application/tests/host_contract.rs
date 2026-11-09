use anyhow::{Result, bail};
use fah_api::*;
use fah_application::{Session, Worker};
use fah_core::runtime::Tensor;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Detector {
    fail: bool,
}
impl Inference for Detector {
    fn detect(&mut self, _: &Tensor) -> fah_core::Result<Tensor> {
        if self.fail {
            return Err("simulated GPU execution failure".into());
        }
        Ok(Tensor::new(1, 6, 1, vec![0.; 6]))
    }
    fn pose(&mut self, _: &Tensor, _: &Tensor, _: &Tensor) -> fah_core::Result<Vec<Tensor>> {
        panic!("No hand should reach pose inference")
    }
}
struct Backends {
    cuda: bool,
    fail_gpu: bool,
    calls: Mutex<Vec<Backend>>,
}
impl InferenceFactory for Backends {
    fn capabilities(&self) -> Capabilities {
        Capabilities { cuda: self.cuda }
    }
    fn create(&self, _: &Path, backend: Backend) -> Result<Box<dyn Inference + Send>> {
        self.calls.lock().unwrap().push(backend);
        if backend == Backend::Cuda {
            bail!("simulated unavailable CUDA provider");
        }
        Ok(Box::new(Detector {
            fail: self.fail_gpu && backend != Backend::Cpu,
        }))
    }
}
fn models() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fah_core::write_floats(directory.path().join("uv_grid.f32"), &vec![0.; 800]).unwrap();
    directory
}
fn frame() -> Frame {
    Frame {
        image: image::RgbaImage::new(8, 8),
        captured: Instant::now(),
    }
}

#[test]
fn host_can_process_without_a_capture_service_or_worker() {
    let directory = models();
    let factory = Arc::new(Backends {
        cuda: true,
        fail_gpu: false,
        calls: Mutex::new(vec![]),
    });
    let mut session = Session::open(directory.path(), Backend::Auto, factory.clone()).unwrap();
    let result = session.process(frame(), true).unwrap();
    assert_eq!(result.result["backend"], "directml");
    assert_eq!(result.result["initialization_fallback"], true);
    assert!(
        result.result["initialization_warning"]
            .as_str()
            .unwrap()
            .contains("CUDA")
    );
    assert_eq!(
        *factory.calls.lock().unwrap(),
        vec![Backend::Cuda, Backend::DirectMl]
    );
}

#[test]
fn unavailable_cuda_is_skipped_and_runtime_failure_reports_cpu() {
    let directory = models();
    let factory = Arc::new(Backends {
        cuda: false,
        fail_gpu: true,
        calls: Mutex::new(vec![]),
    });
    let mut session = Session::open(directory.path(), Backend::Auto, factory.clone()).unwrap();
    let result = session.process(frame(), true).unwrap();
    assert_eq!(session.backend(), Backend::Cpu);
    assert_eq!(result.result["backend"], "cpu");
    assert_eq!(result.result["cpu_fallback"], true);
    assert_eq!(result.result["initialization_fallback"], false);
    assert_eq!(
        *factory.calls.lock().unwrap(),
        vec![Backend::DirectMl, Backend::Cpu]
    );
}

struct Input {
    pending: bool,
    dropped: Arc<AtomicUsize>,
}
impl Drop for Input {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl FrameSource for Input {
    fn next_frame(&mut self) -> Result<Option<Frame>> {
        Ok(if std::mem::take(&mut self.pending) {
            Some(frame())
        } else {
            None
        })
    }
    fn finished(&mut self) -> Result<bool> {
        Ok(false)
    }
}
struct Inputs(Arc<AtomicUsize>);
impl InputFactory for Inputs {
    fn cameras(&self) -> Result<Vec<String>> {
        Ok(vec!["host camera".into()])
    }
    fn open(&self, _: &Source) -> Result<Box<dyn FrameSource>> {
        Ok(Box::new(Input {
            pending: true,
            dropped: self.0.clone(),
        }))
    }
}
fn wait_until(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Worker did not respond"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn desktop_stop_and_shutdown_release_injected_input() {
    let directory = models();
    let dropped = Arc::new(AtomicUsize::new(0));
    let worker = Worker::new(
        Arc::new(Backends {
            cuda: false,
            fail_gpu: false,
            calls: Mutex::new(vec![]),
        }),
        Arc::new(Inputs(dropped.clone())),
    );
    for id in 1..=2 {
        worker
            .send(Command::Start(Request {
                id,
                models: directory.path().into(),
                backend: Backend::Cpu,
                source: Source::Camera("host camera".into()),
            }))
            .unwrap();
        wait_until(|| worker.frame().is_some_and(|output| output.id == id));
        if id == 1 {
            worker.send(Command::Stop).unwrap();
            wait_until(|| dropped.load(Ordering::SeqCst) == 1);
            assert!(worker.frame().is_none());
        }
    }
    drop(worker);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}
