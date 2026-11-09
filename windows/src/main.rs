#![windows_subsystem = "windows"]

mod media;
mod onnx;
use onnx::Backend;
mod worker;
use gpui::{
    App, Application, Bounds, Context, Div, ObjectFit, PathPromptOptions, RenderImage,
    SharedString, Stateful, TitlebarOptions, Window, WindowBounds, WindowOptions, div, img,
    prelude::*, px, rgb, size,
};
use media::Source;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use worker::{Command, Event, Output, Request, Worker};

struct Options {
    models: PathBuf,
    source: Option<Source>,
    backend: Backend,
    smoke: bool,
    frames: usize,
    output: PathBuf,
}
impl Options {
    fn parse(args: &[String]) -> anyhow::Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let adjacent = std::env::current_exe()?.parent().unwrap().join("models");
        let models = if adjacent.is_dir() {
            adjacent
        } else if PathBuf::from("models").is_dir() {
            PathBuf::from("models")
        } else {
            root.join("models")
        };
        let mut result = Self {
            models,
            source: None,
            backend: Backend::Auto,
            smoke: false,
            frames: 3,
            output: PathBuf::from("artifacts/windows-smoke"),
        };
        let mut i = 1;
        while i < args.len() {
            let value = |i: usize| {
                args.get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("Missing value for {}", args[i]))
            };
            match args[i].as_str() {
                "--models" => {
                    result.models = value(i)?.into();
                    i += 1;
                }
                "--input" => {
                    result.source = Some(Source::file(value(i)?.into()));
                    i += 1;
                }
                "--camera" => {
                    result.source = Some(Source::Camera(value(i)?.clone()));
                    i += 1;
                }
                "--sample" => result.source = Some(Source::Image(result.models.join("sample.png"))),
                "--sample-video" => {
                    result.source = Some(Source::SampleVideo(
                        root.join("third_party/FAH/docs/assets/videos/test_video_selfie.mp4"),
                    ))
                }
                "--cpu" => result.backend = Backend::Cpu,
                "--backend" => {
                    result.backend = Backend::parse(value(i)?)?;
                    i += 1;
                }
                "--smoke" => result.smoke = true,
                "--frames" => {
                    result.frames = value(i)?.parse()?;
                    i += 1;
                }
                "--output" => {
                    result.output = value(i)?.into();
                    i += 1;
                }
                option => anyhow::bail!("Unknown argument: {option}"),
            }
            i += 1;
        }
        if result.frames == 0 || result.frames > 1000 {
            anyhow::bail!("frames must be 1..1000");
        }
        Ok(result)
    }
}
fn main() {
    let result = run();
    // The GUI and worker owners have been dropped before Vulkan is shut down.
    unsafe {
        fah_rust::runtime::shutdown();
    }
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--help") {
        println!(
            "FAH Windows\n  --models DIRECTORY\n  --sample | --sample-video | --input FILE | --camera NAME\n  --backend auto|cuda|directml|cpu|vulkan\n  --cpu\n  --list-cameras\n  --check MODELS cpu|cuda|directml|gpu RUNS [FIXTURE]\n  --smoke --frames N --output DIRECTORY\nWithout input options, the GUI opens without activating a camera."
        );
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "--check") {
        if args
            .get(3)
            .is_some_and(|s| ["cuda", "directml", "cpu"].contains(&s.as_str()))
        {
            let models = PathBuf::from(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Missing models"))?,
            );
            let backend = Backend::parse(&args[3])?;
            let runs = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(5);
            let fixture = args.get(5).map(PathBuf::from).unwrap_or(models.clone());
            return onnx::check(&models, backend, runs, &fixture);
        }
        let mut check = vec![args[0].clone()];
        check.extend_from_slice(&args[2..]);
        let code = fah_rust::check::run(check).map_err(anyhow::Error::msg)?;
        if code != 0 {
            anyhow::bail!("Numerical validation failed ({code})");
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "--list-cameras") {
        println!("{}", serde_json::to_string_pretty(&media::cameras()?)?);
        return Ok(());
    }
    let options = Options::parse(&args)?;
    if options.smoke {
        return smoke(options);
    }
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1180.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(860.), px(620.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("FAH · Hand tracking".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Viewer::new(options, cx)),
        )
        .expect("Could not create GPUI window");
        cx.activate(true);
    });
    Ok(())
}
fn smoke(options: Options) -> anyhow::Result<()> {
    let worker = Worker::new();
    let source = options
        .source
        .unwrap_or(Source::Image(options.models.join("sample.png")));
    let expected = if matches!(source, Source::Image(_)) {
        1
    } else {
        options.frames
    };
    worker.commands.send(Command::Start(Request {
        id: 1,
        models: options.models,
        backend: options.backend,
        source,
    }))?;
    std::fs::create_dir_all(&options.output)?;
    let start = Instant::now();
    let mut rows = vec![];
    let mut finished = false;
    while rows.len() < expected && start.elapsed() < Duration::from_secs(180) {
        while let Ok(event) = worker.events.try_recv() {
            match event {
                Event::Error(_, error) => anyhow::bail!(error),
                Event::Finished(_) => finished = true,
                _ => {}
            }
        }
        if let Some(output) = worker.frame.lock().unwrap().take() {
            worker::overlay(&output.image, &output.result)
                .save(options.output.join("last-frame.png"))?;
            rows.push(output.result);
        }
        if finished && rows.len() < expected {
            anyhow::bail!(
                "Input ended after {} frames (expected {expected})",
                rows.len()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    worker.commands.send(Command::Stop)?;
    if rows.len() != expected {
        anyhow::bail!("Timed out waiting for {expected} frames");
    }
    std::fs::write(
        options.output.join("results.json"),
        serde_json::to_vec_pretty(&rows)?,
    )?;
    println!(
        "{}",
        serde_json::json!({"frames":rows.len(),"backend":rows.last().unwrap()["backend"],"hands":rows.last().unwrap()["hands"].as_array().map(Vec::len),"output":options.output})
    );
    Ok(())
}
struct Viewer {
    worker: Worker,
    models: PathBuf,
    backend: Backend,
    generation: u64,
    source: Option<Source>,
    running: bool,
    status: String,
    error: Option<String>,
    gpu_name: String,
    cameras: Vec<String>,
    selected_camera: usize,
    last: Option<Output>,
    image: Option<Arc<RenderImage>>,
    show_joints: bool,
    mirror_camera: bool,
    cuda_available: bool,
}
impl Viewer {
    fn new(options: Options, cx: &mut Context<Self>) -> Self {
        let worker = Worker::new();
        let _ = worker.commands.send(Command::RefreshCameras);
        let mut view = Self {
            worker,
            models: options.models,
            backend: options.backend,
            generation: 0,
            source: None,
            running: false,
            status: "入力を選択してください".into(),
            error: None,
            gpu_name: "未読み込み".into(),
            cameras: vec![],
            selected_camera: 0,
            last: None,
            image: None,
            show_joints: true,
            mirror_camera: false,
            cuda_available: onnx::cuda_libraries().is_ok(),
        };
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if view.update(cx, |view, cx| view.poll(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        if let Some(source) = options.source {
            view.start(source, cx);
        }
        view
    }
    fn start(&mut self, source: Source, cx: &mut Context<Self>) {
        self.generation += 1;
        self.running = true;
        self.error = None;
        self.last = None;
        self.image = None;
        self.status = "モデルと入力を準備中…".into();
        self.gpu_name = "準備中".into();
        self.source = Some(source.clone());
        if self
            .worker
            .commands
            .send(Command::Start(Request {
                id: self.generation,
                models: self.models.clone(),
                backend: self.backend,
                source,
            }))
            .is_err()
        {
            self.error = Some("推論ワーカーが終了しました".into());
            self.running = false;
        }
        cx.notify();
    }
    fn stop(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.running = false;
        self.status = "停止中".into();
        let _ = self.worker.commands.send(Command::Stop);
        cx.notify();
    }
    fn rebuild_image(&mut self) {
        if let Some(output) = &self.last {
            let mut rgba = if self.show_joints {
                worker::overlay(&output.image, &output.result)
            } else {
                output.image.clone()
            };
            if self.mirror_camera && matches!(self.source, Some(Source::Camera(_))) {
                image::imageops::flip_horizontal_in_place(&mut rgba);
            }
            for pixel in rgba.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            self.image = Some(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])));
        }
    }
    fn poll(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        let next = self.worker.frame.lock().unwrap().take();
        if let Some(output) = next.filter(|o| o.id == self.generation) {
            self.gpu_name = format!(
                "{} / {}",
                if self.backend == Backend::Vulkan {
                    "ncnn"
                } else {
                    "ONNX"
                },
                output.result["backend"].as_str().unwrap_or("unknown")
            );
            self.last = Some(output);
            self.rebuild_image();
            self.status = "推論中".into();
            changed = true;
        }
        while let Ok(event) = self.worker.events.try_recv() {
            changed = true;
            match event {
                Event::Loading(id) if id == self.generation => {
                    self.status = "モデルと入力を準備中…".into()
                }
                Event::Ready(id, info, warning) if id == self.generation => {
                    self.gpu_name = info;
                    if !warning.is_empty() {
                        eprintln!("{warning}");
                        self.error = Some(format!(
                            "自動選択: {}を使用します。先行候補を利用できませんでした。詳細は保存JSONのinitialization_warningに記録します。",
                            self.gpu_name
                        ));
                    }
                    self.status = "フレームを待機中…".into();
                }
                Event::Finished(id) if id == self.generation => {
                    self.running = false;
                    self.status = "完了".into();
                }
                Event::Error(id, error) if id == self.generation => {
                    self.running = false;
                    self.gpu_name = "推論停止".into();
                    self.status = "エラー".into();
                    self.error = Some(error);
                }
                Event::Cameras(result) => match result {
                    Ok(cameras) => {
                        self.selected_camera = cameras
                            .iter()
                            .position(|s| s.contains("Webcam"))
                            .unwrap_or(0);
                        self.cameras = cameras;
                    }
                    Err(error) => self.error = Some(error),
                },
                _ => {}
            }
        }
        if changed {
            cx.notify();
        }
    }
    fn choose_path(&mut self, directory: bool, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some(
                if directory {
                    "モデルフォルダーを選択"
                } else {
                    "画像・動画を選択"
                }
                .into(),
            ),
        });
        cx.spawn(async move |view, cx| match selection.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update(cx, |view, cx| {
                        if directory {
                            view.stop(cx);
                            view.models = path;
                            view.error = None;
                            cx.notify();
                        } else {
                            view.start(Source::file(path), cx);
                        }
                    });
                }
            }
            Ok(Err(error)) => {
                let _ = view.update(cx, |view, cx| {
                    view.error = Some(error.to_string());
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(output) = &self.last else { return };
        let image = worker::overlay(&output.image, &output.result);
        let json = output.result.clone();
        let selection = cx.prompt_for_new_path(&self.models, "fah-result.png".into());
        cx.spawn(async move |view, cx| {
            if let Ok(Ok(Some(path))) = selection.await {
                let result = (|| -> anyhow::Result<()> {
                    image.save_with_format(&path, image::ImageFormat::Png)?;
                    std::fs::write(
                        path.with_extension("json"),
                        serde_json::to_vec_pretty(&json)?,
                    )?;
                    Ok(())
                })();
                let _ = view.update(cx, |view, cx| {
                    match result {
                        Ok(()) => view.status = "画像とJSONを保存しました".into(),
                        Err(e) => view.error = Some(e.to_string()),
                    };
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .px_3()
        .py_2()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .bg(rgb(if active { 0x1b6170 } else { 0x202c3c }))
        .text_color(rgb(0xe5edf7))
        .hover(|s| s.bg(rgb(0x304459)))
        .child(label.into())
}
fn metric(label: &'static str, value: String) -> Div {
    div()
        .flex_1()
        .p_3()
        .rounded_lg()
        .bg(rgb(0x152131))
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(rgb(0x8d9fb5)).child(label))
        .child(div().text_lg().text_color(rgb(0xe5edf7)).child(value))
}
impl Render for Viewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let result = self.last.as_ref().map(|o| &o.result);
        let hands = result
            .and_then(|v| v["hands"].as_array())
            .map_or(0, Vec::len);
        let timing = result.and_then(|v| v["total_ms"].as_f64());
        let backend = result
            .and_then(|v| v["backend"].as_str())
            .unwrap_or(self.backend.name());
        let fallback = result.is_some_and(|v| v["cpu_fallback"] == true);
        let camera_buttons: Vec<_> = self
            .cameras
            .iter()
            .enumerate()
            .map(|(index, name)| {
                button(
                    format!("device-{index}"),
                    name.clone(),
                    index == self.selected_camera,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.selected_camera = index;
                    cx.notify();
                }))
            })
            .collect();
        let preview = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .rounded_lg()
            .bg(rgb(0x080f18))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .when_some(self.image.clone(), |div, image| {
                div.child(img(image).size_full().object_fit(ObjectFit::Contain))
            })
            .when(self.image.is_none(), |div| {
                div.child(
                    gpui::div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .items_center()
                        .child(
                            gpui::div()
                                .text_2xl()
                                .text_color(rgb(0x5dd7cc))
                                .child("FAH"),
                        )
                        .child("手の動きを、21の関節へ")
                        .child(
                            gpui::div()
                                .text_sm()
                                .text_color(rgb(0x8193aa))
                                .child("画像・動画を開くか、カメラを開始してください"),
                        ),
                )
            });
        div().size_full().flex().flex_col().bg(rgb(0x0d1623)).text_color(rgb(0xd9e4f3)).font_family("Yu Gothic UI")
            .child(div().h(px(76.)).flex_none().px_6().flex().items_center().justify_between().border_b_1().border_color(rgb(0x253347))
                .child(div().flex().flex_col().gap_1().child(div().text_xl().child("FAH  /  Hand tracking"))
                    .child(div().text_xs().text_color(rgb(0x8d9fb5)).child("WINDOWS · 21 JOINTS · 2 HANDS")))
                .child(div().text_sm().text_color(rgb(0x5dd7cc)).child(self.status.clone())))
            .child(div().flex_1().min_h_0().flex().gap_5().p_5()
                .child(div().w(px(254.)).flex_none().flex().flex_col().gap_3()
                    .child(div().text_xs().text_color(rgb(0x8d9fb5)).child("入力"))
                    .child(button("open","画像・動画を開く",false).on_click(cx.listener(|view,_,_,cx|view.choose_path(false,cx))))
                    .child(div().flex().gap_2()
                        .child(button("sample","サンプル画像",false).on_click(cx.listener(|view,_,_,cx|view.start(Source::Image(view.models.join("sample.png")),cx))))
                        .child(button("sample-video","動画",false).on_click(cx.listener(|view,_,_,cx|{
                            let path=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../third_party/FAH/docs/assets/videos/test_video_selfie.mp4");
                            view.start(Source::SampleVideo(path),cx);
                        }))))
                    .child(div().mt_3().text_xs().text_color(rgb(0x8d9fb5)).child("カメラ"))
                    .child(div().id("camera-list").max_h(px(190.)).overflow_y_scroll().flex().flex_col().gap_2().children(camera_buttons)
                        .when(self.cameras.is_empty(),|d|d.child("カメラが見つかりません")))
                    .child(div().flex().gap_2()
                        .child(button("camera",if self.running && matches!(self.source,Some(Source::Camera(_))) {"カメラ停止"} else {"カメラ開始"},true).on_click(cx.listener(|view,_,_,cx|{
                            if view.running && matches!(view.source,Some(Source::Camera(_))) {view.stop(cx);}
                            else if let Some(name)=view.cameras.get(view.selected_camera).cloned() {view.start(Source::Camera(name),cx);}
                            else {view.error=Some("使用できるカメラを選択してください".into());cx.notify();}
                        })))
                        .child(button("refresh","再検索",false).on_click(cx.listener(|view,_,_,_|{let _=view.worker.commands.send(Command::RefreshCameras);}))))
                    .child(button("mirror-camera",if self.mirror_camera {"左右反転：ON"} else {"左右反転：OFF"},self.mirror_camera)
                        .on_click(cx.listener(|view,_,_,cx|{
                            view.mirror_camera = !view.mirror_camera;
                            view.rebuild_image();
                            cx.notify();
                        })))
                    .child(div().mt_3().text_xs().text_color(rgb(0x8d9fb5)).child("推論"))
                    .child(div().flex().flex_wrap().gap_2().children([(Backend::Auto,"自動"),(Backend::Cuda,"CUDA"),(Backend::DirectMl,"DirectML"),(Backend::Cpu,"CPU"),(Backend::Vulkan,"Vulkan")].into_iter().map(|(backend,label)| {
                        let enabled = backend != Backend::Cuda || self.cuda_available;
                        button(label,if enabled {label} else {"CUDA（未導入）"},self.backend==backend)
                        .when(!enabled, |button| button.opacity(0.4).cursor_default())
                        .on_click(cx.listener(move |view,_,_,cx|{
                            if !enabled {return;}
                            view.backend=backend;if let Some(source)=view.source.clone() {view.start(source,cx);}else{cx.notify();}
                        }))
                    })))
                    .child(button("models","モデルフォルダー",false).on_click(cx.listener(|view,_,_,cx|view.choose_path(true,cx))))
                    .child(div().text_xs().text_color(rgb(0x8d9fb5)).child(self.models.display().to_string()))
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(rgb(0x8d9fb5)).child(self.gpu_name.clone()))
                    .child(div().text_xs().text_color(rgb(0x8d9fb5)).child("3D座標には推定カメラ内部パラメーターを使用します。")))
                .child(div().flex_1().min_w_0().flex().flex_col().gap_3()
                    .child(div().flex().items_center().justify_between()
                        .child(div().text_sm().child(self.source.as_ref().map(Source::label).unwrap_or("プレビュー".into())))
                        .child(div().text_xs().text_color(rgb(if fallback {0xffc857} else {0x5dd7cc})).child(if fallback {"CPU · GPUエラー後".into()} else {backend.to_uppercase()})))
                    .child(preview)
                    .child(div().flex().gap_3()
                        .child(metric("検出した手",format!("{hands} / 2")))
                        .child(metric("推論時間",timing.map(|ms|format!("{ms:.1} ms")).unwrap_or("—".into())))
                        .child(metric("推論FPS",timing.filter(|ms|*ms>0.).map(|ms|format!("{:.1}",1000./ms)).unwrap_or("—".into())))
                        .child(metric("取得→結果",self.last.as_ref().map(|o|format!("{:.0} ms",o.latency_ms)).unwrap_or("—".into()))))
                    .child(div().flex().gap_2()
                        .child(button("stop",if self.running {"停止"} else {"再開"},false).on_click(cx.listener(|view,_,_,cx|{if view.running {view.stop(cx);}else if let Some(source)=view.source.clone(){view.start(source,cx);}})))
                        .child(button("reset","再検出",false).on_click(cx.listener(|view,_,_,cx|{if view.running {let _=view.worker.commands.send(Command::Reset);}else if let Some(source)=view.source.clone(){view.start(source,cx);}})))
                        .child(button("overlay","関節を表示",self.show_joints).on_click(cx.listener(|view,_,_,cx|{view.show_joints = !view.show_joints;view.rebuild_image();cx.notify();})))
                        .child(button("save","結果を保存",false).on_click(cx.listener(|view,_,_,cx|view.save(cx)))))
                    .when_some(self.error.clone(),|d,error|d.child(div().id("runtime-message").max_h(px(100.)).overflow_y_scroll().p_3().rounded_md().bg(rgb(0x442832)).text_sm().text_color(rgb(0xffc0bd)).child(error)))))
    }
}
