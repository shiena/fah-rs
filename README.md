# fah-rs

[日本語](README_ja.md)

Cross-platform FAH-S hand tracking with a shared Rust core, native inference backends, and GPUI desktop apps.
This project builds on [FAH](https://github.com/noahcao/FAH) and [fah-metal](https://github.com/LuckyIYI/fah-metal).

**Model weights are restricted to research and noncommercial use.** Converted models retain these restrictions. See [License and model terms](#license-and-model-terms).

## Supported platforms

| Platform | App or integration | Inference | Validation scope |
|---|---|---|---|
| Windows 11 x64 | GPUI app; C ABI | ONNX Runtime CUDA / DirectML / CPU; ncnn Vulkan | Images, videos, USB cameras; CUDA inference unverified because cuDNN was missing |
| Android arm64, API 26+ | Android app; C ABI | ncnn Vulkan; LiteRT CPU pose + ncnn CPU detector | Device image, video, and camera checks |
| macOS 15+, Apple silicon | GPUI app; C ABI | Metal pose + Core ML detector | Apple M3 image, video, and camera checks |
| iOS 18+, arm64 | Native library; C ABI validation app | Metal pose + Core ML detector | iPhone 15 Pro sample-image inference; camera unverified |
| Linux x86_64 | C ABI; command-line example | ncnn Vulkan; LiteRT CPU pose + ncnn CPU detector | Steam Deck sample-video inference |

Windows Auto tries CUDA, DirectML, then CPU. CUDA is enabled only when its required libraries are present.
Android and Linux Auto prefer Vulkan, then CPU. Apple has no CPU fallback.
Linux has no GPUI app or camera adapter. The iOS validation app isn't a GPUI app.

## Features and limits

- Returns 21 joints per hand in 2D and 3D, for up to two hands.
- Shares cropping, coordinate restoration, tracking, and redetection every 10 processed frames.
- Separates GUI, API, application, tracking core, and platform layers, with independent frame-input adapters.
- Provides a GUI-free C ABI and C/C# examples for Unity or Godot hosts. Engine-specific plugin packages are not implemented.

3D coordinates use camera space in meters. Intrinsics are estimated from image dimensions, not calibrated camera data.
Meshes, MANO rotations, contact estimation, video correction, handedness reassessment, boundary recropping, and smoothing are not implemented.
Performance depends on the device and backend. This port doesn't reproduce the complete upstream pipeline or its reported FPS.

## Prepare models

The tested checkpoint preparation workflow uses Windows, PowerShell, Git, and Python 3.12.
From the repository root, run:

```powershell
.\tools\setup.ps1
```

This creates `.venv`, checks out pinned FAH source, downloads checksum-verified weights, and exports ncnn models and fixtures.
It also downloads Android ncnn libraries and writes `local.properties`. It doesn't install the Android SDK; Windows desktop builds don't require that SDK.

| Target | Additional model preparation |
|---|---|
| Windows ONNX Runtime | `tools/export_onnx.py`; see the [Windows guide](windows/README.md) |
| Android / Linux Vulkan | ncnn files from `tools/setup.ps1` |
| macOS / iOS | `tools/export_apple.py`, then conversion on a Mac; see the [Apple guide](apple/README.md#モデルを準備する) (Japanese) |
| Android / Linux CPU | `tools/export_litert.py` in a dedicated environment; see the [LiteRT guide](docs/litert-performance.md#wslで変換する) (Japanese) |

Copy prepared models to the target platform. These formats aren't interchangeable.
The configured exporters don't require an additional MANO or SAM 3D Body download. FAH weight restrictions still apply.
`models/` and `third_party/` are excluded from Git.

## Build and run

Run commands from the repository root. Platform guides contain runtime installation, model layout, and validation details.
Building the default Cargo target alone doesn't prepare native dependencies or models.

### Windows

Install Rust x64 MSVC, Visual Studio's C++ desktop workload, CMake 3.22+, and Ninja.
Install the [ONNX Runtime DLLs](windows/README.md#install-inference-runtimes). CUDA additionally needs CUDA 13.x and cuDNN 9 for CUDA 13.
Video and camera input require FFmpeg with DirectShow support.

```powershell
.venv/Scripts/python.exe -m pip install -r requirements-onnx.txt
.venv/Scripts/python.exe tools/export_onnx.py
.\tools\windows.ps1 build
.\tools\windows.ps1 run
```

Output: `windows/target/release/fah-windows.exe`. See the [Windows guide](windows/README.md).

### Android

Install JDK 17, Rust 1.87+, Android SDK platform 36, NDK 30.0.16248370, and CMake 3.22.1.
Enable USB debugging and authorize the device.

```powershell
rustup target add aarch64-linux-android
.\tools\device.ps1 -Action install
.\tools\device.ps1 -Action check
```

The install command builds the APK and transfers models separately. The APK alone isn't sufficient.
See the [Android setup, controls, and video validation guide](app/README.md).
CPU inference uses LiteRT 2.3.0 for pose estimation and ncnn for detection. Prepare `models/litert/pose.tflite` before installation.

### macOS

Use an Apple silicon Mac with macOS 15+, Xcode 26.2+, Rust, and Python 3.
Prepare [Apple models](apple/README.md#モデルを準備する), then run on the Mac:

```sh
xcodebuild -downloadComponent MetalToolchain
python3 macos/build.py --models models/apple
```

This builds and launches `macos/build/FAH.app`. Add `--camera` to start capture.
See the [GPUI app guide](macos/README.md) (Japanese).

### iOS

Use a Mac with Xcode 26.2+, Rust, and Python 3. Prepare the Apple models first.

```sh
python3 engines/ios/build.py
```

Output: `engines/build/ios/libfah_engine.a`. Hosts also need the Swift adapter and shader resources.
Add `--team YOUR_APPLE_TEAM_ID` to build the signed device validation app.
See the [iOS device guide](engines/README.md#iosでビルド検証する) and [Swift package guide](apple/README.md) (Japanese).

### Linux

Use Linux or WSL with Rust, Python 3, GCC or Clang, CMake 3.22+, Ninja, and Vulkan development headers.
Provide LiteRT 2.3.0 through the build Python environment (`ai-edge-litert==2.3.0`) or an explicit library path.

```sh
git clone --branch 20261020 --depth 1 --recurse-submodules --shallow-submodules https://github.com/Tencent/ncnn.git third_party/ncnn-src
python3 engines/build.py --platform linux --litert-library /absolute/path/libLiteRt.so
```

Skip cloning if the pinned checkout exists. Omit `--litert-library` when the build environment has the Python package.
Output: `engines/build/linux/`, containing `libfah_engine.so`, `libfah_backend.so`, `libLiteRt.so`, and the LiteRT license.
Runtime inference doesn't require Python. CPU inference needs `models/litert/pose.tflite`.
See the [Linux build and video validation guide](engines/README.md#linuxでビルドする) (Japanese).

## Integrate and validate

The [C ABI guide](engines/README.md) covers packaging, lifecycle, coordinates, and host responsibilities.
Hosts supply RGBA8 frames and manage capture, texture readback, and threading.
See the [C example](engines/examples/check.c) and [C# wrapper](engines/examples/FahSession.cs).
Actual Unity Editor, IL2CPP, and Godot plugin integration remain unvalidated.

```sh
cargo test -p fah-core -p fah-api -p fah-application -p fah-input --release --locked
```

This tests shared Rust layers without a GUI. Native inference checks are described in each platform guide.
Recorded results include [Android](docs/rust-validation.json), [Windows](docs/windows-onnx-validation.json), [macOS](docs/macos-gpui-validation.json), and [Linux LiteRT](engines/linux/litert-validation.json).
Parity and video smoke tests don't measure accuracy against ground-truth labels.

## Project structure

| Path | Responsibility |
|---|---|
| `crates/fah-gui/` | Shared Windows/macOS GPUI interface |
| `crates/fah-api/` | Common frame, inference, and tracking contracts |
| `crates/fah-application/` | Sessions and desktop worker scheduling |
| `crates/fah-core/` | Preprocessing, postprocessing, and tracking |
| `crates/fah-input/` | Shared still-image input adapter |
| `crates/fah-ncnn/`, `backends/ncnn/cpp/` | Rust ncnn adapter and C++/Vulkan runtime |
| `crates/fah-litert/`, `crates/fah-ncnn-platform/`, `crates/fah-linux-platform/` | LiteRT C API wrapper, shared Android/Linux backend selection, and Linux entry point |
| `crates/fah-apple-platform/`, `apple/` | Rust Apple adapter, Swift/Metal/Core ML implementation, vendored source |
| `crates/fah-ffi/`, `engines/` | C ABI, headers, host examples, and packaging |
| `windows/` | Windows entry point, ONNX inference, platform services, and native build |
| `macos/` | macOS GPUI entry point and app bundle build |
| `app/` | Android UI, Rust JNI bridge, and Gradle/CMake build |
| `tools/`, `docs/` | Exporters, setup, architecture, and validation records |
| `licenses/`, `NOTICE` | Third-party notices and model terms |
| `models/`, `third_party/`, `artifacts/` | Local models, dependencies, and generated results; ignored by Git |

See the [architecture guide](docs/architecture.md) (Japanese) for dependency boundaries and input ownership.

## License and model terms

Project code is under [Apache-2.0](LICENSE). Third-party code retains its own terms; see [NOTICE](NOTICE) and [licenses](licenses/).
This does **not** license model weights under Apache-2.0.

The [FAH weights license](licenses/FAH-WEIGHTS.txt), including SAM/DINOv3 pass-through terms, covers pose and detector weights and converted exports.
Research/noncommercial restrictions remain in ncnn, ONNX, Core ML, Metal, and LiteRT formats.
Redistribution must include the weight license. Detector attribution includes FAH and Ultralytics YOLOv8; its terms also flag applicable AGPL-3.0 obligations.

The bundled weight terms match upstream as checked on November 10, 2026.
Check the [upstream weight terms](https://github.com/noahcao/FAH/blob/main/WEIGHTS_LICENSE) before distribution.
A commercial game or plugin cannot rely on this repository's Apache license to use these weights.
This review doesn't establish AGPL compliance for a particular application or clear third-party commercial rights.
ONNX Runtime, CUDA/cuDNN, FFmpeg, ncnn, and LiteRT have separate distribution terms. Preserve notices for the actual binaries you distribute.
Converting models or distributing them separately doesn't remove their restrictions.
