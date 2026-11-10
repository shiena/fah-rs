"""Run changing frames from FAH's published video through the Android pipeline."""

import argparse
import hashlib
import json
import math
import statistics
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", required=True)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--remote", default="/data/local/tmp/fah-android")
    parser.add_argument("--engine", help="Remote C ABI engine-check executable")
    parser.add_argument(
        "--backend", choices=["cpu", "vulkan", "auto"], default="vulkan"
    )
    args = parser.parse_args()
    if not args.engine and args.backend != "vulkan":
        parser.error("--engine is required for CPU or Auto validation")
    artifacts = ROOT / "artifacts"
    artifacts.mkdir(exist_ok=True)
    source = ROOT / "third_party/FAH/docs/assets/videos/test_video_selfie.mp4"
    raw = artifacts / "sample-video.rgba"
    # The upper-left panel contains the source image without prediction overlays.
    subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-ss",
            "5",
            "-i",
            str(source),
            "-vf",
            "crop=640:426:0:0,fps=5",
            "-frames:v",
            "30",
            "-pix_fmt",
            "rgba",
            "-f",
            "rawvideo",
            "-y",
            str(raw),
        ],
        check=True,
    )
    frame_bytes = 640 * 426 * 4
    data = raw.read_bytes()
    if len(data) != frame_bytes * 30:
        raise RuntimeError("Expected 30 video frames")
    distinct = len(
        {
            hashlib.sha256(data[i : i + frame_bytes]).digest()
            for i in range(0, len(data), frame_bytes)
        }
    )
    if distinct != 30:
        raise RuntimeError("Expected 30 distinct video frames")
    adb = [args.adb, "-s", args.serial]
    subprocess.run(
        [*adb, "push", str(raw), f"{args.remote}/sample-video.rgba"], check=True
    )
    prefix = f"engine-video-{args.backend}" if args.engine else "device-video"
    log = artifacts / f"{prefix}.log"
    with (
        log.open("w", encoding="utf-8") as output,
        log.with_suffix(".stderr.log").open("w", encoding="utf-8") as errors,
    ):
        command = (
            [
                args.engine,
                args.remote,
                f"{args.remote}/sample-video.rgba",
                "640",
                "426",
                {"cpu": "1", "vulkan": "2", "auto": "0"}[args.backend],
            ]
            if args.engine
            else [
                f"{args.remote}/fah-check",
                args.remote,
                "video",
                f"{args.remote}/sample-video.rgba",
                "640",
                "426",
            ]
        )
        subprocess.run(
            [*adb, "shell", f"LD_LIBRARY_PATH={args.remote}", *command],
            stdout=output,
            stderr=errors,
            check=True,
        )
    rows = [
        json.loads(line)
        for line in log.read_text(encoding="utf-8").splitlines()
        if line.startswith("{")
    ]
    if len(rows) != 30 or [r["frame"] for r in rows] != list(range(1, 31)):
        raise RuntimeError("Missing video results")
    expected = args.backend if args.backend != "auto" else rows[0]["backend"]
    if not all(r["backend"] == expected and not r["cpu_fallback"] for r in rows):
        raise RuntimeError("Unexpected video backend or runtime fallback")
    if args.engine:
        pose = "litert" if expected == "cpu" else "ncnn"
        if not all(
            r["pose_engine"] == pose and r["detector_engine"] == "ncnn" for r in rows
        ):
            raise RuntimeError("Unexpected inference engines")
    for row in rows:
        if len(row["hands"]) > 2:
            raise RuntimeError("Too many hands")
        for hand in row["hands"]:
            for name, dimensions in [("joints2d", 2), ("joints3d", 3)]:
                joints = hand[name]
                if len(joints) != 21 or any(len(p) != dimensions for p in joints):
                    raise RuntimeError("Invalid joint dimensions")
                if not all(
                    isinstance(v, (int, float)) and math.isfinite(v)
                    for p in joints
                    for v in p
                ):
                    raise RuntimeError("Non-finite joints")
    if not any(r["hands"] for r in rows) or not any(not r["detected"] for r in rows):
        raise RuntimeError("Video did not exercise hand tracking")
    if not all(rows[i]["detected"] for i in (0, 10, 20)):
        raise RuntimeError("Missing periodic detection")
    report = {
        "source": str(source.relative_to(ROOT)),
        "start_seconds": 5,
        "input_fps": 5,
        "crop": [0, 0, 640, 426],
        "frames": len(rows),
        "distinct_frames": distinct,
        "input_sha256": hashlib.sha256(data).hexdigest(),
        "backend": expected,
        "cpu_fallback": False,
        "frames_with_hands": sum(bool(r["hands"]) for r in rows),
        "hand_counts": [len(r["hands"]) for r in rows],
        "detection_frames": [r["frame"] for r in rows if r["detected"]],
        "median_ms": statistics.median(r["total_ms"] for r in rows),
        "passed": True,
    }
    if args.engine:
        report.update(
            pose_engine=rows[0]["pose_engine"],
            detector_engine=rows[0]["detector_engine"],
        )
    filename = f"{prefix}-validation.json" if args.engine else "video-validation.json"
    (artifacts / filename).write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
