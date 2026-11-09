"""Prepare upstream fah-metal exports for the shared Rust pipeline (no MANO needed at runtime)."""
import argparse
import json
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import tempfile


def prepare(source, output, compile_detector=True):
    source, output = Path(source).resolve(), Path(output).resolve()
    if source == output:
        raise ValueError("Use a separate output directory")
    metadata = json.loads((source / "fah-s-pose.json").read_text(encoding="utf-8"))
    grid = metadata["tensors"]["uv_ds"]
    if grid["dtype"] != "f32" or grid["shape"] != [400, 2]:
        raise ValueError("Expected a float32 400x2 camera grid")
    weights = (source / "fah-s-pose.bin").read_bytes()
    offset = grid["offset"]
    if not isinstance(offset, int) or offset < 0 or offset + 3200 > len(weights):
        raise ValueError("Invalid camera grid offset")
    # The shared core and Metal must use exactly the same antialiased ray grid.
    camera = weights[offset:offset + 3200]
    import math
    if not all(math.isfinite(v) for v in struct.unpack("<800f", camera)):
        raise ValueError("Non-finite camera grid")
    output.mkdir(parents=True, exist_ok=True)
    for name in ["fah-s-pose.json", "fah-s-pose.bin"]:
        shutil.copy2(source / name, output / name)
    (output / "uv_grid.f32").write_bytes(camera)
    for sample in ["sample.png", "sample.rgba"]:
        if (source / sample).is_file():
            shutil.copy2(source / sample, output / sample)
    if compile_detector:
        if platform.system() != "Darwin":
            raise RuntimeError("Core ML compilation requires macOS; use --skip-detector only for grid preparation")
        with tempfile.TemporaryDirectory() as temp:
            subprocess.run(["xcrun", "coremlcompiler", "compile", str(source / "detector-640.mlpackage"), temp], check=True)
            destination = output / "detector-640.mlmodelc"
            if destination.exists():
                shutil.rmtree(destination)
            shutil.copytree(Path(temp) / destination.name, destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Directory exported by upstream fah-metal/tools/export.py")
    parser.add_argument("output", type=Path)
    parser.add_argument("--skip-detector", action="store_true")
    args = parser.parse_args()
    prepare(args.source, args.output, not args.skip_detector)


if __name__ == "__main__":
    main()
