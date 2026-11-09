"""Package the macOS camera viewer with its privacy description and Metal resources."""
import argparse
from pathlib import Path
import plistlib
import shutil
import subprocess

APPLE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", type=Path)
    parser.add_argument("--log", type=Path)
    args = parser.parse_args()
    subprocess.run(["swift", "build", "--package-path", str(APPLE), "-c", "release", "--product", "fah-apple-camera"], check=True)
    binary = Path(subprocess.check_output(["swift", "build", "--package-path", str(APPLE), "-c", "release", "--show-bin-path"], text=True).strip())
    app = APPLE / "build/FAH Camera.app"
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True, exist_ok=True)
    (contents / "Resources").mkdir(exist_ok=True)
    shutil.copy2(binary / "fah-apple-camera", contents / "MacOS/fah-apple-camera")
    for bundle in binary.glob("*.bundle"):
        shutil.copytree(bundle, contents / "Resources" / bundle.name, dirs_exist_ok=True)
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump(dict(CFBundleIdentifier="dev.fah.apple.camera", CFBundleName="FAH Camera",
            CFBundleExecutable="fah-apple-camera", CFBundlePackageType="APPL", CFBundleVersion="1",
            CFBundleShortVersionString="0.1.0", LSMinimumSystemVersion="15.0",
            NSCameraUsageDescription="Display camera frames with hand tracking on this Mac."), stream)
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
    print(app)
    if args.models:
        command = ["open", "-a", str(app), "--args", "--models", str(args.models.resolve())]
        if args.log:
            command += ["--log", str(args.log.resolve())]
        subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
