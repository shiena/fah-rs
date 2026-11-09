# FAH for Windows 11

The GPUI app tracks up to two hands in images, videos, and USB camera streams. Windows inference uses ONNX Runtime with CUDA, DirectML, or CPU. The Rust preprocessing and tracking code is shared with Android. The original ncnn/Vulkan backend remains available for comparison.

See the [Japanese guide](README_ja.md) for translated instructions.

## Install inference runtimes

Install the [Microsoft Visual C++ x64 Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist) and a current GPU driver. The application and build script do not install CUDA, cuDNN, or inference runtimes automatically.

### DirectML and CPU (no CUDA required)

1. Download [Microsoft.ML.OnnxRuntime.DirectML 1.24.4](https://www.nuget.org/packages/Microsoft.ML.OnnxRuntime.DirectML/1.24.4). A `.nupkg` file is a ZIP archive.
2. Extract `runtimes/win-x64/native/onnxruntime.dll` and `onnxruntime_providers_shared.dll` into `runtimes/directml` beside `fah-windows.exe`.
3. Download [Microsoft.AI.DirectML 1.15.4](https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4).
4. Extract **`bin/x64-win/DirectML.dll`** into that same directory. Do not use an Xbox DLL.

The CPU option uses the CPU provider in this DirectML runtime package. Neither option requires CUDA.

### CUDA (optional)

1. Install [CUDA Toolkit 13.1](https://developer.nvidia.com/cuda-13-1-0-download-archive) and a compatible NVIDIA driver.
2. Install [cuDNN 9 for Windows x86-64 and CUDA 13](https://developer.nvidia.com/cudnn-downloads). Select the CUDA 13 build. See the [CUDA provider requirements](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html).
3. Add the directories containing `cudart64_13.dll`, `cublas64_13.dll`, `cublasLt64_13.dll`, and the cuDNN DLLs to your user `PATH`. CUDA 13.1 usually stores its DLLs in `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1\bin\x64`. For cuDNN, use the directory containing `cudnn64_9.dll` and its companion DLLs.
4. Download [ONNX Runtime 1.31.1 for Windows x64, CUDA 13](https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-gpu_cuda13-1.31.1.zip).
5. Copy `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, and `onnxruntime_providers_cuda.dll` from its `lib` directory into `runtimes/cuda` beside the application.
6. Restart the application and its launching terminal after changing `PATH`.

Keep the CUDA and DirectML runtime DLLs in separate directories. TensorRT is not used or required. CUDA inference disables TF32 to preserve FP32 accuracy.

For custom installations, set `FAH_ORT_DML` and `FAH_ORT_CUDA` to the full paths of the respective `onnxruntime.dll` files. Lookup order is the explicit environment variable, the executable's `runtimes` directory, then the development checkout. An invalid explicit path produces an error.

Development lookup paths:

- `third_party/ort-directml-1.24.4/runtimes/win-x64/native/onnxruntime.dll`
- `third_party/ort-cuda13-1.31.1/onnxruntime-win-x64-gpu_cuda13-1.31.1/lib/onnxruntime.dll`

## Export models and build

Install Rust x64 MSVC (tested with Rust 1.96), Visual Studio's **Desktop development with C++** workload, Git, CMake 3.22 or later, and Ninja. Put CMake and Ninja on PATH. Video and camera inputs require FFmpeg with DirectShow support on PATH, or an executable path in `FAH_FFMPEG`.

Prepare checkpoints with the [repository setup instructions](../README.md). From the repository root, run:

```powershell
.venv/Scripts/python.exe -m pip install -r requirements-onnx.txt
.venv/Scripts/python.exe tools/export_onnx.py
.\tools\windows.ps1 build
.\tools\windows.ps1 run
```

The app needs `models/pose.onnx`, `detector.onnx`, and `uv_grid.f32`. The sample image also needs `sample.png`. Export uses fixed shapes and FP32 weights. The [model license](../licenses/FAH-WEIGHTS.txt) still applies.

Run `windows/target/release/fah-windows.exe` directly after building. Keep `fah_backend.dll` beside it for the Vulkan backend. No console opens. Use `--models DIRECTORY` to select another model directory. Close the app before rebuilding. The Windows build does not require the Android SDK.

Vulkan uses the original ncnn model files and camera grid.

## Select a backend and input

**自動** (Auto) tries CUDA, DirectML, then CPU. Each candidate must load both models and complete a warm-up inference. This catches cuDNN dependencies loaded only by the first convolution. Failed candidates are recorded in saved JSON under `initialization_warning`.

Explicit **CUDA**, **DirectML**, **CPU**, or **Vulkan** selections report initialization errors without choosing another provider. An inference failure after successful initialization uses the shared pipeline's CPU recovery path; the preview and JSON show CPU. Both GPU providers use device index 0. ONNX Runtime may run unsupported operators on CPU within a GPU session.

Select a camera, then click **カメラ開始** (Start camera). **再検索** refreshes the device list. **停止** releases the camera. **左右反転** mirrors the camera preview and joints, including while paused. Saved PNG/JSON results and image/video inputs remain unmirrored.

**画像・動画を開く** opens a file. **サンプル画像** uses the sample image. The adjacent video button needs the source checkout's FAH sample video. **結果を保存** saves a PNG with joint overlays and a JSON file.

Video/camera frames fit into 640×480 while preserving aspect ratio. Pending frames are dropped when inference falls behind. Audio is not processed. Inference FPS excludes capture and rendering. Camera intrinsics are estimated from image dimensions.

## Validate the installation

Use `Start-Process -Wait` for CLI checks because this is a Windows GUI executable. Capture stderr to see dependency errors.

```powershell
$app = (Resolve-Path windows/target/release/fah-windows.exe).Path
$p = Start-Process $app -ArgumentList '--check models directml 5' -Wait -PassThru -RedirectStandardOutput directml.json -RedirectStandardError directml.log
$p.ExitCode
$p = Start-Process $app -ArgumentList '--check models cuda 5' -Wait -PassThru -RedirectStandardOutput cuda.json -RedirectStandardError cuda.log
$p.ExitCode
Start-Process $app -ArgumentList '--models models --backend auto --sample-video --smoke --frames 30 --output artifacts/onnx-video' -Wait
.\tools\windows.ps1 test
```

`--check` accepts cpu, cuda, directml, or gpu (legacy Vulkan), a run count, and an optional fixture directory. `--smoke` saves results.json and last-frame.png, then exits. Use `--list-cameras` with stdout redirection to list devices.

If CUDA reports cudnn64_9.dll missing, install cuDNN and add its DLL directory to PATH. A working nvcc does not establish that cuDNN is available. For DirectML load failures, check the x64 Visual C++ Redistributable and the DirectML package version.

## Design and validation status

Rust loads the ONNX Runtime C API through ort-sys. Each runtime owns its API table and native handles; CUDA and DirectML do not share a global API pointer. The inference worker owns sessions exclusively. Switching backends drops sessions before creating replacements. Handles are released before library unload. DirectML uses sequential execution with memory patterns disabled.

DirectML passed the pose reference check and four real-image fixtures on RTX 5070. Pose-only median latency was approximately 4.6 ms, versus approximately 29 ms in the earlier ncnn/Vulkan run. These are separate runs, not a controlled throughput benchmark. CPU checks, sample/video processing, and CUDA-to-DirectML recovery also passed.

CUDA 13.1 and ONNX Runtime 1.31.1 loaded, but inference failed because cuDNN was absent. CUDA accuracy and performance remain unverified until cuDNN is installed. See [validation results](../docs/windows-onnx-validation.json).
