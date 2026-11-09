"""Build the shared Rust core as an Apple silicon XCFramework on a Mac."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
APPLE = ROOT / "apple"
TARGETS = {
    "aarch64-apple-darwin": "macosx",
    "aarch64-apple-ios": "iphoneos",
    "aarch64-apple-ios-sim": "iphonesimulator",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mac-only", action="store_true")
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("Run on an Apple silicon Mac with Xcode 26.2 or later")
    targets = dict(list(TARGETS.items())[:1]) if args.mac_only else TARGETS
    subprocess.run(["rustup", "target", "add", *targets], check=True)
    libraries = []
    for target, sdk in targets.items():
        env = os.environ.copy()
        env.update(SDKROOT=subprocess.check_output(["xcrun", "--sdk", sdk, "--show-sdk-path"], text=True).strip(),
                   MACOSX_DEPLOYMENT_TARGET="15.0", IPHONEOS_DEPLOYMENT_TARGET="18.0")
        subprocess.run(["cargo", "build", "-p", "fah-apple", "--release", "--locked", "--target", target,
                        "--target-dir", str(APPLE / "build/rust")], cwd=ROOT, env=env, check=True)
        libraries += ["-library", str(APPLE / "build/rust" / target / "release/libfah_apple.a"),
                      "-headers", str(APPLE / "include")]
    artifacts = APPLE / "Artifacts"
    artifacts.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=artifacts) as temporary:
        output = Path(temporary) / "FAHCore.xcframework"
        subprocess.run(["xcodebuild", "-create-xcframework", *libraries, "-output", str(output)], check=True)
        destination = artifacts / "FAHCore.xcframework"
        if destination.exists():
            shutil.rmtree(destination)
        shutil.move(str(output), destination)
    print(artifacts / "FAHCore.xcframework")


if __name__ == "__main__":
    main()
