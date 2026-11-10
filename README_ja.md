# fah-rs

[English](README.md)

Rustの共通コアとプラットフォーム別の推論エンジンで、FAH-Sの手姿勢推定を実行します。デスクトップGUIにはGPUIを使用します。
元のモデルは[FAH](https://github.com/noahcao/FAH)、移植元は[fah-metal](https://github.com/LuckyIYI/fah-metal)です。

**モデルの重みは研究・非商用用途に限定されています。** 変換後のモデルにも条件が残ります。取得・配布前に[ライセンスとモデルの利用条件](#ライセンスとモデルの利用条件)を確認します。

## 対応プラットフォーム

| プラットフォーム | アプリ・組み込み方法 | 推論エンジン | 検証範囲 |
|---|---|---|---|
| Windows 11 x64 | GPUIアプリ、C ABI | ONNX Runtime CUDA／DirectML／CPU、ncnn Vulkan | 画像・動画・USBカメラ。CUDA推論はcuDNN不足により未検証 |
| Android arm64、API 26以降 | Androidアプリ、C ABI | ncnn Vulkan、LiteRT CPU姿勢推定＋ncnn CPU検出器 | 実機の画像・動画・カメラ |
| macOS 15以降、Apple silicon | GPUIアプリ、C ABI | Metal姿勢推定＋Core ML検出器 | Apple M3の画像・動画・カメラ |
| iOS 18以降、arm64 | ネイティブライブラリ、C ABI確認アプリ | Metal姿勢推定＋Core ML検出器 | iPhone 15 Proのサンプル画像。カメラは未検証 |
| Linux x86_64 | C ABI、コマンドラインのサンプル | ncnn Vulkan、LiteRT CPU姿勢推定＋ncnn CPU検出器 | Steam Deckのサンプル動画 |

自動選択では、WindowsはCUDA→DirectML→CPUの順で初期化します。CUDAは必要なライブラリがある場合だけ有効になります。
AndroidとLinuxはVulkanを優先し、失敗時にCPUを使用します。AppleにはCPUフォールバックがありません。
Linux用のGPUIアプリとカメラ入力は未実装です。iOSの確認アプリはGPUIを使用しません。

## 機能と制限

- 最大2手について、各21関節の2D・3D座標を出力します。
- 切り出し、座標復元、追跡を共有し、10処理フレームごとに再検出します。
- GUI、共通API、アプリケーション、追跡コア、プラットフォームの5層と、映像入力アダプターを分離しています。
- Unity・Godot向けにGUI不要のC ABIとC/C#サンプルを提供します。エンジン固有のプラグインパッケージは未実装です。

3D座標はカメラ座標系のメートル単位です。内部パラメーターは画像サイズから推定し、校正値は使用しません。
メッシュ、MANO回転、接触推定、動画補正、左右の再推定、境界での再切り出し、平滑化は未実装です。
性能は端末と推論エンジンによって異なります。移植元の全パイプラインや公称FPSを再現する構成ではありません。

## モデルを準備する

検証済みのチェックポイント準備環境はWindows、PowerShell、Git、Python 3.12です。
リポジトリのルートで実行します。

```powershell
.\tools\setup.ps1
```

`.venv`を作成し、固定したFAHソースとハッシュ検証付きの重みを取得します。ncnnモデルと検証用データも生成します。
Android用ncnnライブラリの取得と`local.properties`の作成も行います。Android SDK自体は導入しません。WindowsアプリのビルドにAndroid SDKは不要です。

| 対象 | 追加のモデル準備 |
|---|---|
| Windows ONNX Runtime | `tools/export_onnx.py`。[Windows手順](windows/README_ja.md)を参照 |
| Android／Linux Vulkan | `tools/setup.ps1`が生成するncnnファイルを使用 |
| macOS／iOS | `tools/export_apple.py`で出力後、Macで変換。[Apple手順](apple/README.md#モデルを準備する)を参照 |
| Android／Linux CPU | 専用環境で`tools/export_litert.py`を実行。[LiteRT変換手順](docs/litert-performance.md#wslで変換する)を参照 |

生成したモデルを実行先へコピーします。各形式には互換性がありません。
この構成のエクスポーターではMANOやSAM 3D Bodyの追加取得は不要です。ただし、FAHの重みの制限は残ります。
`models/`と`third_party/`はGitの管理対象外です。

## ビルドして起動する

コマンドはリポジトリのルートで実行します。ランタイム、モデルの配置、検証の詳細は各ガイドを参照します。
Cargoの既定ターゲットをビルドするだけでは、ネイティブ依存関係やモデルを準備できません。

### Windows

Rust x64 MSVC、Visual StudioのC++デスクトップ開発、CMake 3.22以降、Ninjaを導入します。
[ONNX RuntimeのDLL](windows/README_ja.md#ランタイムを導入する)を配置します。CUDA使用時はCUDA 13.xとCUDA 13用cuDNN 9も必要です。
動画・カメラ入力にはDirectShow対応のFFmpegが必要です。

```powershell
.venv/Scripts/python.exe -m pip install -r requirements-onnx.txt
.venv/Scripts/python.exe tools/export_onnx.py
.\tools\windows.ps1 build
.\tools\windows.ps1 run
```

実行ファイルは`windows/target/release/fah-windows.exe`です。[Windowsガイド](windows/README_ja.md)を参照してください。

### Android

JDK 17、Rust 1.87以降、Android SDK platform 36、NDK 30.0.16248370、CMake 3.22.1を導入します。
端末のUSBデバッグを有効にして接続を許可します。

```powershell
rustup target add aarch64-linux-android
.\tools\device.ps1 -Action install
.\tools\device.ps1 -Action check
```

`install`はAPKをビルドし、モデルを別途転送します。APKだけでは推論できません。
[Androidの準備・操作・動画検証ガイド](app/README_ja.md)を参照してください。
CPUはLiteRT 2.3.0で姿勢を推論し、ncnnで検出します。インストール前に`models/litert/pose.tflite`も準備します。

### macOS

Apple silicon Mac、macOS 15以降、Xcode 26.2以降、Rust、Python 3を使用します。
[Apple用モデル](apple/README.md#モデルを準備する)を準備し、Mac上で実行します。

```sh
xcodebuild -downloadComponent MetalToolchain
python3 macos/build.py --models models/apple
```

`macos/build/FAH.app`を生成して起動します。`--camera`を追加するとカメラを開始します。
[GPUIアプリのガイド](macos/README.md)を参照してください。

### iOS

Xcode 26.2以降、Rust、Python 3を導入したMacを使用します。先にApple用モデルを準備します。

```sh
python3 engines/ios/build.py
```

`engines/build/ios/libfah_engine.a`を生成します。ホストにはSwiftアダプターとシェーダーリソースも必要です。
署名付きの実機確認アプリをビルドする場合は`--team YOUR_APPLE_TEAM_ID`を追加します。
[iOSの実機確認手順](engines/README.md#iosでビルド検証する)と[Swiftパッケージのガイド](apple/README.md)を参照してください。

### Linux

LinuxまたはWSLにRust、Python 3、GCCまたはClang、CMake 3.22以降、Ninja、Vulkan開発ヘッダーを導入します。
LiteRT 2.3.0はビルド用Python環境の`ai-edge-litert==2.3.0`、またはライブラリのパス指定で準備します。

```sh
git clone --branch 20261020 --depth 1 --recurse-submodules --shallow-submodules https://github.com/Tencent/ncnn.git third_party/ncnn-src
python3 engines/build.py --platform linux --litert-library /absolute/path/libLiteRt.so
```

固定版のncnnを取得済みならcloneを省略します。Pythonパッケージを導入済みなら`--litert-library`を省略できます。
`engines/build/linux/`に`libfah_engine.so`、`libfah_backend.so`、`libLiteRt.so`とLiteRTのライセンスを配置します。
実行時にPythonは不要です。CPU推論には`models/litert/pose.tflite`も必要です。
[Linuxのビルド・動画検証ガイド](engines/README.md#linuxでビルドする)を参照してください。

## 組み込みと検証

[C ABIガイド](engines/README.md)では、配布物、ライフサイクル、座標系、ホストの責務を説明しています。
ホストはRGBA8画像を渡し、映像入力、テクスチャの読み出し、スレッドを管理します。
[Cサンプル](engines/examples/check.c)と[C#ラッパー](engines/examples/FahSession.cs)を参照してください。
Unity Editor、IL2CPP、Godotプラグインへの組み込みは未検証です。

```sh
cargo test -p fah-core -p fah-api -p fah-application -p fah-input --release --locked
```

このコマンドはGUIなしでRust共通層をテストします。ネイティブ推論の確認方法は各プラットフォームのガイドを参照します。
検証記録は[Android](docs/rust-validation.json)、[Windows](docs/windows-onnx-validation.json)、[macOS](docs/macos-gpui-validation.json)、[Linux LiteRT](engines/linux/litert-validation.json)にあります。
数値一致と動画の動作確認は、正解ラベルに対する推定精度の評価ではありません。

## ファイル構成

| パス | 役割 |
|---|---|
| `crates/fah-gui/` | Windows・macOS共通のGPUI画面 |
| `crates/fah-api/` | 画像、推論、追跡の共通契約 |
| `crates/fah-application/` | セッションとデスクトップ用ワーカー制御 |
| `crates/fah-core/` | 前処理、後処理、追跡 |
| `crates/fah-input/` | 共通の静止画入力アダプター |
| `crates/fah-ncnn/`、`backends/ncnn/cpp/` | RustのncnnアダプターとC++／Vulkanランタイム |
| `crates/fah-litert/`、`crates/fah-ncnn-platform/`、`crates/fah-linux-platform/` | LiteRT C API、Android・Linux共通の推論方式選択、Linuxの入口 |
| `crates/fah-apple-platform/`、`apple/` | RustのAppleアダプター、Swift／Metal／Core ML実装、移植元ソース |
| `crates/fah-ffi/`、`engines/` | C ABI、ヘッダー、ホスト用サンプル、配布物の生成 |
| `windows/` | Windowsの起動処理、ONNX推論、OS機能、ネイティブビルド |
| `macos/` | macOSのGPUI起動処理とアプリバンドル生成 |
| `app/` | Android UI、Rust JNIブリッジ、Gradle／CMakeビルド |
| `tools/`、`docs/` | モデル変換、セットアップ、設計、検証記録 |
| `licenses/`、`NOTICE` | 依存コードの告知とモデルの利用条件 |
| `models/`、`third_party/`、`artifacts/` | ローカルのモデル、依存ライブラリ、生成物。Git管理対象外 |

依存方向と映像入力の所有権は[アーキテクチャガイド](docs/architecture.md)を参照してください。

## ライセンスとモデルの利用条件

プロジェクトのコードは[Apache-2.0](LICENSE)です。依存コードには各ライセンスが適用されます。[NOTICE](NOTICE)と[licenses](licenses/)を参照してください。
**モデルの重みがApache-2.0になるわけではありません。**

[FAHの重みライセンス](licenses/FAH-WEIGHTS.txt)は、姿勢モデル、検出器、変換後のモデルに適用されます。SAM／DINOv3の継承条件も含みます。
ncnn、ONNX、Core ML、Metal、LiteRTへ変換しても研究・非商用の制限は残ります。
再配布時は重みのライセンスを同梱します。検出器はFAHとUltralytics YOLOv8のクレジットが必要で、適用されるAGPL-3.0の義務にも言及されています。

2026年11月10日の確認では、同梱した重みの利用条件は上流と一致しています。
配布前に[上流の重みの利用条件](https://github.com/noahcao/FAH/blob/main/WEIGHTS_LICENSE)を確認します。
このリポジトリのApacheライセンスを根拠に、商用ゲームやプラグインで現在の重みを使用することはできません。
今回の確認は、個別アプリのAGPL適合性や第三者の商用利用権を保証するものではありません。
ONNX Runtime、CUDA／cuDNN、FFmpeg、ncnn、LiteRTの配布条件も別途適用されます。実際に同梱するバイナリーの告知を保持します。
モデルの形式変換や別配布によって、利用制限がなくなることはありません。
