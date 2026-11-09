# Windows版

GPUIで画像、動画、USBカメラに手の21関節を描画します。Android版とRustの追跡処理、ncnnのCPU・Vulkan推論を共有しています。

## ビルドと起動

次の環境を用意します。

- Windows x64、RustのMSVCツールチェーン（検証環境は1.96）
- Visual Studioの「C++によるデスクトップ開発」
- Git、CMake 3.22以降、NinjaをPATHに登録
- 動画・カメラ用のFFmpeg（DirectShow対応）をPATHに登録
- Vulkan対応のGPUドライバー

リポジトリのルートで実行します。初回ビルドではncnnとRustの依存ライブラリを取得します。

```powershell
.\tools\windows.ps1 build
.\tools\windows.ps1 run
```

モデルは既存の`models`フォルダーを使用します。未取得の場合は、[共通の準備手順](../README_ja.md#準備する)の`tools/setup.ps1`で変換してください。Windowsアプリ自体のビルドにAndroid SDKは不要です。モデルの利用条件は[重みのライセンス](../licenses/FAH-WEIGHTS.txt)を参照してください。

必要なモデルは`pose.ncnn.param`、`pose.ncnn.bin`、`detector.ncnn.param`、`detector.ncnn.bin`、`uv_grid.f32`です。サンプル画像には`sample.png`も必要です。

ビルド後は`windows/target/release/fah-windows.exe`を直接起動できます。`fah_backend.dll`は実行ファイルと同じ場所に置きます。モデルの場所は`--models`または画面の「モデルフォルダー」で指定できます。再ビルドの前にアプリを終了してください。

## カメラを使う

1. USBカメラを接続してアプリを起動します。
2. 左側の一覧からカメラを選択します。起動後に接続した場合は「再検索」を押します。
3. 「カメラ開始」を押します。「停止」で撮影を終了します。

「左右反転」でカメラのプレビューを鏡のように左右反転できます。関節の描画も一緒に反転し、停止中にも切り替えられます。初期値はOFFです。画像・動画入力と、保存するPNG・JSONは反転しません。

HD Pro Webcam C920で映像取得と手の描画を確認しています。映らない場合は、Windowsのカメラ設定でデスクトップアプリのアクセスを許可し、同じカメラを使用しているアプリを終了してください。FFmpegの場所は環境変数`FAH_FFMPEG`でも指定できます。

「画像・動画を開く」でファイルを選択できます。「サンプル画像」と、その隣の「動画」は移植元のサンプルを使います。サンプル動画はソースチェックアウト内の`third_party/FAH/docs/assets/videos/test_video_selfie.mp4`が必要です。「結果を保存」で表示画像と同名のJSONを保存します。

## コマンドで確認する

```powershell
$app = '.\windows\target\release\fah-windows.exe'
& $app --list-cameras
& $app --models models --camera 'HD Pro Webcam C920'
& $app --models models --sample
& $app --models models --input 'C:\videos\hands.mp4' --cpu
& $app --check models gpu 5
& $app --models models --camera 'HD Pro Webcam C920' --smoke --frames 5 --output artifacts/windows-camera
.\tools\windows.ps1 test
```

`--smoke`は画面を開かず、指定フレーム数の結果を`results.json`と`last-frame.png`に保存して終了します。画像入力では1フレームを処理します。

## 実装上の制限と検証

- 動画・カメラは縦横比を保って640×480に収めます。処理が追いつかないフレームは破棄し、最新フレームを処理します。音声の再生・録音は行いません。
- 「推論FPS」は推論時間の逆数です。撮影・描画を含む実効FPSとは異なります。「取得→結果」はFFmpegからのフレーム受信後を計測し、カメラ内部の遅延は含みません。
- 3D座標には画像サイズから推定したカメラ内部パラメーターを使用します。
- WindowsのVulkanでは中間テンソルを保持します。RTX 5070でバッファー再利用時の数値不一致を避けるためで、メモリー消費が増えます。終了時にはVulkanを明示的に解放します。
- 推論はFP32です。CUDAは使用していません。

RTX 5070でCPU・Vulkanの参照値比較と4種類の実画像由来のVulkan入力検査が通過しました。姿勢ネットワーク単体の中央値はCPU約42 ms、Vulkan約29 msです。サンプル画像の2手描画、動画30フレーム、USBカメラの処理、画面操作、PNG・JSON保存を確認しました。測定の詳細は[検証結果](../docs/windows-validation.json)を参照してください。
