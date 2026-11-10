"""Build the GUI-free engine library and collect its platform dependencies."""

import argparse
import importlib.util
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "engines/build"


def run(args, env=None):
    subprocess.run([str(x) for x in args], cwd=ROOT, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--platform",
        choices=["windows", "macos", "android", "linux"],
        default={"Windows": "windows", "Darwin": "macos", "Linux": "linux"}.get(
            platform.system()
        ),
    )
    parser.add_argument("--ndk", type=Path)
    parser.add_argument("--cmake", default="cmake")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument(
        "--litert-library",
        type=Path,
        help="Linux libLiteRt.so (tested with LiteRT 2.3.0)",
    )
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    if not args.platform:
        parser.error("Select a target platform")
    out = BUILD / args.platform
    out.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    target = []
    if args.platform == "android":
        ndk = args.ndk or Path(env.get("ANDROID_NDK_HOME", ""))
        if not (ndk / "build/cmake/android.toolchain.cmake").is_file():
            parser.error("Supply --ndk or ANDROID_NDK_HOME")
        ndk = ndk.resolve()
        native = BUILD / ("android-native-" + ndk.name)
        run(
            [
                args.cmake,
                "-S",
                ROOT / "engines/android",
                "-B",
                native,
                "-G",
                "Ninja",
                f"-DCMAKE_TOOLCHAIN_FILE={ndk}/build/cmake/android.toolchain.cmake",
                "-DANDROID_ABI=arm64-v8a",
                "-DANDROID_PLATFORM=android-26",
                "-DANDROID_STL=c++_shared",
                "-DCMAKE_BUILD_TYPE=Release",
            ]
        )
        run([args.cmake, "--build", native, "--parallel", args.jobs])
        host = {
            "Windows": "windows-x86_64",
            "Darwin": "darwin-x86_64",
            "Linux": "linux-x86_64",
        }[platform.system()]
        toolchain = ndk / "toolchains/llvm/prebuilt" / host
        linker = (
            toolchain
            / "bin"
            / (
                "aarch64-linux-android26-clang.cmd"
                if platform.system() == "Windows"
                else "aarch64-linux-android26-clang"
            )
        )
        env["CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"] = str(linker)
        env["FAH_NATIVE_DIR"] = str(native)
        target = ["--target", "aarch64-linux-android"]
        shutil.copy2(native / "libfah_backend.so", out)
        shutil.copy2(
            toolchain / "sysroot/usr/lib/aarch64-linux-android/libc++_shared.so", out
        )
    elif args.platform == "linux":
        if platform.system() != "Linux":
            parser.error("Build Linux on a Linux host or container")
        litert = args.litert_library
        if litert is None and env.get("FAH_LITERT_LIBRARY"):
            litert = Path(env["FAH_LITERT_LIBRARY"])
        if litert is None:
            package = importlib.util.find_spec("ai_edge_litert")
            if package and package.origin:
                litert = Path(package.origin).parent / "libLiteRt.so"
        if litert is None or not litert.is_file():
            parser.error(
                "Supply --litert-library /path/libLiteRt.so or install ai-edge-litert==2.3.0"
            )
        native = BUILD / "linux-native"
        run(
            [
                args.cmake,
                "-S",
                ROOT / "engines/linux",
                "-B",
                native,
                "-G",
                "Ninja",
                "-DCMAKE_BUILD_TYPE=Release",
            ]
        )
        run([args.cmake, "--build", native, "--parallel", args.jobs])
        env["FAH_NATIVE_DIR"] = str(native)
        shutil.copy2(native / "libfah_backend.so", out)
        if litert.resolve() != (out / "libLiteRt.so").resolve():
            shutil.copy2(litert, out / "libLiteRt.so")
        shutil.copy2(ROOT / "licenses/LITERT-APACHE-2.0.txt", out)
    elif args.platform == "windows":
        native = ROOT / "windows/native/build"
        if not (native / "fah_backend.lib").is_file():
            parser.error("Build windows/native with MSVC first (see windows/README.md)")
        env["FAH_NATIVE_DIR"] = str(native)
        shutil.copy2(native / "fah_backend.dll", out)
    else:
        if platform.system() != "Darwin":
            parser.error("Build macOS on a Mac")
        if not (ROOT / "apple/Artifacts/FAHCore.xcframework").exists():
            run([sys.executable, "apple/build.py", "--mac-only"])
        run(
            [
                "swift",
                "build",
                "--package-path",
                "apple",
                "-c",
                "release",
                "--product",
                "FAHAppleNative",
            ]
        )
        native = Path(
            subprocess.check_output(
                [
                    "swift",
                    "build",
                    "--package-path",
                    str(ROOT / "apple"),
                    "-c",
                    "release",
                    "--show-bin-path",
                ],
                text=True,
            ).strip()
        )
        shutil.copy2(native / "libFAHAppleNative.dylib", out)
        for bundle in native.glob("*.bundle"):
            shutil.copytree(bundle, out / bundle.name, dirs_exist_ok=True)
        env["CARGO_PROFILE_RELEASE_STRIP"] = "none"
        env["MACOSX_DEPLOYMENT_TARGET"] = "15.0"
    cargo_dir = BUILD / "rust"
    run(
        [
            "cargo",
            "build",
            "-p",
            "fah-ffi",
            "--release",
            "--locked",
            "--target-dir",
            cargo_dir,
            *target,
        ],
        env,
    )
    binary = {
        "windows": "fah_engine.dll",
        "macos": "libfah_engine.dylib",
        "android": "libfah_engine.so",
        "linux": "libfah_engine.so",
    }[args.platform]
    release = cargo_dir / ("aarch64-linux-android/release" if target else "release")
    shutil.copy2(release / binary, out)
    if args.platform == "windows":
        shutil.copy2(release / "fah_engine.dll.lib", out)
    if args.platform == "macos":
        for dylib in out.glob("*.dylib"):
            run(["codesign", "--force", "--sign", "-", dylib])
    shutil.copy2(ROOT / "engines/include/fah_engine.h", out)
    print(out)


if __name__ == "__main__":
    main()
