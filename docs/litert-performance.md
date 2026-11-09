# AndroidでLiteRTの速度と精度を比較する

SO-51A（Adreno 650、Android 12）でFAHの姿勢モデルを比較しました。
LiteRTのCPU実行では、ncnn CPUより約19%短い時間で推論できました。
CPUは既存の検証入力5種類で精度検査を通過しました。
今回のGPU Delegateは精度検査に失敗したため、採用対象にしていません。

これは姿勢モデル単体の比較です。検出器、動画の追跡処理、GUIのFPSは計測していません。
Androidアプリと共通C ABIの既定エンジンはncnnのままです。
LiteRTの新しいCompiledModel APIとNPU経路は、この検証には含みません。

## 比較条件

- CPUは4スレッド、モデルはFP32です。
- 既存の`img.f32`、`rays.f32`、`cond.f32`を入力します。
- モデル読み込みを除き、ウォームアップ2回の後に推論時間を計測します。
- 2D出力は320ピクセル換算で最大0.5ピクセル、3D出力は最大1mmの誤差まで許容します。既存のncnn検査と同じ基準です。
- 主入力と`hand_lower`、`hand_mirrored`、`hand_right`、`wide_crop`を検証します。

両エンジンの計測回数を10回に揃えた最終結果です。

| 実行経路 | 中央値 | 精度 |
|---|---:|---|
| ncnn CPU | 278.01ms | 合格 |
| ncnn Vulkan | 1,539.51ms | 合格 |
| LiteRT CPU（XNNPACK） | 224.51ms | 合格 |
| LiteRT GPU（OpenCL、CPU併用） | 242.45ms | 不合格 |

LiteRT CPUはncnn CPUに対して処理時間を19.24%短縮しました。
姿勢モデル単体では約3.60回/秒から約4.45回/秒に相当します。
アプリ全体のFPS向上を示す値ではありません。

数値とバイナリーのSHA-256は[検証結果](litert-validation.json)に記録しています。
GPU Delegateでは110演算がGPU、1,048演算がCPUで実行されました。
未対応のブロードキャストがあり、GPU単独の速度ではありません。
OpenGLを明示した場合は、バッチサイズの不一致で初期化に失敗しました。

## モデル変換時の精度問題

LiteRT Torch 0.10.0でそのまま変換すると、RoPEのsinとcosが同じ定数になりました。
元モデルでは、この2つのテンソルが同じストレージの異なる領域を参照しています。
変換前に`clone().contiguous()`で独立した定数にすると、CPUの精度検査を通過しました。
変換スクリプトは5種類の入力を検証し、不一致がある場合は終了コードを非ゼロにします。

この修正後もAndroidのOpenCL GPU Delegateでは大きな精度不一致が残りました。
GPU固有の不一致の原因は未解決です。GPUの計測時間を有効な高速化結果として扱いません。

## WSLで変換する

既存のセットアップで`models/model.pt`、検証用の`.f32`ファイル、`models/fixtures`、`third_party/FAH`を準備します。
変換にはLinuxとPython 3.11を使用します。既存のncnn変換環境とは別の仮想環境を作成します。

```sh
uv venv --python 3.11 .venv-litert
uv pip install --python .venv-litert/bin/python -r requirements-litert.txt
.venv-litert/bin/python tools/export_litert.py
```

`models/litert/pose.tflite`と`manifest.json`が生成されます。
`manifest.json`の`host_validation`で全項目の`passed`が`true`であることを確認します。

## Android実機で検証する

[公式のベンチマーク手順](https://developers.google.com/edge/litert/models/measurement)にあるInterpreter API用ツールを使用します。
今回使用したarm64バイナリーの配布先は次のとおりです。

```text
https://storage.googleapis.com/tensorflow-nightly-public/prod/tensorflow/release/lite/tools/nightly/latest/android_aarch64_benchmark_model
```

Windows側ではNumPyを導入済みの既存Python環境とADBを使用します。
他の推論アプリを終了してから、端末のシリアル番号を指定します。

```powershell
Invoke-WebRequest 'https://storage.googleapis.com/tensorflow-nightly-public/prod/tensorflow/release/lite/tools/nightly/latest/android_aarch64_benchmark_model' -OutFile artifacts/litert-benchmark-model
.venv/Scripts/python.exe tools/check_litert.py --adb "$env:LOCALAPPDATA/Android/Sdk/platform-tools/adb.exe" --serial SERIAL --binary artifacts/litert-benchmark-model --backend cpu
```

結果は`artifacts/litert-check/validation.json`に保存されます。
各入力のログと出力バイナリーも同じフォルダーに保存されます。
`--backend all`ではCPUとGPUを比較します。精度不一致があれば結果を保存して終了コード2を返します。
配布URLはnightly版です。再計測時はバイナリーのSHA-256も比較します。
