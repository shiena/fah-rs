"""Compare FP32 pose inference through Linux ncnn and LiteRT CPU runtimes."""

import argparse
import ctypes as ct
import hashlib
import importlib.metadata
import json
import platform
import statistics
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
FLOAT = ct.POINTER(ct.c_float)
LAYOUTS = [("img", (1, 3, 320, 320)), ("rays", (1, 400, 99)), ("cond", (1, 3))]


class View(ct.Structure):
    _fields_ = [("data", FLOAT), ("w", ct.c_int), ("h", ct.c_int), ("c", ct.c_int)]


class Ncnn:
    def __init__(self, library, models, gpu):
        self.lib = ct.CDLL(str(library.resolve()))
        self.lib.fah_runtime_create.argtypes = [
            ct.c_char_p,
            ct.c_bool,
            ct.c_bool,
            ct.c_bool,
        ]
        self.lib.fah_runtime_create.restype = ct.c_void_p
        self.lib.fah_runtime_destroy.argtypes = [ct.c_void_p]
        self.lib.fah_runtime_destroy.restype = None
        self.lib.fah_shutdown.argtypes = []
        self.lib.fah_shutdown.restype = None
        self.lib.fah_error.restype = ct.c_char_p
        self.lib.fah_pose.argtypes = [
            ct.c_void_p,
            FLOAT,
            FLOAT,
            FLOAT,
            ct.POINTER(View),
        ]
        self.lib.fah_pose.restype = ct.c_bool
        self.handle = self.lib.fah_runtime_create(
            str(models.resolve()).encode(), gpu, False, False
        )
        if not self.handle:
            raise RuntimeError(self.lib.fah_error().decode())
        self.output = (View * 6)()

    def prepare(self, inputs):
        self.inputs = inputs
        self.pointers = [x.ctypes.data_as(FLOAT) for x in inputs]

    def invoke(self):
        if not self.lib.fah_pose(self.handle, *self.pointers, self.output):
            raise RuntimeError(self.lib.fah_error().decode())
        return [
            np.ctypeslib.as_array(x.data, shape=(x.w * x.h * x.c,)).copy()
            for x in self.output
        ]

    def close(self):
        self.lib.fah_runtime_destroy(self.handle)
        self.lib.fah_shutdown()


class LiteRT:
    def __init__(self, models):
        from ai_edge_litert.interpreter import Interpreter

        self.interpreter = Interpreter(
            model_path=str(models / "litert/pose.tflite"), num_threads=4
        )
        self.interpreter.allocate_tensors()
        self.manifest = json.loads((models / "litert/manifest.json").read_text())

    def prepare(self, inputs):
        self.inputs = inputs

    def invoke(self):
        # Include host input/output copies for both runtimes in the timing.
        for detail, layout in zip(
            self.interpreter.get_input_details(), self.manifest["inputs"]
        ):
            index = [name for name, _ in LAYOUTS].index(layout["fixture"])
            self.interpreter.set_tensor(detail["index"], self.inputs[index])
        self.interpreter.invoke()
        outputs = [None] * 6
        for detail, layout in zip(
            self.interpreter.get_output_details(), self.manifest["outputs"]
        ):
            outputs[layout["reference"]] = self.interpreter.get_tensor(
                detail["index"]
            ).reshape(-1)
        return outputs

    def close(self):
        del self.interpreter


def accuracy(outputs, directory):
    errors = []
    for index, values in enumerate(outputs):
        reference = np.fromfile(directory / f"reference{index}.f32", dtype=np.float32)
        if (
            values.shape != reference.shape
            or not np.isfinite(values).all()
            or not np.isfinite(reference).all()
        ):
            raise ValueError("Invalid output shape or non-finite output/reference")
        errors.append(float(np.max(np.abs(values - reference))))
    return {
        "max_abs_errors": errors,
        "passed": errors[0] * 320 <= 0.5 and errors[1] * 1000 <= 1.0,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--backend", choices=["ncnn_cpu", "ncnn_vulkan", "litert_cpu"], required=True
    )
    parser.add_argument("--models", type=Path, default=ROOT / "models")
    parser.add_argument(
        "--native", type=Path, default=ROOT / "engines/build/linux/libfah_backend.so"
    )
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--warmup-runs", type=int, default=2)
    parser.add_argument("--fixture", help="Only measure the named fixture directory")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.runs < 1 or args.rounds < 1 or args.warmup_runs < 0:
        parser.error("Runs and rounds must be positive; warmup must be nonnegative")
    files = (
        [args.models / "litert/pose.tflite"]
        if args.backend == "litert_cpu"
        else [
            args.native,
            args.models / "pose.ncnn.bin",
            args.models / "pose.ncnn.param",
        ]
    )
    report = {
        "backend": args.backend,
        "platform": platform.platform(),
        "python": platform.python_version(),
        "threads": 4,
        "warmup_runs_per_fixture": args.warmup_runs,
        "runs_per_round": args.runs,
        "rounds": args.rounds,
        "timing_scope": "Pose inference and host tensor copies; excludes initialization and preprocessing",
        "sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in files},
        "fixtures": {},
    }
    if args.backend == "litert_cpu":
        report["runtime_version"] = importlib.metadata.version("ai-edge-litert")
    start = time.perf_counter()
    runtime = (
        LiteRT(args.models)
        if args.backend == "litert_cpu"
        else Ncnn(args.native, args.models, args.backend == "ncnn_vulkan")
    )
    report["initialization_ms"] = (time.perf_counter() - start) * 1000
    try:
        directories = [args.models, *sorted((args.models / "fixtures").glob("*"))]
        if args.fixture:
            directories = [p for p in directories if p.name == args.fixture]
            if not directories:
                raise ValueError("Unknown fixture directory")
        for directory in directories:
            if not (directory / "img.f32").is_file():
                continue
            inputs = [
                np.fromfile(directory / f"{name}.f32", dtype=np.float32).reshape(shape)
                for name, shape in LAYOUTS
            ]
            runtime.prepare(inputs)
            for _ in range(args.warmup_runs):
                runtime.invoke()
            rounds = []
            for _ in range(args.rounds):
                durations = []
                for _ in range(args.runs):
                    start = time.perf_counter()
                    outputs = runtime.invoke()
                    durations.append((time.perf_counter() - start) * 1000)
                rounds.append(
                    {
                        "median_ms": statistics.median(durations),
                        "mean_ms": statistics.mean(durations),
                        "samples_ms": durations,
                    }
                )
            report["fixtures"][directory.name] = {
                **accuracy(outputs, directory),
                "rounds": rounds,
                "median_ms": statistics.median(
                    [x for r in rounds for x in r["samples_ms"]]
                ),
            }
            print(
                directory.name,
                report["fixtures"][directory.name]["median_ms"],
                report["fixtures"][directory.name]["passed"],
                flush=True,
            )
    finally:
        runtime.close()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    if not all(row["passed"] for row in report["fixtures"].values()):
        raise SystemExit(2)


if __name__ == "__main__":
    main()
