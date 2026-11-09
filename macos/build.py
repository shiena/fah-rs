"""Build and package the shared GPUI desktop app for Apple silicon macOS."""
import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
MAC = ROOT / "macos"
APPLE = ROOT / "apple"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", type=Path)
    parser.add_argument("--camera", action="store_true")
    parser.add_argument("--log", type=Path)
    parser.add_argument("--sample-video", type=Path)
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("Run on an Apple silicon Mac with Xcode")
    if not (APPLE / "Artifacts/FAHCore.xcframework").exists():
        subprocess.run([sys.executable, str(APPLE / "build.py"), "--mac-only"], check=True)
    subprocess.run(["swift", "build", "--package-path", str(APPLE), "-c", "release", "--product", "FAHAppleNative"], check=True)
    native = Path(subprocess.check_output(["swift", "build", "--package-path", str(APPLE), "-c", "release", "--show-bin-path"], text=True).strip())
    env = os.environ.copy()
    env["MACOSX_DEPLOYMENT_TARGET"] = "15.0"
    # Xcode 27 strip can produce misaligned LINKEDIT data in Rust proc macros.
    env["CARGO_PROFILE_RELEASE_STRIP"] = "none"
    subprocess.run(["cargo", "build", "-p", "fah-macos", "--release", "--locked", "--target-dir", str(MAC / "build/rust")], cwd=ROOT, env=env, check=True)
    app = MAC / "build/FAH.app"
    contents = app / "Contents"
    for folder in ["MacOS", "Resources", "Frameworks"]:
        (contents / folder).mkdir(parents=True, exist_ok=True)
    shutil.copy2(MAC / "build/rust/release/fah-macos", contents / "MacOS/fah-macos")
    shutil.copy2(native / "libFAHAppleNative.dylib", contents / "Frameworks/libFAHAppleNative.dylib")
    for bundle in native.glob("*.bundle"):
        shutil.copytree(bundle, contents / "Resources" / bundle.name, dirs_exist_ok=True)
    if args.models:
        defaults = {"models": str(args.models.resolve())}
        if args.sample_video:
            defaults["sample_video"] = str(args.sample_video.resolve())
        (contents / "Resources/defaults.json").write_text(json.dumps(defaults), encoding="utf-8")
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump(dict(CFBundleIdentifier="dev.fah.gpui", CFBundleName="FAH",
            CFBundleExecutable="fah-macos", CFBundlePackageType="APPL", CFBundleVersion="1",
            CFBundleShortVersionString="0.1.0", LSMinimumSystemVersion="15.0",
            NSHighResolutionCapable=True,
            NSCameraUsageDescription="Display camera frames with hand tracking on this Mac."), stream)
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
    print(app)
    if args.models:
        command = ["open", "-a", str(app), "--args", "--models", str(args.models.resolve())]
        if args.camera:
            command += ["--camera"]
        if args.log:
            args.log.parent.mkdir(parents=True, exist_ok=True)
            command += ["--log", str(args.log.resolve())]
        if args.sample_video:
            command += ["--sample-video", str(args.sample_video.resolve())]
        subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
