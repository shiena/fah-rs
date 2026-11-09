# fah-android

Macでは[Windowsと共通のRust／GPUIアプリ](macos/README.md)を使えます。macOS／iOS向けのMetalアダプターを追加しています。[ビルド手順と検証状況](apple/README.md)を参照してください。

GUI・共通API・アプリケーション・追跡コア・プラットフォームの5層と映像入力アダプターを分離しています。[構成とUnity・Godot向けの接続境界](docs/architecture.md)を参照してください。

Windows版も利用できます。GPUIのGUIで画像・動画・USBカメラを処理します。[ビルドと使い方](windows/README_ja.md)を参照してください。

Windows 11版はCUDA・DirectML・CPUに対応します。[ランタイムの導入手順](windows/README_ja.md#ランタイムを導入する)に従い、`tools/export_onnx.py`でモデルを変換します。CUDAを使う場合はCUDA 13.xとCUDA 13用cuDNN 9が必要です。アプリによる自動導入は行いません。

FAH-Sの手姿勢推定を、AndroidのVulkan Computeで実行する検証用アプリです。
対象端末は、接続して検証したSony SO-51Aです。

移植元は[fah-metal](https://github.com/LuckyIYI/fah-metal)です。
元のモデルとPyTorch実装は[FAH](https://github.com/noahcao/FAH)を使用しています。

**リアルタイム速度には達していません。** 現段階は精度と実機動作を確認する移植版です。
Metal版の115〜234FPSを再現したものではありません。

## 実装範囲

- FAH-Sの姿勢推定とFAHの手検出モデルを実行します。
- 最大2手の21関節を2D・3D座標として出力します。
- 前フレームの推定領域を使い、10処理フレームごとに再検出します。
- カメラ画面とサンプル画像に関節を重ねて表示します。
- メッシュ、MANO回転、接触推定、動画補正モデルは実装していません。

推論基盤はncnnです。行列積、集約演算、GELU、LayerNormには独自のVulkanシェーダーを追加しています。
Metalコードを機械変換した構成ではありません。
残差計算を含む姿勢推定はFP32を使います。
FP16経路は精度比較を通過していないため、アプリでは使用しません。

JNI、画像の切り出し、座標復元、追跡、検証コマンドはRustで実装しています。
ncnnのモデル実行と独自VulkanレイヤーはC++に残し、C ABIで接続しています。
3D座標はカメラ座標系のメートル単位です。
カメラ内部パラメーターは画像サイズから推定します。
実機カメラの校正値を使った精度評価は未実施です。

手の左右を再推定する処理、境界での再切り出し、平滑化は未実装です。
手の姿勢ネットワークの数値一致と、追跡全体の一致は別の検証です。
FAHの追跡パイプライン全体との完全な一致は保証していません。

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
他のAndroid端末では検証していません。

## 準備する

Rust 1.87以降とCargoが必要です。Cargoを`PATH`に登録し、Androidターゲットを追加します。

```powershell
rustup target add aarch64-linux-android
```

SDKの既定パスは`$env:LOCALAPPDATA\Android\Sdk`です。
初回はモデルと依存ライブラリをダウンロードします。
FAHの重みには研究・非商用の利用条件があります。
詳細は[重みのライセンス](licenses/FAH-WEIGHTS.txt)を参照します。
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
詳しい初回検証結果は[検証記録](docs/validation.md)を参照します。

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

Rust移植後の動画・APK検証結果は[検証データ](docs/rust-validation.json)を参照します。

## ファイル構成

| パス | 内容 |
|---|---|
| `tools/export.py` | FAHの必要なモジュールだけを読み込み、ncnnへ変換 |
| `tools/make_fixtures.py` | 比較用の入力とPyTorchの基準値を生成 |
| `app/src/main/rust/src/android.rs` | JNI入口とビットマップのロック管理 |
| `crates/fah-ncnn/src/runtime.rs` | ネイティブハンドルの所有とテンソルのコピー |
| `backends/ncnn/cpp/bridge.cpp` | ncnnのC ABIアダプターとC++例外の処理 |
| `backends/ncnn/cpp/runtime.cpp` | ncnnの読み込み、推論、GPU処理の分割 |
| `backends/ncnn/cpp/vulkan_ops.cpp` | 独自のVulkan行列積・集約・GELU・LayerNorm |
| `crates/fah-core/src/pipeline.rs` | 検出、切り出し、カメラ条件、座標復元、追跡 |
| `app/src/main/java/dev/fah/android/MainActivity.java` | カメラと関節表示 |
| `crates/fah-ncnn/src/check.rs` | 実機の比較・計測用コマンド |

## ライセンス

本プロジェクトのコードはApache-2.0です。
FAH、fah-metal、MLX、ncnnの帰属情報は[NOTICE](NOTICE)と`licenses/`に記載しています。
モデルと変換済み重みはFAHの重みライセンスに従います。
`models/`と`third_party/`はGitの管理対象から除外しています。
