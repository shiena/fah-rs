# macOS／iOS用Metalアダプター

移植元の[fah-metal](https://github.com/LuckyIYI/fah-metal)を使い、Rust共通層で追跡します。
対象はApple silicon Macとarm64のiOS端末です。ホストが映像入力を管理します。

**検証状況:** Apple M3、macOS 27.0.1、Xcode 27.1でビルドとカメラ推論を確認しました。
カメラ300フレーム中289フレームで手を検出し、最大2手・各21関節を検証しました。
推論時間の中央値は約35.7msでした。Swiftの2件のテストも成功しています。
iOS実機向けとarm64 Simulator向けのビルドは成功しています。iOS実機での推論は未検証です。
数値の詳細は[検証記録](../docs/apple-validation.json)を参照してください。
この変更はライブラリとサンプルの追加です。Windows版GUIの全機能は移植していません。

## 構成

| 層 | 実装 |
|---|---|
| ホスト／GUI | 任意のmacOS・iOSアプリ。`Examples/FAHSampleApp.swift`が最小サンプル |
| 共通API | `fah-api::TrackingSession`。Swift側は`FAHApple.TrackingSession` |
| アプリケーション | `fah-application::Session` |
| 追跡コア | `fah-core`の切り出し、座標変換、再検出、追跡 |
| Appleバックエンド | 移植元の`FAHPoseNetwork`とCore ML検出器を呼ぶ`MetalRuntime.swift` |
| 映像入力 | `FrameInput`でBGRAの`CVPixelBuffer`をRGBA8へ変換 |

移植元のコミット`3ddc269a6a4ea8d921630ac4fc20d7d260375bca`から、姿勢ネットワーク・シェーダー・エクスポーターを`Vendor/fah-metal`へ取り込んでいます。
Simulator SDKで未定義のMetal 4 GPU種別を参照しないよう、Simulatorではtensor opsを無効にしています。
実機とmacOSの選択処理は移植元のままです。変更内容は[UPSTREAM.md](Vendor/fah-metal/UPSTREAM.md)を参照してください。
Metal姿勢推論は移植元の自動選択を使います。対応GPUではtensor ops、その他ではsimdgroup matrixを選びます。
検出器は`detector-640`の`s640x640`関数を使います。
DFLデコード後のNMSはRust共通コアが担当します。

Apple側はMetal専用です。CPUフォールバックはありません。
メッシュ、接触推定、MANOフィッティングは実行しません。
出力は既存のJSON形式で、最大2手、各21関節です。
元のMetalパイプライン全体の速度・精度をそのまま再現する構成ではありません。

## ビルドする

前提はApple silicon Mac、macOS 15以降、iOS 18以降、Xcode 26.2以降、Rust、Python 3です。
固定した移植元が参照する新しいMetal APIのため、ビルドにはXcode 26.2以降を使います。
実行先の最低OSとビルド用SDKの要件は異なります。

1. Mac上のリポジトリルートでRustライブラリをビルドします。

   ```sh
   python3 apple/build.py
   ```

   macOS arm64、iOS arm64、iOS Simulator arm64を含む`apple/Artifacts/FAHCore.xcframework`を生成します。
   macOSだけの場合は`--mac-only`を指定します。この場合、iOS向けには利用できません。

2. Swift側をビルドし、モデル不要の変換テストを実行します。

   ```sh
   swift build --package-path apple -c release
   swift test --package-path apple -c release
   ```

3. iOSのリンクも確認します。

   ```sh
   cd apple
   xcodebuild -scheme FAHApple -destination 'generic/platform=iOS' -derivedDataPath build/ios CODE_SIGNING_ALLOWED=NO build
   xcodebuild -scheme FAHApple -destination 'generic/platform=iOS Simulator' -derivedDataPath build/simulator ARCHS=arm64 CODE_SIGNING_ALLOWED=NO build
   ```

Simulatorはビルド確認用です。GPU・Core MLの動作と性能は実機で確認します。
XCFrameworkにはRust共通層が入ります。Swiftアダプターと移植元のシェーダーリソースも必要です。
アプリにはXCFramework単体ではなく、`apple`のSwift Packageを追加します。

## モデルを準備する

ncnn／ONNX形式のモデルはAppleアダプターで使えません。
このリポジトリの`tools/setup.ps1`で準備したチェックポイントから、MANOを読み込まずにエクスポートできます。
Windowsの既存Python環境では、次のコマンドを使います。

```powershell
.venv/Scripts/python.exe tools/export_apple.py
```

`models/apple-export`をMacへコピーします。Mac上の専用Python環境でCore MLへ変換します。
検証時のバージョンはPython 3.12、PyTorch 2.15.0、coremltools 9.0、NumPy 2.3.5です。
NumPy 2.5.4ではcoremltoolsの変換処理が失敗したため、2.3.5に固定しています。

```sh
uv venv --python 3.12 .venv-apple
uv pip install --python .venv-apple/bin/python torch==2.15.0 coremltools==9.0 numpy==2.3.5
.venv-apple/bin/python tools/apple-convert-detector.py models/apple-export
```

移植元の`tools/export.py`で作ったモデルも利用できます。
移植元のエクスポーターを直接使う場合は、そちらのFAH・MANOの準備手順に従います。

Mac上で、エクスポート済みのフォルダーを変換します。

```sh
python3 tools/apple-models.py models/apple-export models/apple
```

以下を出力します。

- `fah-s-pose.bin`と`fah-s-pose.json`
- Metalの重みから抽出した`uv_grid.f32`
- コンパイル済みの`detector-640.mlmodelc`

Core MLのコンパイルはMac上で実行します。iOSアプリには`.mlmodelc`を含む`models`フォルダーを同梱します。
モデルはリポジトリへコミットしません。元のFAH・MANOのライセンス条件が適用されます。

## サンプルを実行する

このリポジトリの`models/sample.rgba`は640×426のRGBA8画像です。
Macへコピーしてから実行します。

```sh
swift run --package-path apple -c release fah-apple-check models/apple models/sample.rgba 640 426
```

同じ寸法のRGBA8フレームを連結したファイルも処理できます。
`artifacts/sample-video.rgba`がある場合は、サンプル動画の30フレームを検証できます。
各フレームのJSONを標準出力に出力します。

macOS／iOSアプリで確認する場合は、XcodeでSwiftUI Appを作成します。
ローカルの`apple`パッケージを追加し、生成されたAppファイルを`Examples/FAHSampleApp.swift`に置き換えます。
準備したモデルフォルダーを`models`というフォルダー参照で追加します。
その中に`sample.rgba`をコピーし、実機で［Run sample］を実行します。
iOS実機へのインストールにはアプリ側の署名設定が必要です。

## Macのカメラで確認する

次のコマンドでアプリをパッケージ化し、起動します。

```sh
python3 apple/camera-app.py --models models/apple --log artifacts/camera-results.jsonl
```

ログの親フォルダー`artifacts`は事前に作成します。`--log`は省略できます。
初回はmacOSのカメラ使用許可を承認します。
映像に関節を重ねて表示し、［Stop camera］で停止、［Start camera］で再開します。
停止するとプレビューをクリアします。カメラ画像は保存しません。
`--log`を指定した場合は、開始ごとに最大300フレームのJSON結果を追記します。

アプリは`apple/build/FAH Camera.app`に生成されます。
モデルの場所は起動引数で指定するため、再起動も上記のコマンドを使います。

## ホストの映像入力を使う

```swift
import FAHApple

let session = try TrackingSession(models: modelsURL)
let resultJSON = try session.process(pixelBuffer: pixelBuffer, reset: false)
```

キャプチャ側で`kCVPixelFormatType_32BGRA`を指定します。
入力画像の回転はホスト側で正立方向へ補正します。
このアダプターはNV12入力やARKitのカメラ内部パラメーターを直接受け取りません。
2D座標は渡した画像のピクセル座標です。3D座標は共通コアが推定したカメラ座標です。
推論はUIスレッド以外で実行します。同一セッションの処理は内部で直列化します。

RGBA8データを直接渡す場合は`process(rgba:width:height:stride:reset:)`を使います。
フレームはRust側へコピーされるため、GPUテクスチャのゼロコピー経路ではありません。

## C境界と所有権

`include/fah_apple.h`でバージョン1のC境界を定義しています。
Swiftはネットワーク実行用コールバックを登録します。Rustは共通セッションを所有します。
有効なコールバックのコンテキストは初期化の成否にかかわらずRustへ移譲されます。
同じセッションの破棄と処理を並行して呼び出してはいけません。
返されたJSONとエラー文字列は`fah_apple_string_free`で解放します。
Swiftラッパーでは、これらの解放を自動で実行します。

Unity／Godot向けのバインディングは別途必要です。
このC境界を使う場合も、Apple側のコールバックとシェーダーリソースを組み込みます。

## Windowsで実行できる検証

```powershell
cargo test -p fah-apple -p fah-api -p fah-application --release --locked
rustup target add aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim
cargo check -p fah-apple --target aarch64-apple-darwin --release --locked
cargo check -p fah-apple --target aarch64-apple-ios --release --locked
cargo check -p fah-apple --target aarch64-apple-ios-sim --release --locked
python -m unittest discover -s apple/python_tests
```

これらはSwift・MetalのビルドやApple実機の推論を検証しません。
Appleでの最終確認には、上記のビルドとサンプル実行が必要です。

XCFrameworkの構成は[Appleの作成手順](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle)に従っています。
