# macOS用GPUIアプリ

Windows版と同じRust製GPUI画面で、画像・動画・カメラを処理します。
GUIは`crates/fah-gui`、処理制御は`fah-application::Worker`を共有します。
Swift側の`FAHAppleNative`はMetal推論・Core ML検出・AVFoundation入力だけを担当します。
Mac向けのGPUI依存では、文字描画に必要な`font-kit`機能を有効にしています。

## 準備する

Apple silicon Mac、macOS 15以降、Xcode 26.2以降、Rustが必要です。
GPUIの描画シェーダー用にMetal Toolchainも導入します。

```sh
xcodebuild -downloadComponent MetalToolchain
```

モデルは[Apple用モデルの準備手順](../apple/README.md#モデルを準備する)で生成します。
モデルフォルダーに`sample.png`があると［サンプル画像］も使えます。
既存モデルを使う場合は、リポジトリの`models/sample.png`を`models/apple/sample.png`へコピーします。

## ビルドして起動する

リポジトリのルートで実行します。

```sh
python3 macos/build.py --models models/apple
```

`macos/build/FAH.app`を生成して起動します。
初回はSwiftライブラリとRustのGPUIアプリをビルドします。
Xcode 27環境のライブラリ読み込みエラーを避けるため、ビルドスクリプトはRustのシンボル削除を無効にします。
モデルの場所はアプリ内の設定ファイルへ記録します。次回はFinderから`FAH.app`を開けます。
モデルを移動した場合は、［モデルフォルダー］で新しい場所を選択します。

カメラを自動で開始する場合は`--camera`を指定します。

```sh
python3 macos/build.py --models models/apple --camera
```

初回はmacOSのカメラ使用許可を承認します。
［カメラ停止］で入力を解放し、プレースホルダーへ戻ります。
［左右反転］はカメラのプレビューを反転します。

［画像・動画を開く］でファイルを選択できます。
［動画］ボタンへサンプルを割り当てる場合は、ビルド時に`--sample-video FILE.mp4`を追加します。
推論方式は［自動］または［Metal］です。Apple側にはCPUフォールバックはありません。

## 診断する

`--log artifacts/gpui-camera.jsonl`を付けると、GUIが受け取った最大300フレームの結果を記録します。
カメラ画像は保存しません。通常使用時は指定不要です。

GUIを開かない画像入力の確認には、次のコマンドを使います。

```sh
macos/build/FAH.app/Contents/MacOS/fah-macos --models models/apple --input models/sample.png --smoke
```

## 配置と所有権

- `Contents/MacOS/fah-macos`: Rust／GPUI実行ファイル
- `Contents/Frameworks/libFAHAppleNative.dylib`: Swiftの推論・入力アダプター
- `Contents/Resources`: Metalシェーダー、ライセンス、起動時のモデル設定

SwiftライブラリにはRust追跡コアをリンクしません。
推論結果はRustの`Inference`契約へ、映像入力は`FrameSource`契約へ接続します。
SwiftのObjective-Cクラス登録がプロセス終了まで残るため、ネイティブライブラリは途中でアンロードしません。
モデルと映像入力のインスタンスは、ワーカーの終了・入力停止時に解放します。

iOS向けのSwift APIは引き続き`apple`パッケージで利用できます。
以前のAppKitビューアーは`apple/camera-app.py`で実行する確認用ツールとして残しています。

## 検証結果

Apple M3の実機で、GPUIがカメラの結果を300フレーム受信することを確認しました。
サンプル画像と動画30フレームも、共通ワーカー経由で処理できました。
すべてMetalを使用し、検出した手の21関節が有限値であることを確認しています。
詳細は[検証記録](../docs/macos-gpui-validation.json)を参照してください。
