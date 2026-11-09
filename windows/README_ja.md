# Windows版

Windows 11向けに、GPUIで画像、動画、USBカメラに手の21関節を描画します。Android版とRustの追跡処理を共有します。推論はONNX RuntimeのCUDA・DirectML・CPUに対応します。比較用にncnn/Vulkanも残しています。

## ランタイムを導入する

アプリとビルドスクリプトはランタイムを自動導入しません。最初に[Visual C++再頒布可能パッケージ（x64）](https://learn.microsoft.com/ja-jp/cpp/windows/latest-supported-vc-redist)とGPUドライバーを導入します。

### CUDAを使わない場合

1. [Microsoft.ML.OnnxRuntime.DirectML 1.24.4](https://www.nuget.org/packages/Microsoft.ML.OnnxRuntime.DirectML/1.24.4)をダウンロードします。nupkgはZIP形式です。
2. パッケージの`runtimes/win-x64/native`内にある`onnxruntime.dll`と`onnxruntime_providers_shared.dll`を、exeの隣の`runtimes/directml`へ展開します。
3. [Microsoft.AI.DirectML 1.15.4](https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4)をダウンロードします。
4. **`bin/x64-win/DirectML.dll`**を同じフォルダーへ展開します。Xbox用のDLLは使用しません。

CPUもこのONNX RuntimeのCPU実装を使います。DirectML・CPUのどちらにもCUDAは不要です。

### CUDAを使う場合

1. [CUDA Toolkit 13.1](https://developer.nvidia.com/cuda-13-1-0-download-archive)を導入します。
2. [cuDNN 9](https://developer.nvidia.com/cudnn-downloads)のWindows x86-64・CUDA 13用を導入します。
3. CUDAとcuDNNのDLLフォルダーをユーザーの`PATH`に追加します。CUDA 13.1では通常、`C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1\bin\x64`です。cuDNNでは`cudnn64_9.dll`と関連DLLがあるフォルダーを指定します。
4. [ONNX Runtime 1.31.1のWindows x64・CUDA 13版](https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-gpu_cuda13-1.31.1.zip)をダウンロードします。
5. `lib`内の`onnxruntime.dll`、`onnxruntime_providers_shared.dll`、`onnxruntime_providers_cuda.dll`を、exeの隣の`runtimes/cuda`へ展開します。
6. アプリと起動元のターミナルを再起動します。

CUDA版とDirectML版のDLLは別フォルダーに置きます。TensorRTは不要です。CUDAでは精度検証のためTF32を無効にしています。互換性は[ONNX Runtimeの対応条件](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html)を参照します。

DLLの配置先は環境変数`FAH_ORT_CUDA`と`FAH_ORT_DML`でも指定できます。値は各`onnxruntime.dll`の絶対パスです。環境変数、exeの隣の`runtimes`、開発用チェックアウトの順に検索します。開発用の配置先は[英語版README](README.md#install-inference-runtimes)に記載しています。

## 推論方式を選ぶ

起動時にCUDA用ONNXプロバイダー、CUDAランタイム、cuBLAS、cuBLASLt、cuDNN、NVIDIAドライバーのDLLを確認します。不足している場合、CUDAボタンは無効になります。導入やPATH変更後はアプリを再起動します。ファイルの存在確認後も、互換性を確認するためウォームアップ推論を行います。

［自動］はCUDA→DirectML→CPUの順に試します。CUDAのDLLが不足している場合はCUDAを候補から外します。候補ごとに両モデルを読み込み、ウォームアップ推論を行います。選択結果を画面に表示し、初期化に失敗した理由を保存JSONの`initialization_warning`に記録します。

方式を明示して選択した場合、初期化に失敗するとエラーを表示します。初期化後の推論で失敗した場合は、共通処理でCPUへ切り替えます。画面とJSONには実際の方式が表示されます。CUDA・DirectMLのデバイス番号は0です。ONNX Runtimeが一部の演算をCPUへ割り当てる場合があります。

## ビルドと起動

次の環境を用意します。

- Windows x64、RustのMSVCツールチェーン（検証環境は1.96）
- Visual Studioの「C++によるデスクトップ開発」
- Git、CMake 3.22以降、NinjaをPATHに登録
- 動画・カメラ用のFFmpeg（DirectShow対応）をPATHに登録
- GPUドライバー（Vulkan選択時はVulkan対応が必要）

リポジトリのルートで実行します。初回ビルドではncnnとRustの依存ライブラリを取得します。

```powershell
.venv/Scripts/python.exe -m pip install -r requirements-onnx.txt
.venv/Scripts/python.exe tools/export_onnx.py
.\tools\windows.ps1 build
.\tools\windows.ps1 run
```

モデルは既存の`models`フォルダーを使用します。未取得の場合は、[共通の準備手順](../README_ja.md#準備する)の`tools/setup.ps1`で変換してください。Windowsアプリ自体のビルドにAndroid SDKは不要です。モデルの利用条件は[重みのライセンス](../licenses/FAH-WEIGHTS.txt)を参照してください。

ONNX推論に必要なモデルは`pose.onnx`、`detector.onnx`、`uv_grid.f32`です。上記の変換には共通の準備手順で取得する`model.pt`と`detector.pt`が必要です。Vulkanでは従来のncnnモデルを使用します。サンプル画像には`sample.png`も必要です。

ビルド後は`windows/target/release/fah-windows.exe`を直接起動できます。`fah_backend.dll`は実行ファイルと同じ場所に置きます。モデルの場所は`--models`または画面の「モデルフォルダー」で指定できます。再ビルドの前にアプリを終了してください。

## カメラを使う

1. USBカメラを接続してアプリを起動します。
2. 左側の一覧からカメラを選択します。起動後に接続した場合は「再検索」を押します。
3. 「カメラ開始」を押します。同じボタンが「カメラ停止」に切り替わり、もう一度押すと撮影を終了します。停止時は映像と推論結果を消去し、入力待ちの画面に戻ります。

「左右反転」でカメラのプレビューを鏡のように左右反転できます。関節の描画も一緒に反転します。初期値はOFFです。画像・動画入力と、保存するPNG・JSONは反転しません。

HD Pro Webcam C920で映像取得と手の描画を確認しています。映らない場合は、Windowsのカメラ設定でデスクトップアプリのアクセスを許可し、同じカメラを使用しているアプリを終了してください。FFmpegの場所は環境変数`FAH_FFMPEG`でも指定できます。

「画像・動画を開く」でファイルを選択できます。「サンプル画像」と、その隣の「動画」は移植元のサンプルを使います。サンプル動画はソースチェックアウト内の`third_party/FAH/docs/assets/videos/test_video_selfie.mp4`が必要です。「結果を保存」で表示画像と同名のJSONを保存します。

## コマンドで確認する

```powershell
$app = (Resolve-Path windows/target/release/fah-windows.exe).Path
Start-Process $app -ArgumentList '--list-cameras' -Wait -RedirectStandardOutput cameras.json
Start-Process $app -ArgumentList '--models models --camera "HD Pro Webcam C920"'
$p = Start-Process $app -ArgumentList '--check models directml 5' -Wait -PassThru -RedirectStandardOutput directml.json -RedirectStandardError directml.log
$p.ExitCode
$p = Start-Process $app -ArgumentList '--check models cuda 5' -Wait -PassThru -RedirectStandardOutput cuda.json -RedirectStandardError cuda.log
$p.ExitCode
Start-Process $app -ArgumentList '--models models --backend auto --sample-video --smoke --frames 30 --output artifacts/onnx-video' -Wait
.\tools\windows.ps1 test
```

`--smoke`は画面を開かず、指定フレーム数の結果を`results.json`と`last-frame.png`に保存して終了します。画像入力では1フレームを処理します。

GUI実行ファイルの終了を待つため、検証には`Start-Process -Wait`を使います。`--check`は`cpu`、`cuda`、`directml`、`gpu`（従来のVulkan）を指定できます。通常起動時は`--backend auto|cuda|directml|cpu|vulkan`で方式を選択します。

## 実装上の制限と検証

コードの配置と所有権は[アーキテクチャ](../docs/architecture.md)に記載しています。同期セッションはホストからフレームを受け取り、GUI・カメラ・デスクトップ用ワーカーなしで動作します。Windowsライブラリは`--no-default-features`でGPUIを除外できます。Unity・Godot向けの公開ABIは未実装です。

- 動画・カメラは縦横比を保って640×480に収めます。処理が追いつかないフレームは破棄し、最新フレームを処理します。音声の再生・録音は行いません。
- 「推論FPS」は推論時間の逆数です。撮影・描画を含む実効FPSとは異なります。「取得→結果」はFFmpegからのフレーム受信後を計測し、カメラ内部の遅延は含みません。
- 3D座標には画像サイズから推定したカメラ内部パラメーターを使用します。
- WindowsのVulkanでは中間テンソルを保持します。RTX 5070でバッファー再利用時の数値不一致を避けるためで、メモリー消費が増えます。終了時にはVulkanを明示的に解放します。
- 推論はFP32です。

RTX 5070でDirectMLの参照値比較と4種類の実画像由来の入力検査が通過しました。姿勢推定単体の中央値はDirectML約4.6 ms、ONNXのCPU約44 msです。以前のVulkan約29 msは別の実行での測定です。サンプル画像の2手描画、動画30フレーム、CUDAからDirectMLへの切り替えを確認しました。

このPCにはCUDA 13.1がありますが、cuDNNがありません。CUDA版ONNX Runtimeの読み込みと依存不足の検出は確認しました。CUDAの精度・速度はcuDNN導入後の検証が必要です。[ONNX版の検証結果](../docs/windows-onnx-validation.json)と[従来版の検証結果](../docs/windows-validation.json)を参照してください。
