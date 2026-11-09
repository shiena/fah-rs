"""Measure LiteRT CPU/GPU pose inference and validate outputs on an Android device."""

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", default="adb")
    parser.add_argument("--serial", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--backend", choices=["cpu", "gpu", "all"], default="all")
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts/litert-check")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    args.output.mkdir(parents=True, exist_ok=True)
    remote = "/data/local/tmp/fah-litert-check"

    def adb(*command, check=True):
        return subprocess.run(
            [args.adb, "-s", args.serial, *map(str, command)],
            capture_output=True,
            text=True,
            check=check,
        )

    adb("shell", "mkdir", "-p", remote)
    adb("push", args.binary, remote + "/benchmark")
    adb("shell", "chmod", "755", remote + "/benchmark")
    model = ROOT / "models/litert/pose.tflite"
    adb("push", model, remote + "/pose.tflite")
    manifest = json.loads((model.parent / "manifest.json").read_text())
    report = {
        "device": adb("shell", "getprop", "ro.product.model").stdout.strip(),
        "android": adb("shell", "getprop", "ro.build.version.release").stdout.strip(),
        "runtime": "Official nightly Interpreter API benchmark; XNNPACK or GPU delegate",
        "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "model_sha256": hashlib.sha256(model.read_bytes()).hexdigest(),
        "threads": 4,
        "warmup_runs": 2,
        "runs": args.runs,
        "results": {},
    }
    backends = ["cpu", "gpu"] if args.backend == "all" else [args.backend]
    directories = [ROOT / "models", *(ROOT / "models/fixtures").glob("*")]
    for directory in directories:
        if not (directory / "img.f32").is_file():
            continue
        for layout in manifest["inputs"]:
            adb(
                "push",
                directory / (layout["fixture"] + ".f32"),
                remote + "/" + layout["fixture"] + ".f32",
            )
        for backend in backends:
            key = directory.name + "-" + backend
            flags = [
                f"--graph={remote}/pose.tflite",
                "--num_threads=4",
                "--warmup_runs=2",
                "--warmup_min_secs=0",
                f"--num_runs={args.runs}",
                "--min_secs=0",
                "--input_layer=" + ",".join(x["name"] for x in manifest["inputs"]),
                "--input_layer_shape="
                + ":".join(",".join(map(str, x["shape"])) for x in manifest["inputs"]),
                "--input_layer_value_files="
                + ",".join(
                    x["name"].replace(":", "::")
                    + ":"
                    + remote
                    + "/"
                    + x["fixture"]
                    + ".f32"
                    for x in manifest["inputs"]
                ),
                f"--output_filepath={remote}/result.f32",
            ]
            flags += (
                ["--use_xnnpack=true"]
                if backend == "cpu"
                else ["--use_gpu=true", "--gpu_precision_loss_allowed=false"]
            )
            result = adb("shell", remote + "/benchmark", *flags, check=False)
            log = result.stdout + result.stderr
            (args.output / (key + ".log")).write_text(log)
            if result.returncode:
                report["results"][key] = {
                    "passed": False,
                    "exit_code": result.returncode,
                }
                continue
            output = args.output / (key + ".f32")
            adb("pull", remote + "/result.f32", output)
            data = np.fromfile(output, dtype=np.float32)
            offset, errors, finite = 0, [None] * 6, True
            for layout in manifest["outputs"]:
                index = layout["reference"]
                reference = np.fromfile(
                    directory / f"reference{index}.f32", dtype=np.float32
                )
                values = data[offset : offset + reference.size]
                if values.size != reference.size:
                    raise ValueError("Truncated benchmark output")
                offset += reference.size
                finite &= bool(
                    np.isfinite(values).all() and np.isfinite(reference).all()
                )
                errors[index] = float(np.max(np.abs(values - reference)))
            if offset != data.size:
                raise ValueError("Unexpected benchmark output size")
            medians = re.findall(r"\bmedian=([\d.]+)", log)
            means = re.findall(r"Inference \(avg\): ([\d.]+)", log)
            coverage = re.search(
                r"(\d+) operations will run on the GPU, and the remaining (\d+) operations will run on the CPU",
                log,
            )
            report["results"][key] = {
                "median_ms": float(medians[-1]) / 1000 if medians else None,
                "mean_ms": float(means[-1]) / 1000 if means else None,
                "max_abs_errors": errors,
                "finite": finite,
                "passed": finite and errors[0] * 320 <= 0.5 and errors[1] * 1000 <= 1,
                "gpu_cpu_operations": list(map(int, coverage.groups()))
                if coverage
                else None,
            }
            print(key, json.dumps(report["results"][key]), flush=True)
    (args.output / "validation.json").write_text(json.dumps(report, indent=2) + "\n")
    if not all(row["passed"] for row in report["results"].values()):
        raise SystemExit(2)


if __name__ == "__main__":
    main()
