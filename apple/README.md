# macOS／iOS用Metalアダプター

移植元の[fah-metal](https://github.com/LuckyIYI/fah-metal)を使い、Rust共通層で追跡します。
対象はApple silicon Macとarm64のiOS端末です。ホストが映像入力を管理します。

**検証状況:** Windows上でRustの単体テストとAppleの3ターゲット向け静的ライブラリ生成を実施しています。
Swift・Metalのビルド、Apple実機での推論・精度・性能は未検証です。
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

移植元はコミット`3ddc269a6a4ea8d921630ac4fc20d7d260375bca`に固定しています。
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
   xcodebuild -scheme FAHApple -destination 'generic/platform=iOS Simulator' -derivedDataPath build/simulator CODE_SIGNING_ALLOWED=NO build
   ```

Simulatorはビルド確認用です。GPU・Core MLの動作と性能は実機で確認します。
XCFrameworkにはRust共通層が入ります。Swiftアダプターと移植元のシェーダーリソースも必要です。
アプリにはXCFramework単体ではなく、`apple`のSwift Packageを追加します。

## モデルを準備する

ncnn／ONNX形式のモデルはAppleアダプターで使えません。
[fah-metalのセットアップ](https://github.com/LuckyIYI/fah-metal/tree/3ddc269a6a4ea8d921630ac4fc20d7d260375bca#setup)に従い、モデルをエクスポートします。
移植元のエクスポーターにはFAH一式とMANOの準備が必要です。
本アダプターの実行時には、エクスポートされたMANOデータは使いません。

Mac上で、エクスポート済みのフォルダーを変換します。

```sh
python3 tools/apple-models.py /path/to/fah-metal/models models/apple
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
