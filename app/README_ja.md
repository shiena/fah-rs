# Androidのビルドと検証

[English](README.md)・[プロジェクト概要](../README_ja.md)

コマンドはすべてリポジトリのルートで実行します。

## 動作環境

| 項目 | 検証環境 |
|---|---|
| 端末 | Sony SO-51A |
| SoC・GPU | SM8250・Adreno 650 |
| Android | 12、API 31（端末のプロパティ値） |
| ABI | arm64-v8a |
| Vulkan | 1.1.128、FP16対応、Cooperative Matrix非対応 |
| ホスト | Windows、PowerShell、Python 3.12、JDK 17 |
| ビルド | Gradle 8.14.5、AGP 8.13.2、NDK 30.0.16248370、CMake 3.22.1 |

Android SDKには`platforms;android-36`も必要です。
この表は初期のSO-51A検証環境を記録したものです。

## 準備する

Rust 1.87以降とCargoが必要です。Cargoを`PATH`に登録し、Androidターゲットを追加します。

```powershell
rustup target add aarch64-linux-android
```

SDKの既定パスは`$env:LOCALAPPDATA\Android\Sdk`です。
初回はモデルと依存ライブラリをダウンロードします。
FAHの重みには研究・非商用の利用条件があります。
詳細は[重みのライセンス](../licenses/FAH-WEIGHTS.txt)を参照します。
この構成ではMANOやSAM 3D Bodyの追加モデルは不要です。

1. Android SDKの必要なパッケージをインストールします。

   ```powershell
   & "$env:LOCALAPPDATA\Android\Sdk\cmdline-tools\latest\bin\sdkmanager.bat" 'platforms;android-36' 'ndk;30.0.16248370' 'cmake;3.22.1'
   ```

2. モデルを取得し、変換します。

   ```powershell
   .\tools\setup.ps1
   ```

3. USBデバッグを許可した端末へアプリとモデルを配置します。

   ```powershell
   .\tools\device.ps1 -Action install
   ```

複数の端末を接続している場合は、`-Serial`で対象を指定します。
SDKの場所が異なる場合は、両スクリプトに`-Sdk`を指定します。

## 操作する

端末のロックを解除して、アプリの［カメラ］をタップします。
［サンプル検証］は公開サンプル画像で推論します。
［再検出］は追跡領域をリセットします。
［CPU/GPU］は実行方式を切り替えます。
この端末ではCPUの方が高速です。既定値はVulkanです。
GPU推論がエラーになった場合はCPUで再試行し、画面に「CPU（GPUエラー後）」と表示します。
以後はCPUで処理し、［CPU/GPU］で実行方式を選び直せます。

SO-51Aの古いAdrenoドライバーでは、カメラ描画と長いGPU処理の同時実行でエラーが発生しました。
対策として、推論中はカメラプレビューを停止します。
取得したフレームを表示したまま推論し、完了後にプレビューを再開します。
画面が連続した動画のように更新されないのは、この動作によるものです。

画面に表示するFPSは、ネイティブ処理時間の逆数です。
カメラの取得、プレビューの再開、画面描画の時間は含みません。

モデルは次の端末内フォルダーに配置します。

```text
/sdcard/Android/data/dev.fah.android/files/models/
```

その親フォルダーには`last-result.json`が保存されます。
サンプル検証時は`sample-result.png`も保存されます。
APKには学習済みモデルを含めていません。
APKだけを別端末へコピーしても推論は動作しません。

## 検証する

```powershell
.\tools\device.ps1 -Action check
```

このコマンドはカメラアプリを停止してから実行します。
検証が完了したら、次のコマンドでカメラ画面を再開します。

```powershell
.\tools\device.ps1 -Action camera
```

準備時と実機検証時の検証内容は次のとおりです。最初の2項目は`setup.ps1`で実行します。

- FAHの元のforward処理と、変換用forward処理を比較します。
- PCのncnn出力とPyTorchの基準値を比較します。
- AndroidのCPU・Vulkan出力とPyTorchの基準値を比較します。
- 実画像由来の4種類の切り出し画像を比較します。
- 同じサンプル画像を12回処理し、追跡と再検出を確認します。

2D座標の最大差が入力画像上で0.5ピクセル未満、3D座標の最大差が1mm未満なら合格です。
いずれも実データの正解ラベルに対する推定精度ではなく、移植による差の検査です。
動画全体に対する追跡精度評価は含みません。

測定値は`artifacts/validation.json`、実行ログは`artifacts/`に保存します。
モデルの読み込みと初回2回の推論は、単体ベンチマークから除外します。
詳しい初回検証結果は[検証記録](../docs/validation.md)を参照します。

### サンプル動画を検証する

FFmpegをインストールして`PATH`に登録し、次のコマンドを実行します。

```powershell
.\tools\device.ps1 -Action video
```

公開セルフィー動画の5秒地点から、5FPSで30枚の異なるフレームを抽出します。
元画像の領域（640×426）を切り出し、Androidで順番に推論します。
関節座標の有限性、追跡、定期的な再検出、CPU切り替えの有無を検査します。
結果は`artifacts/video-validation.json`に保存します。
これは動作確認であり、正解ラベルに対する精度評価ではありません。

### Rustのコードを検証する

```powershell
cargo test -p fah-core -p fah-api -p fah-application -p fah-input --release --locked
cargo clippy --manifest-path app/src/main/rust/Cargo.toml --all-targets --target aarch64-linux-android -- -D warnings
```

GradleはCMakeからRustの静的ライブラリをビルドします。依存関係はCargo.lockで固定しています。
アプリと`fah-check`は、共通のRustパイプラインとncnnアダプターを使います。

Rust移植後の動画・APK検証結果は[検証データ](../docs/rust-validation.json)を参照します。

