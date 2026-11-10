# Androidに組み込んだLiteRTを検証する

Androidアプリと共通C ABIのCPU経路は、LiteRT 2.3.0のCompiledModel APIで姿勢を推論します。検出器はncnn CPUです。
`crates/fah-ncnn-platform`でLinuxと同じ推論方式選択を共有します。Vulkan経路はncnnを使用します。
GPUエラー後のCPU回復処理もLiteRTを使用します。NPUとLiteRT GPUは使用しません。

## ビルドとモデルを準備する

1. [モデル変換手順](litert-performance.md#wslで変換する)で`models/litert/pose.tflite`を生成します。
2. [Androidアプリの手順](../app/README_ja.md)でAPKとモデルをインストールします。
3. 共通C ABIは[ビルド手順](../engines/README.md#androidでビルドする)で生成します。

APKはGoogle Mavenの`com.google.ai.edge.litert:litert:2.3.0`を使用します。
C ABIのビルドは同じ公式AARを取得し、固定したSHA-256を検証します。
配布物は`libfah_engine.so`、`libfah_backend.so`、`libc++_shared.so`、`libLiteRt.so`、`LITERT-LICENSE.txt`です。
AARに含まれるCaffe由来の告知も保持します。アプリのライセンスはAPKの`assets/LITERT-ANDROID.txt`にも含めます。モデルには引き続き[FAHの利用条件](../licenses/FAH-WEIGHTS.txt)が適用されます。

AndroidではAPK内のライブラリから直接読み込む場合があるため、LiteRTはAndroidのライブラリ検索パスで解決します。
独自ホストでは4つのライブラリを同じABIのネイティブライブラリとして配置します。
コマンドラインのサンプルでは`LD_LIBRARY_PATH`を指定します。`FAH_LITERT_LIBRARY`で絶対パスを指定することもできます。
CPU用ライブラリやモデルが不足している場合は、必要なファイルを示すエラーを返します。

## サンプル動画を確認する

NDKの`aarch64-linux-android26-clang`でCホストをビルドします。Windowsでは末尾が`.cmd`です。

```sh
aarch64-linux-android26-clang engines/examples/check.c -Iengines/include -Lengines/build/android -lfah_engine -o engine-check
```

ADBで`engine-check`、4つのライブラリ、モデルを`/data/local/tmp/fah-litert-native/`へ配置します。
モデルは`pose.ncnn.param/bin`、`detector.ncnn.param/bin`、`uv_grid.f32`、`litert/pose.tflite`です。
`engine-check`には実行権限を付けます。FFmpegをPATHへ追加し、アプリを停止してから実行します。

```sh
python tools/check_video.py --adb /path/to/adb --serial SERIAL --remote /data/local/tmp/fah-litert-native --engine /data/local/tmp/fah-litert-native/engine-check --backend cpu
```

`--backend vulkan`または`--backend auto`でも検証できます。
CPUの場合、結果を`artifacts/engine-video-cpu-validation.json`へ保存します。
姿勢推論エンジン、検出器、30フレームの追跡、周期的な再検出、21関節の有限値を検査します。
従来の`tools/device.ps1 -Action check`はncnn単体の検証として残しています。

## 検証結果

Sony SO-51A（Android 12、Adreno 650）で確認しました。

- LiteRTネイティブAPIの5種類の入力が精度検査に合格しました。モデルを破棄・再生成した後にも合格しました。
- APKのCPUサンプル画像で2手を検出しました。JSONの`pose_engine`は`litert`、`detector_engine`は`ncnn`です。
- 共通C ABIのCPU・Vulkan両経路が、サンプル動画30フレームを完走しました。全フレームで手を検出し、各手21関節が有限値でした。
- Vulkan初期化が失敗する構成で、AutoがLiteRT CPUへ切り替わることを確認しました。
- LiteRTライブラリ不足とモデル不足では、CPU初期化エラーを確認しました。

CPU動画の処理時間中央値は約224msでした。これは特定のサンプル動画の値で、カメラやGUI全体のFPSを示すものではありません。
詳細は[検証記録](android-litert-integration.json)を参照してください。今回、カメラ入力は検証していません。
