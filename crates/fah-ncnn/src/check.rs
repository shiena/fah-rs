use crate::{
    pipeline, read_floats,
    runtime::{Inference, Runtime, Tensor},
    write_floats, Result,
};
use serde_json::json;
use std::{
    ffi::{c_char, CStr},
    time::Instant,
};
fn number(args: &[String], index: usize, default: usize) -> Result<usize> {
    args.get(index)
        .map(|s| s.parse::<usize>().map_err(|e| e.to_string()))
        .unwrap_or(Ok(default))
}
pub fn run(args: Vec<String>) -> Result<i32> {
    let dir = args
        .get(1)
        .ok_or("Usage: fah-check MODELS [cpu|gpu|half|image|video] [runs]")?;
    let backend = args.get(2).map(String::as_str).unwrap_or("gpu");
    if backend == "image" || backend == "video" {
        let path = args
            .get(3)
            .ok_or("Usage: fah-check MODELS image|video RGBA WIDTH HEIGHT [runs]")?;
        let (w, h) = (number(&args, 4, 0)?, number(&args, 5, 0)?);
        if w == 0 || h == 0 || w > 8192 || h > 8192 {
            return Err("Invalid image dimensions".into());
        }
        let rgba = std::fs::read(path).map_err(|e| e.to_string())?;
        let frame_bytes = w * h * 4;
        if rgba.len() < frame_bytes || (backend == "video" && rgba.len() % frame_bytes != 0) {
            return Err("Invalid RGBA file".into());
        }
        let frames = if backend == "video" {
            rgba.len() / frame_bytes
        } else {
            1
        };
        let runs = number(&args, 6, frames)?;
        if !(1..=1000).contains(&runs) {
            return Err("runs must be 1..1000".into());
        }
        let mut pipeline = pipeline::create(dir, true, false)?;
        for i in 0..runs {
            let offset = (i % frames) * frame_bytes;
            println!(
                "{}",
                pipeline.process(&rgba[offset..offset + frame_bytes], w, h, w * 4, false)?
            );
        }
        return Ok(0);
    }
    if !["cpu", "gpu", "half"].contains(&backend) {
        return Err("Unknown backend".into());
    }
    let runs = number(&args, 3, 10)?;
    if !(1..=1000).contains(&runs) {
        return Err("runs must be 1..1000".into());
    }
    let fixture = args.get(4).unwrap_or(dir);
    eprintln!("GPU {}", crate::runtime::gpu_info());
    let mut net = Runtime::new(dir, backend != "cpu", backend == "half", false)?;
    eprintln!("COVERAGE {}", net.coverage());
    let im = Tensor::new(320, 320, 3, read_floats(format!("{fixture}/img.f32"))?);
    let ra = Tensor::new(99, 400, 1, read_floats(format!("{fixture}/rays.f32"))?);
    let co = Tensor::new(3, 1, 1, read_floats(format!("{fixture}/cond.f32"))?);
    let mut times = vec![];
    let mut outputs = vec![];
    let warmup = if std::env::var_os("FAH_TRACE").is_some() {
        0
    } else {
        -2
    };
    for i in warmup..runs as i32 {
        let start = Instant::now();
        outputs = net.pose(&im, &ra, &co)?;
        let ms = start.elapsed().as_secs_f64() * 1000.;
        eprintln!("run {i} {ms} ms");
        if i >= 0 {
            times.push(ms);
        }
    }
    times.sort_by(f64::total_cmp);
    let median = (times[(runs - 1) / 2] + times[runs / 2]) * 0.5;
    let mut passed = true;
    let mut stats = vec![];
    for (i, out) in outputs.iter().enumerate() {
        let reference = read_floats(format!("{fixture}/reference{i}.f32"))?;
        if reference.len() != out.data.len() || reference.is_empty() {
            return Err("Output size mismatch".into());
        }
        let (mut max, mut sum) = (0f64, 0f64);
        for (&a, &b) in out.data.iter().zip(&reference) {
            if !a.is_finite() || !b.is_finite() {
                return Err("Non-finite output or reference".into());
            }
            let d = (a - b).abs() as f64;
            max = max.max(d);
            sum += d;
        }
        if (i == 0 && max * 320. > 0.5) || (i == 1 && max * 1000. > 1.) {
            passed = false;
        }
        stats.push(json!({"index":i,"max":max,"mean":sum/reference.len() as f64}));
        write_floats(format!("{fixture}/actual{i}.f32"), &out.data)?;
    }
    println!(
        "{}",
        json!({"backend":backend,"runs":runs,"median_ms":median,"fps":1000./median,"outputs":stats,"passed":passed})
    );
    Ok(if passed { 0 } else { 2 })
}
/// Run the numerical check from a native command-line entry point.
///
/// # Safety
/// `argc` must be nonnegative and `argv` must contain that many valid pointers
/// to null-terminated strings, readable for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn fah_check_main(argc: i32, argv: *const *const c_char) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let args = (0..argc as usize)
            .map(|i| CStr::from_ptr(*argv.add(i)).to_string_lossy().into_owned())
            .collect();
        run(args)
    });
    match result {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => {
            eprintln!("{e}");
            1
        }
        Err(_) => {
            eprintln!("Native check panicked");
            1
        }
    }
}
