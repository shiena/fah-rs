//! Validate the native LiteRT CPU ABI against exported FAH fixtures.
use anyhow::{ensure, Context, Result};
use fah_litert::{Model, TensorSpec};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
fn floats(path: PathBuf) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    ensure!(bytes.len() % 4 == 0, "Truncated float fixture");
    Ok(bytes
        .chunks(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() == 3, "Usage: pose_check LIBRARY MODELS");
    let library = Path::new(&args[1]);
    let root = Path::new(&args[2]);
    let inputs = [
        TensorSpec {
            name: "serving_default_args_0",
            shape: &[1, 3, 320, 320],
        },
        TensorSpec {
            name: "serving_default_args_1",
            shape: &[1, 400, 99],
        },
        TensorSpec {
            name: "serving_default_args_2",
            shape: &[1, 3],
        },
    ];
    let shapes: [&[i32]; 6] = [
        &[1, 461, 2],
        &[1, 461, 3],
        &[1, 461],
        &[1, 461],
        &[1, 1],
        &[1, 2],
    ];
    let names: Vec<_> = (0..6)
        .map(|i| format!("serving_default_output_{i}_output"))
        .collect();
    let outputs: Vec<_> = names
        .iter()
        .zip(shapes)
        .map(|(name, shape)| TensorSpec { name, shape })
        .collect();
    let path = root.join("litert/pose.tflite");
    ensure!(
        Model::open(&root.join("missing.so"), &path, &inputs, &outputs).is_err(),
        "Missing library accepted"
    );
    ensure!(
        Model::open(library, &root.join("missing.tflite"), &inputs, &outputs).is_err(),
        "Missing model accepted"
    );
    ensure!(
        Model::open(library, &path, &inputs[..2], &outputs).is_err(),
        "Bad signature accepted"
    );
    for pass in 0..2 {
        let mut model = Model::open(library, &path, &inputs, &outputs)?;
        ensure!(model.run(&[&[], &[], &[]]).is_err(), "Bad inputs accepted");
        let mut directories = vec![root.to_path_buf()];
        directories.extend(
            std::fs::read_dir(root.join("fixtures"))?
                .map(|p| p.map(|p| p.path()))
                .collect::<std::io::Result<Vec<_>>>()?,
        );
        for directory in directories {
            let data = [
                floats(directory.join("img.f32"))?,
                floats(directory.join("rays.f32"))?,
                floats(directory.join("cond.f32"))?,
            ];
            let tensors: Vec<_> = data.iter().map(Vec::as_slice).collect();
            for _ in 0..2 {
                model.run(&tensors)?;
            }
            let mut times = vec![];
            let mut actual = vec![];
            for _ in 0..10 {
                let start = Instant::now();
                actual = model.run(&tensors)?;
                times.push(start.elapsed().as_secs_f64() * 1000.);
            }
            times.sort_by(f64::total_cmp);
            let mut errors = vec![];
            for (i, values) in actual.iter().enumerate() {
                let reference = floats(directory.join(format!("reference{i}.f32")))?;
                ensure!(reference.len() == values.len(), "Invalid output length");
                let error = values
                    .iter()
                    .zip(reference)
                    .map(|(a, b)| {
                        assert!(a.is_finite() && b.is_finite());
                        (a - b).abs()
                    })
                    .fold(0f32, f32::max);
                ensure!(
                    !(i == 0 && error * 320. > 0.5 || i == 1 && error * 1000. > 1.),
                    "Fixture accuracy failed: {i}, {error}"
                );
                errors.push(error);
            }
            println!(
                "pass={pass} fixture={} median_ms={} max_errors={errors:?}",
                directory.file_name().unwrap().to_string_lossy(),
                (times[4] + times[5]) * 0.5
            );
        }
        // Exercise moving ownership to a worker and destroying all native resources there.
        std::thread::spawn(move || drop(model))
            .join()
            .map_err(|_| anyhow::anyhow!("Worker panicked"))
            .context("Move/drop model")?;
    }
    Ok(())
}
