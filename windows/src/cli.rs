struct CliOptions {
    models: PathBuf,
    source: Option<Source>,
    backend: Backend,
    smoke: bool,
    frames: usize,
    output: PathBuf,
    sample_video: PathBuf,
}
use fah_api::{Backend, Command, Event, Options, Request, Source};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn parse(args: &[String]) -> anyhow::Result<CliOptions> {
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
    let mut result = CliOptions {
        sample_video: root.join("third_party/FAH/docs/assets/videos/test_video_selfie.mp4"),
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
pub fn run() -> anyhow::Result<()> {
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
            return crate::inference::onnx::check(&models, backend, runs, &fixture);
        }
        let mut check = vec![args[0].clone()];
        check.extend_from_slice(&args[2..]);
        let code = fah_ncnn::check::run(check).map_err(anyhow::Error::msg)?;
        if code != 0 {
            anyhow::bail!("Numerical validation failed ({code})");
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "--list-cameras") {
        println!(
            "{}",
            serde_json::to_string_pretty(&crate::platform::video::cameras()?)?
        );
        return Ok(());
    }
    let options = parse(&args)?;
    if options.smoke {
        return smoke(options);
    }
    crate::gui::run(
        Options {
            models: options.models,
            source: options.source,
            backend: options.backend,
            sample_video: options.sample_video,
        },
        crate::platform::controller(),
    )
}
fn smoke(options: CliOptions) -> anyhow::Result<()> {
    let worker = crate::platform::controller();
    let source = options
        .source
        .unwrap_or(Source::Image(options.models.join("sample.png")));
    let expected = if matches!(source, Source::Image(_)) {
        1
    } else {
        options.frames
    };
    worker.send(Command::Start(Request {
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
        while let Some(event) = worker.event() {
            match event {
                Event::Error(_, error) => anyhow::bail!(error),
                Event::Finished(_) => finished = true,
                _ => {}
            }
        }
        if let Some(output) = worker.frame() {
            worker
                .preview(&output, true, false)
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
    worker.send(Command::Stop)?;
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
