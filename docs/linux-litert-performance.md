# Steam DeckでLiteRT CPUを比較する

Steam Deckで、ncnn CPU、ncnn Vulkan、LiteRT CPUの姿勢推論を比較しました。
LiteRT CPUはncnn CPUより約28〜30%短い時間で推論できました。
ただし、ncnn Vulkanが最速でした。GPUを利用できる場合の全面的な置き換えにはなりません。

## 計測条件

- 実行先: Steam Deck、SteamOS、AMD Custom APU 0405
- ncnn: 既存のLinux向けライブラリ。WSL Ubuntu 24.04でビルドしたものを使用
- LiteRT: `ai-edge-litert==2.3.0`、CPUのXNNPACK経路
- モデル: FAHの姿勢モデル、FP32、同じ重みと入力
- CPUスレッド数: 両エンジンとも4
- 入力: 主入力と`hand_lower`、`hand_mirrored`、`hand_right`、`wide_crop`の5種類
- ウォームアップ: 入力ごとに2回
- 計測: 各入力で10回×3組。初期化と前処理を除き、推論とホスト側のテンソルコピーを含む
- 精度: 2D出力の最大誤差0.5ピクセル以内（320ピクセル換算）、3D出力の最大誤差1mm以内

各エンジンは別プロセスで順番に実行しました。
計測順を逆にした再測定も実施しました。
最後に主入力でウォームアップを10回に増やし、30回の推論を計測しました。
サンプル動画全体の追跡速度やGUIのFPSを示す値ではありません。
今回の比較では、カメラを使用していません。

## 結果

主入力のウォームアップを10回に増やした最終計測の中央値です。
精度の欄は、5入力すべての検査結果を示します。

| 実行経路 | 主入力の中央値 | 5入力の精度検査 |
|---|---:|---|
| ncnn CPU | 197.06ms | 全件合格 |
| LiteRT CPU | 141.79ms | 全件合格 |
| ncnn Vulkan | 123.79ms | 全件合格 |

LiteRT CPUは最終計測でncnn CPUより28.05%短い時間で推論できました。
一方、ncnn Vulkanに対しては14.54%長くかかりました。
初回計測でもCPU間の短縮率は約30%で、改善傾向は再現しました。
逆順の再計測では開始直後のVulkanが153msまで変動しました。
ウォームアップを増やすと124msに戻りました。
この端末ではVulkanを優先し、CPU実行を選ぶ場合にLiteRTを検討する方針が適切です。
この比較を基に、Linuxの共通C ABIへLiteRT CPUの姿勢推論を組み込みました。
Autoはncnn Vulkanを優先します。CPU指定およびCPUへの回復時にはLiteRTを使用し、検出器はncnnのままです。
ネイティブ実装の検証結果は[組み込み検証](../engines/linux/litert-validation.json)を参照してください。
比較表はPythonのInterpreter APIによる比較時の記録です。組み込み版はネイティブCompiledModel APIをCPU指定で使用します。

測定値、精度誤差、使用ライブラリのSHA-256は[検証結果](linux-litert-validation.json)に記録しています。
Androidで必要だったRoPE定数のコピーを適用した同じLiteRTモデルを使用しました。
変換方法は[LiteRTの変換手順](litert-performance.md#wslで変換する)を参照してください。

## 再計測する

Linux実機に次のファイルを配置します。

- `tools/check_linux_inference.py`
- `engines/build/linux/libfah_backend.so`。ビルドは[Linux向け手順](../engines/README.md#linuxでビルドする)を参照
- `models/pose.ncnn.param`と`models/pose.ncnn.bin`
- `models/litert/pose.tflite`と`models/litert/manifest.json`
- `models`と`models/fixtures/*`内の`img.f32`、`rays.f32`、`cond.f32`、`reference0.f32`〜`reference5.f32`

実機のホームディレクトリ配下に専用のPython環境を作成します。
SteamOSのシステム領域を変更する必要はありません。

```sh
python3 -m venv artifacts/litert-linux-env
artifacts/litert-linux-env/bin/python -m pip install ai-edge-litert==2.3.0 numpy==2.4.6
```

リポジトリのルートで次を実行します。他の推論処理と同時に実行しないようにします。

```sh
for backend in ncnn_cpu litert_cpu ncnn_vulkan; do
  artifacts/litert-linux-env/bin/python tools/check_linux_inference.py \
    --backend "$backend" --output "artifacts/linux-litert/$backend.json"
done
```

ライブラリの配置先が異なる場合は、`--native /absolute/path/libfah_backend.so`を指定します。
`--rounds 1`では計測を10回×1組に変更できます。
最終計測は`--fixture models --warmup-runs 10 --runs 30 --rounds 1`を指定しました。
結果の`fixtures`内で、全項目の`passed`が`true`であることを確認します。
精度検査に失敗すると、結果を保存して終了コード2を返します。
