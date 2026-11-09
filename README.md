# fah-android

An Apple adapter connects the original Metal networks to the shared Rust tracking core. See the [macOS/iOS build guide and validation status](apple/README.md) (Japanese). Camera inference is verified on an Apple M3 Mac. iOS device and Simulator builds pass; iOS device inference is unverified.

The shared code is organized into GUI, API, application, tracking core, and platform layers, with separate frame-input adapters. See the [architecture and engine-integration boundaries](docs/architecture.md) (Japanese). The Windows runtime library can be built without GPUI using `--no-default-features`.

A Windows app with a GPUI interface supports images, videos, and USB cameras. See the [Windows build and usage guide](windows/README.md).

Windows 11 inference supports CUDA, DirectML (no CUDA required), and CPU. Follow the [runtime installation steps](windows/README.md#install-inference-runtimes), then export the ONNX models with `tools/export_onnx.py`. CUDA requires CUDA 13.x and the CUDA 13 build of cuDNN 9. The app does not install these dependencies automatically.

[日本語](README_ja.md)

An experimental Android app that runs FAH-S hand pose estimation with Vulkan Compute.
The tested device is a Sony SO-51A.

This project is ported from [fah-metal](https://github.com/LuckyIYI/fah-metal).
It uses the original models and PyTorch implementation from [FAH](https://github.com/noahcao/FAH).

**This port doesn't achieve real-time performance.** Its current purpose is to validate numerical accuracy and operation on an Android device.
It doesn't reproduce the Metal version's reported 115–234 FPS.

## Features and limitations

- Runs the FAH-S pose estimator and FAH hand detector.
- Outputs 21 joints per hand in 2D and 3D, for up to two hands.
- Tracks regions from the previous frame and runs detection again every 10 processed frames.
- Overlays joints on the camera preview and a sample image.
- Doesn't implement meshes, MANO rotations, contact estimation, or the video correction model.

Inference uses ncnn, with custom Vulkan shaders for matrix multiplication, reduction, GELU, and LayerNorm.
The implementation isn't an automatic translation of Metal code.
Pose estimation, including residual calculations, uses FP32.
The app doesn't use FP16 because that path failed the numerical accuracy checks.

Rust handles JNI, image cropping, coordinate restoration, tracking, and the validation command.
A C ABI adapter retains ncnn model execution and custom Vulkan layers in C++.
3D coordinates use the camera coordinate system, in meters.
Camera intrinsics are estimated from the image dimensions.
Accuracy with calibrated device camera intrinsics hasn't been evaluated.

Handedness reassessment, boundary recropping, and smoothing aren't implemented.
Numerical agreement of the pose network is separate from agreement of the complete tracking pipeline.
This port doesn't guarantee identical behavior to the full FAH tracking pipeline.

## Requirements

| Item | Tested environment |
|---|---|
| Device | Sony SO-51A |
| SoC and GPU | SM8250 and Adreno 650 |
| Android | 12, API 31, as reported by device properties |
| ABI | arm64-v8a |
| Vulkan | 1.1.128; FP16 supported; Cooperative Matrix unsupported |
| Host | Windows, PowerShell, Python 3.12, and JDK 17 |
| Build tools | Gradle 8.14.5, AGP 8.13.2, NDK 30.0.16248370, and CMake 3.22.1 |

The Android SDK also requires `platforms;android-36`.
Other Android devices haven't been tested.

## Set up the app

Install Rust 1.87 or later and Cargo. Add Cargo to `PATH`, then install the Android target.

```powershell
rustup target add aarch64-linux-android
```

The default SDK path is `$env:LOCALAPPDATA\Android\Sdk`.
Initial setup downloads the models and dependencies.
FAH weights have research and noncommercial use restrictions. See the [weights license](licenses/FAH-WEIGHTS.txt) for the full terms.
This configuration doesn't require additional MANO or SAM 3D Body models.

Run these commands from the repository root in PowerShell.

1. Install the required Android SDK packages.

   ```powershell
   & "$env:LOCALAPPDATA\Android\Sdk\cmdline-tools\latest\bin\sdkmanager.bat" 'platforms;android-36' 'ndk;30.0.16248370' 'cmake;3.22.1'
   ```

2. Download and convert the models.

   ```powershell
   .\tools\setup.ps1
   ```

3. Install the app and models on a connected device with USB debugging authorized.

   ```powershell
   .\tools\device.ps1 -Action install
   ```

If multiple devices are connected, select the target with `-Serial`.
If your SDK is in a different location, pass `-Sdk` to both scripts.

## Use the app

The app's controls are in Japanese. Unlock the device and tap **カメラ** (Camera).

| Control | Action |
|---|---|
| **カメラ** (Camera) | Open the camera preview and run inference. |
| **サンプル検証** (Sample test) | Run inference on the published sample image. |
| **再検出** (Redetect) | Reset the tracked regions. |
| **CPU/GPU** | Switch the inference backend. |

CPU inference is faster on this device. The default backend is Vulkan.
If GPU inference fails, the app retries on the CPU and displays **CPU（GPUエラー後）** (CPU after a GPU error).
Subsequent frames use the CPU. Use **CPU/GPU** to select the backend again.

The SO-51A's older Adreno driver produced errors when camera rendering overlapped with long GPU operations.
During Vulkan inference, the app pauses the camera preview and keeps the captured frame visible.
It resumes the preview after inference completes. This pause prevents continuous video updates.

The displayed FPS is the reciprocal of the native processing time.
It excludes camera capture, preview restart, and screen rendering.

The models are stored in this directory on the device:

```text
/sdcard/Android/data/dev.fah.android/files/models/
```

The parent directory contains `last-result.json`.
Sample tests also save `sample-result.png` there.
The APK doesn't include model weights. Copying only the APK to another device isn't enough to run inference.

## Run validation

```powershell
.\tools\device.ps1 -Action check
```

This command stops the camera app before running validation.
After validation, reopen the camera screen:

```powershell
.\tools\device.ps1 -Action camera
```

Setup and device validation perform the following checks. `setup.ps1` runs the first two checks.

- Compare FAH's original forward pass with the rewritten forward pass used for conversion.
- Compare host ncnn outputs with PyTorch reference outputs.
- Compare Android CPU and Vulkan outputs with PyTorch reference outputs.
- Compare outputs for four crops derived from a real image.
- Process the same sample image 12 times to check tracking state and repeated detection.

The coordinate checks pass when the maximum 2D difference is below 0.5 pixels and the maximum 3D difference is below 1 mm.
The 2D threshold uses input image coordinates.
These checks measure differences introduced by the port, not estimation accuracy against ground-truth labels.
They don't evaluate tracking accuracy across a video.

Measurements are saved to `artifacts/validation.json`, and logs are saved to `artifacts/`.
The standalone model benchmarks exclude model loading and the first two inference runs.
For the initial device test results, see the [validation report (Japanese)](docs/validation.md).

### Validate a sample video

Install FFmpeg and add it to `PATH`, then run:

```powershell
.\tools\device.ps1 -Action video
```

The command extracts 30 distinct frames at 5 FPS from the published selfie video, starting at 5 seconds.
It uses the source-image panel (640 × 426) and runs the frames sequentially on Android.
It checks finite joint coordinates, tracking, periodic detection, and absence of CPU fallback.
Results are saved to `artifacts/video-validation.json`. This is a smoke test, not a ground-truth accuracy evaluation.

### Test the Rust code

```powershell
cargo test -p fah-core -p fah-api -p fah-application -p fah-input --release --locked
cargo clippy --manifest-path app/src/main/rust/Cargo.toml --all-targets --target aarch64-linux-android -- -D warnings
```

Gradle builds the Rust static library through CMake using the committed Cargo lockfile.
The app and `fah-check` link the same Rust pipeline and ncnn adapter.

For the Rust migration checks, including video and APK tests, see [the recorded results](docs/rust-validation.json).

## Project structure

| Path | Purpose |
|---|---|
| `tools/export.py` | Load the required FAH modules and convert them to ncnn. |
| `tools/make_fixtures.py` | Generate test inputs and PyTorch reference outputs. |
| `app/src/main/rust/src/android.rs` | Implement JNI entry points and bitmap locking. |
| `crates/fah-ncnn/src/runtime.rs` | Own native runtime handles and copy tensor outputs into Rust. |
| `backends/ncnn/cpp/bridge.cpp` | Adapt the ncnn runtime to C ABI calls and contain C++ exceptions. |
| `backends/ncnn/cpp/runtime.cpp` | Load ncnn models, run inference, and split GPU processing. |
| `backends/ncnn/cpp/vulkan_ops.cpp` | Implement custom Vulkan matrix multiplication, reduction, GELU, and LayerNorm. |
| `crates/fah-core/src/pipeline.rs` | Detect hands, crop images, construct camera conditioning, restore coordinates, and track regions. |
| `app/src/main/java/dev/fah/android/MainActivity.java` | Display the camera preview and joints. |
| `crates/fah-ncnn/src/check.rs` | Run numerical comparisons and benchmarks on the device. |

## License

The project code is licensed under Apache-2.0.
Attributions for FAH, fah-metal, MLX, and ncnn are in [NOTICE](NOTICE) and `licenses/`.
Models and converted weights remain subject to the FAH weights license.
The `models/` and `third_party/` directories are excluded from Git.
