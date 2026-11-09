# FAH for Windows

The GPUI app displays 21 hand joints over images, videos, and USB camera streams. It shares the Rust tracking pipeline and ncnn CPU/Vulkan runtime with the Android app. See the [Japanese guide](README_ja.md) for camera controls and implementation details.

## Build and run

Install Rust with the x64 MSVC toolchain (tested with Rust 1.96), Visual Studio's **Desktop development with C++** workload, Git, CMake 3.22 or later, and Ninja. Add CMake and Ninja to `PATH`. Video and camera inputs also require an FFmpeg build with DirectShow support on `PATH`; alternatively, set `FAH_FFMPEG` to its executable path. GPU inference requires a Vulkan driver.

From the repository root, run:

```powershell
.\tools\windows.ps1 build
.\tools\windows.ps1 run
```

Reuse the Android app's exported `models` directory. If it is missing, follow the [model preparation instructions](../README.md) and run `tools/setup.ps1`. Building the Windows app itself does not require the Android SDK. The [model weight license](../licenses/FAH-WEIGHTS.txt) still applies.

The required files are `pose.ncnn.param`, `pose.ncnn.bin`, `detector.ncnn.param`, `detector.ncnn.bin`, and `uv_grid.f32`. The sample image also requires `sample.png`.

After building, run `windows/target/release/fah-windows.exe` directly. Keep `fah_backend.dll` beside the executable. Use `--models DIRECTORY` to select another model directory. Close the app before rebuilding.

## Use a camera

Select a device from the left panel, then click **カメラ開始** (Start camera). Click **再検索** (Refresh) if you connected the camera after launch. **停止** (Stop) releases the camera. Capture and hand overlays were tested with an HD Pro Webcam C920.

Use **左右反転** to mirror the camera preview, including its joint overlays. The toggle also works while paused and defaults to OFF. Image/video inputs and saved PNG/JSON results remain unmirrored.

Use **画像・動画を開く** to open a file, or **サンプル画像** for the bundled sample image. The adjacent video button requires the source checkout's `third_party/FAH/docs/assets/videos/test_video_selfie.mp4`. **結果を保存** saves the displayed image and a JSON file with the same stem.

```powershell
$app = '.\windows\target\release\fah-windows.exe'
& $app --list-cameras
& $app --models models --camera 'HD Pro Webcam C920'
& $app --models models --sample --cpu
& $app --check models gpu 5
& $app --models models --sample-video --smoke --frames 30 --output artifacts/windows-video
.\tools\windows.ps1 test
```

Video and camera frames are fitted into 640×480 while preserving their aspect ratio. The app drops pending frames when inference falls behind. It does not play or record audio. Displayed inference FPS excludes capture and rendering time. Camera intrinsics are estimated from image dimensions.

Windows Vulkan inference retains intermediate tensors to avoid incorrect results observed with buffer reuse on the RTX 5070. This increases memory use. The app explicitly releases Vulkan before unloading its native DLL. Inference uses FP32; CUDA is not included.

CPU and Vulkan reference checks, four real-image Vulkan fixtures, sample image/video processing, USB camera capture, and GUI controls were verified. See the [validation results](../docs/windows-validation.json).
