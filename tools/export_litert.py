"""Export and validate the FP32 FAH pose model for LiteRT benchmarks (Linux)."""

import argparse
import json
import re
from pathlib import Path

import litert_torch
import numpy as np
import torch
from ai_edge_litert.interpreter import Interpreter
from export import ROOT, Pose

LAYOUTS = [("img", (1, 3, 320, 320)), ("rays", (1, 400, 99)), ("cond", (1, 3))]


def read_inputs(directory):
    return tuple(
        np.fromfile(directory / f"{name}.f32", dtype=np.float32).reshape(shape)
        for name, shape in LAYOUTS
    )


def compare(actual, directory):
    errors = []
    for index, values in enumerate(actual):
        reference = np.fromfile(
            directory / f"reference{index}.f32", dtype=np.float32
        ).reshape(values.shape)
        if not np.isfinite(values).all() or not np.isfinite(reference).all():
            raise ValueError("Non-finite inference output or reference")
        errors.append(float(np.max(np.abs(values - reference))))
    return {
        "max_abs_errors": errors,
        "passed": errors[0] * 320 <= 0.5 and errors[1] * 1000 <= 1.0,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "models/litert")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    torch.set_num_threads(8)
    model = Pose(ROOT / "models/model.pt").eval().requires_grad_(False)
    # LiteRT Torch 0.10.0 conflates these shared-storage views during conversion.
    # Materialize both constants to preserve distinct sine and cosine values.
    model.rope_sin = model.rope_sin.clone().contiguous()
    model.rope_cos = model.rope_cos.clone().contiguous()
    inputs = tuple(torch.from_numpy(value) for value in read_inputs(ROOT / "models"))
    converted = litert_torch.convert(model, inputs)
    path = args.output / "pose.tflite"
    converted.export(str(path))
    interpreter = Interpreter(model_path=str(path), num_threads=4)
    interpreter.allocate_tensors()
    manifest = {"inputs": [], "outputs": [], "host_validation": {}}
    for detail in interpreter.get_input_details():
        name, shape = next(
            (name, shape)
            for name, shape in LAYOUTS
            if list(shape) == detail["shape"].tolist()
        )
        manifest["inputs"].append(
            {"name": detail["name"], "fixture": name, "shape": list(shape)}
        )
    for detail in interpreter.get_output_details():
        index = int(re.search(r"output_(\d+)_", detail["name"]).group(1))
        manifest["outputs"].append(
            {
                "name": detail["name"],
                "reference": index,
                "shape": detail["shape"].tolist(),
            }
        )
    directories = [ROOT / "models", *(ROOT / "models/fixtures").glob("*")]
    for directory in directories:
        if not (directory / "img.f32").is_file():
            continue
        for detail, layout in zip(interpreter.get_input_details(), manifest["inputs"]):
            values = np.fromfile(
                directory / (layout["fixture"] + ".f32"), dtype=np.float32
            ).reshape(layout["shape"])
            interpreter.set_tensor(detail["index"], values)
        interpreter.invoke()
        actual = [None] * 6
        for detail, output in zip(
            interpreter.get_output_details(), manifest["outputs"]
        ):
            actual[output["reference"]] = interpreter.get_tensor(detail["index"])
        manifest["host_validation"][directory.name] = compare(actual, directory)
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest["host_validation"], indent=2))
    if not all(result["passed"] for result in manifest["host_validation"].values()):
        raise SystemExit("LiteRT output failed the existing pose accuracy thresholds")


if __name__ == "__main__":
    main()
