# ソース構成とホスト連携

Unity・Godot向けのプラグイン化に備え、GUI、アプリケーション処理、共通API、追跡コア、プラットフォーム実装を分離しています。映像入力は推論バックエンドから独立したアダプターです。

## 5層の責務

| 層 | 配置 | 責務 |
|---|---|---|
| GUI | `windows/src/gui/` | GPUIの描画、操作、表示状態、ファイル選択 |
| 共通インターフェース | `crates/fah-api/` | フレーム・結果、開始停止コマンド、通知、実装差し替え用trait |
| アプリケーション処理 | `crates/fah-application/` | 推論方式の選択、同期セッション、非同期ワーカー、描画用画像の生成、結果保存 |
| 推論・追跡コア | `crates/fah-core/` | 切り出し、座標変換、追跡、再検出、推論実装を呼び出す処理 |
| プラットフォーム固有実装 | `windows/src/platform/`、`app/src/main/rust/src/android.rs` | WindowsのDLL探索・ロード、DirectShow入力、Android JNI・ビットマップ操作 |

推論の実装は、Windowsの`windows/src/inference/`と共有の`crates/fah-ncnn/`、`backends/ncnn/cpp/`にあります。ncnn実装をAndroid配下から移し、WindowsとAndroidが同じ実装を参照する構成にしています。

```text
windows/src/gui             → fah-api::Controller
                                    ↑ 実装
fah-application::Worker     → fah-application::Session
                                    ↓
                               fah-core::Pipeline
                                    ↓ Inference契約
                         ONNX Runtime / ncnnアダプター

Worker → InputFactory / FrameSource契約
                     ↑ 実装
          fah-input::StillImage / WindowsのFFmpeg入力

ホストの更新ループ → TrackingSession::process(Frame)
                     （Workerと映像入力は不要）
```

GUIは`fah-api`を通じて操作します。DLL探索、推論モデル生成、FFmpeg起動を直接呼びません。共通API・アプリケーション・コアの各クレートはGPUIやWindows APIに依存しません。Windowsの`platform`モジュールが実装を組み立てます。

AndroidのActivityとJNIは既存のホスト側アダプターとして維持しています。今回、AndroidのJava UIやライフサイクル制御は書き換えていません。Androidも新しい共有コアとncnnアダプターを利用します。

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

これはRust内のAPI分離です。UnityのP/InvokeやGodotのGDExtensionで使う公開ABIはまだありません。今後のバインディング層では、セッションハンドルとフレーム・結果の所有権を変換し、この境界へ接続します。Rustのtraitや`serde_json::Value`をそのままDLLの公開ABIにはしません。

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
