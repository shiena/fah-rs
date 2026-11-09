# ソース構成とホスト連携

Unity・Godot向けのプラグイン化に備え、GUI、アプリケーション処理、共通API、追跡コア、プラットフォーム実装を分離しています。映像入力は推論バックエンドから独立したアダプターです。

## 5層の責務

| 層 | 配置 | 責務 |
|---|---|---|
| GUI | `crates/fah-gui/` | GPUIの描画、操作、表示状態、ファイル選択 |
| 共通インターフェース | `crates/fah-api/` | フレーム・結果、開始停止コマンド、通知、実装差し替え用trait |
| アプリケーション処理 | `crates/fah-application/` | 推論方式の選択、同期セッション、非同期ワーカー、描画用画像の生成、結果保存 |
| 推論・追跡コア | `crates/fah-core/` | 切り出し、座標変換、追跡、再検出、推論実装を呼び出す処理 |
| プラットフォーム固有実装 | `windows/src/platform/`、`app/src/main/rust/src/android.rs` | WindowsのDLL探索・ロード、DirectShow入力、Android JNI・ビットマップ操作 |

推論の実装は、Windowsの`windows/src/inference/`と共有の`crates/fah-ncnn/`、`backends/ncnn/cpp/`にあります。ncnn実装をAndroid配下から移し、WindowsとAndroidが同じ実装を参照する構成にしています。

```text
crates/fah-gui              → fah-api::Controller
                                    ↑ 実装
fah-application::Worker     → fah-application::Session
                                    ↓
                               fah-core::Pipeline
                                    ↓ Inference契約
                         ONNX Runtime / ncnn / Metalアダプター

Worker → InputFactory / FrameSource契約
                     ↑ 実装
          fah-input::StillImage / WindowsのFFmpeg入力

ホストの更新ループ → TrackingSession::process(Frame)
                     （Workerと映像入力は不要）
```

GUIは`fah-api`を通じて操作します。DLL探索、推論モデル生成、FFmpeg起動を直接呼びません。共通API・アプリケーション・コアの各クレートはGPUIやWindows APIに依存しません。Windowsの`platform`モジュールが実装を組み立てます。

AndroidのActivityとJava側のライフサイクル制御は維持しています。JNIは共通APIの`TrackingSession`を呼び出します。Androidも共通アプリケーション層、共有コア、ncnnアダプターを利用します。

自動バックエンド選択の候補は`InferenceFactory::automatic_backends`で指定します。WindowsはCUDA、DirectML、CPUの順、AndroidはVulkan、CPUの順です。共通セッションにWindows固有の選択順はありません。

## 映像入力を差し替える

`fah-api/src/input.rs`で`Frame`、`Source`、`FrameSource`、`InputFactory`を定義しています。

- `crates/fah-input/`は静止画像のデコードを担当します。
- `windows/src/platform/video.rs`はFFmpegの起動、DirectShowカメラ列挙、動画デコードを担当します。
- `FrameSource`はフレームの取得と終了状態を返します。推論処理は持ちません。
- `Worker`は入力を注入して使います。停止時と終了時に入力の所有権を解放します。

カメラをホスト側で管理する場合は、`FrameSource`の実装も不要です。ホストが作った`Frame`を同期セッションへ直接渡せます。

## Unity・Godotの更新ループから使う境界

エンジン向けの接続点は`fah-api::TrackingSession`です。`process`は所有権を持つRGBA画像を受け取り、座標と処理情報を含む`Output`を返します。`reset`を指定すると追跡をリセットします。セッションは`Send`ですが、同じセッションの推論は排他的に呼び出します。

Windowsでは`fah_windows::platform::session`でセッションを生成できます。デスクトップ用の`Controller`や`Worker`、カメラ、GPUIは必要ありません。現在のフレーム契約はCPUメモリー上のRGBA8です。GPUテクスチャの直接受け渡しはまだ実装していません。

ホスト側入力のサンプルは[`windows/examples/host_frame.rs`](../windows/examples/host_frame.rs)です。既存のモデルとランタイムを使い、GUIなしで1フレームを処理します。

```powershell
$env:PATH = (Resolve-Path windows/native/build).Path + ';' + $env:PATH
cargo run -p fah-windows --example host_frame --no-default-features --release --locked -- models models/sample.png directml
```

Windowsクレートの`desktop`機能を無効にすると、GPUIとデスクトップ実行ファイルはビルド対象から外れます。

```powershell
cargo check -p fah-windows --lib --no-default-features --release --locked
cargo tree -p fah-windows --no-default-features
```

Rustのtraitや`serde_json::Value`をそのままDLLの公開ABIにはしません。Apple連携用には`apple/include/fah_apple.h`のC境界を追加しています。セッションハンドル、RGBA8入力、JSON結果、ネットワーク実行コールバックを受け渡します。UnityのP/InvokeやGodotのGDExtension用バインディングは未実装です。

macOSのGUIはWindowsと同じ`fah-gui`を使います。`macos/src/platform.rs`が共通ワーカーへMetal推論とAVFoundation入力を注入します。Swiftの`FAHAppleNative`はRustコアに依存しない動的ライブラリです。

macOS／iOSは移植元のMetal姿勢推論とCore ML検出器を使います。切り出し・座標変換・追跡は同じRust共通層が担当します。Swift側の`FrameInput`で映像入力を分離しています。[Apple向けビルド手順と検証範囲](../apple/README.md)を参照してください。

## 所有権と終了処理

- 同期ホストはセッションを所有し、使い終わったら破棄します。
- デスクトップ版では`Worker`がセッションと映像入力を所有します。`Controller`の破棄時にスレッドを終了・joinします。
- ONNX Runtimeのセッションやテンソルは、対応するDLLをアンロードする前に破棄します。
- Windowsのプロセス終了処理では、すべてのセッションとワーカーを破棄してから`platform::shutdown`を呼びます。これはプロセス全体のVulkan資源を解放するため、ホスト内に利用中のセッションがある間は呼べません。

## ビルドと検証

ルートのCargoワークスペースと`Cargo.lock`で依存関係を管理します。既定の出力先は従来と同じ`windows/target`です。AndroidのCMakeは`CARGO_TARGET_DIR`を指定し、NDK用の出力を分離します。

```powershell
cargo test -p fah-core -p fah-api -p fah-application -p fah-input --release --locked
.\tools\windows.ps1 test
.\gradlew.bat :app:assembleDebug
```

ホスト契約のテストでは、カメラやGUIなしの同期推論、初期化候補の選択、CPU復旧時の結果表示、注入した映像入力の停止・終了時の解放を検証します。

### Android向けにビルドする

対象ABIは`arm64-v8a`です。SDK、NDK、ncnnの準備は[README](../README_ja.md)の手順に従います。Rustの`aarch64-linux-android`ターゲットも必要です。

共通API以下とAndroidアダプターの型検査は、リポジトリのルートで実行します。

```powershell
cargo check -p fah-native -p fah-api -p fah-application -p fah-input --target aarch64-linux-android --release --locked
```

ネイティブライブラリのリンクとAPK生成にはGradleを使います。

```powershell
.\gradlew.bat :app:assembleDebug
```

APKは`app/build/outputs/apk/debug/app-debug.apk`に出力されます。CMakeは共通クレートのソースとマニフェストを監視します。共通層の変更時もRustライブラリを再ビルドします。

Android側の接続点は`app/src/main/rust/src/platform.rs`です。`session`は共通の`TrackingSession`を返します。対応バックエンドはncnnのCPUとVulkanです。CUDAとDirectMLを指定するとエラーを返します。

JNIはBitmapから`Frame::from_rgba`で所有権付きのフレームを生成します。行末のパディングを除去し、推論前にBitmapのロックを解除します。ホストがRGBA8データを用意する場合も、このAPIを使えます。`fah-input`の静止画像入力もAndroidのビルド対象です。カメラ取得は引き続きJava側が担当します。
