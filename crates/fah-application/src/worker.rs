use crate::{Session, overlay::overlay};
use fah_api::*;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};
pub struct Worker {
    commands: Sender<Command>,
    events: Receiver<Event>,
    frame: Arc<Mutex<Option<Output>>>,
    capabilities: Capabilities,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new(backends: Arc<dyn InferenceFactory>, inputs: Arc<dyn InputFactory>) -> Self {
        let capabilities = backends.capabilities();
        let (commands, rx) = mpsc::channel();
        let (tx, events) = mpsc::channel();
        let frame = Arc::new(Mutex::new(None));
        let slot = frame.clone();
        let thread = thread::spawn(move || run(rx, tx, slot, backends, inputs));
        Self {
            capabilities,
            commands,
            events,
            frame,
            thread: Some(thread),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn run(
    commands: Receiver<Command>,
    events: Sender<Event>,
    frame: Arc<Mutex<Option<Output>>>,
    backends: Arc<dyn InferenceFactory>,
    inputs: Arc<dyn InputFactory>,
) {
    let mut pipeline: Option<Session> = None;
    let mut loaded: Option<(PathBuf, Backend)> = None;
    let mut input: Option<Box<dyn FrameSource>> = None;
    let mut id = 0;
    let mut active_backend = Backend::Cpu;
    let mut warning = String::new();
    let mut reset = true;
    let mut last_frame = Instant::now();
    loop {
        match commands.recv_timeout(Duration::from_millis(5)) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Stop) => {
                input = None;
                *frame.lock().unwrap() = None;
            }
            Ok(Command::Reset) => reset = true,
            Ok(Command::RefreshCameras) => {
                let _ = events.send(Event::Cameras(
                    inputs.cameras().map_err(|e| format!("{e:#}")),
                ));
            }
            Ok(Command::Start(request)) => {
                input = None;
                id = request.id;
                reset = true;
                *frame.lock().unwrap() = None;
                let _ = events.send(Event::Loading(id));
                let result = (|| -> anyhow::Result<Box<dyn FrameSource>> {
                    let key = (request.models.clone(), request.backend);
                    if loaded.as_ref() != Some(&key) {
                        pipeline = None;
                        loaded = None;
                        let session =
                            Session::open(&request.models, request.backend, backends.clone())?;
                        active_backend = session.backend();
                        warning = session.warning().to_owned();
                        pipeline = Some(session);
                        loaded = Some(key);
                    }
                    inputs.open(&request.source)
                })();
                match result {
                    Ok(source) => {
                        input = Some(source);
                        last_frame = Instant::now();
                        let _ = events.send(Event::Ready(
                            id,
                            format!(
                                "{} / {}",
                                if active_backend == Backend::Vulkan {
                                    "ncnn"
                                } else {
                                    "ONNX"
                                },
                                active_backend.name()
                            ),
                            warning.clone(),
                        ));
                    }
                    Err(error) => {
                        let _ = events.send(Event::Error(id, format!("{error:#}")));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let Some(source) = input.as_mut() else {
            continue;
        };
        let result = (|| -> anyhow::Result<bool> {
            if let Some(input_frame) = source.next_frame()? {
                let mut output = pipeline.as_mut().unwrap().process(input_frame, reset)?;
                output.id = id;
                *frame.lock().unwrap() = Some(output);
                reset = false;
                last_frame = Instant::now();
            } else if last_frame.elapsed() > Duration::from_secs(20) {
                anyhow::bail!(
                    "入力フレームを取得できません。カメラの接続と他アプリでの使用状況を確認してください。"
                );
            }
            source.finished()
        })();
        match result {
            Ok(true) => {
                input = None;
                let _ = events.send(Event::Finished(id));
            }
            Ok(false) => {}
            Err(error) => {
                input = None;
                let _ = events.send(Event::Error(id, format!("{error:#}")));
            }
        }
    }
}

impl Controller for Worker {
    fn send(&self, command: Command) -> anyhow::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| anyhow::anyhow!("Inference worker has stopped"))
    }
    fn event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
    fn frame(&self) -> Option<Output> {
        self.frame.lock().unwrap().take()
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn preview(&self, output: &Output, joints: bool, mirror: bool) -> image::RgbaImage {
        let mut image = if joints {
            overlay(&output.image, &output.result)
        } else {
            output.image.clone()
        };
        if mirror {
            image::imageops::flip_horizontal_in_place(&mut image);
        }
        image
    }
    fn save(&self, path: &Path, output: &Output) -> anyhow::Result<()> {
        overlay(&output.image, &output.result).save_with_format(path, image::ImageFormat::Png)?;
        std::fs::write(
            path.with_extension("json"),
            serde_json::to_vec_pretty(&output.result)?,
        )?;
        Ok(())
    }
}
