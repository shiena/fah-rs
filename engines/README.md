# Unity・Godot向けの共通コア

`fah-ffi`は、GUIなしで手の追跡処理を呼び出すC ABIです。
Windows、Android、macOS、iOS、Linuxで[同じヘッダー](include/fah_engine.h)を使用します。
エンジン本体のプラグイン登録やカメラ制御は、ホスト側で実装します。

## APIを呼び出す

1. `fah_engine_abi_version()`が`FAH_ENGINE_ABI`と一致することを確認します。
2. `FahConfig`にバージョン、`sizeof(FahConfig)`、モデルフォルダー、バックエンドを設定します。
3. `fah_engine_create`でセッションを作成します。
4. ワーカースレッドから`fah_engine_process`へRGBA8画像を渡します。
5. `fah_engine_result_data`で固定長の関節データを取得します。JSONが必要なら`fah_engine_result_json`を呼びます。
6. 各結果を`fah_engine_result_destroy`で解放します。
7. ホストの推論タスクを終了し、`fah_engine_destroy`でセッションを破棄します。
8. 全セッションと結果の解放後、`fah_engine_shutdown`を呼びます。

最後の終了処理はWindowsとLinuxのVulkan状態を解放します。ライブラリのアンロード前とアプリ終了前に実行します。
同じ`fah_backend`を使う別のホストも、この時点で処理を終了している必要があります。

セッションと結果は64ビットの整数ハンドルです。解放済みのハンドルはエラーになります。
結果はセッションを破棄した後も有効です。ホストへライブラリ所有のポインターは返しません。
JSONは必要な容量を問い合わせてから、ホスト所有のバッファへコピーします。容量不足時の再取得で推論をやり直す必要はありません。

同じセッションへの処理は直列化します。異なるセッションは並行して呼び出せます。
破棄と同時に開始済みの呼び出しは完了する場合があります。アンロード前にはホスト側のタスクを必ず終了します。
`FAH_ERROR`の場合は、同じスレッドで直後に`fah_engine_last_error`を読み出します。
NULLやサイズ、バージョンは検証しますが、不正なメモリーアドレスへのアクセスは回復できません。

## 画像と座標を扱う

- 入力はCPUメモリー上のRGBA8です。左上原点、行順、正のバイト単位ストライドを指定します。
- 画像の回転、左右反転、GPUテクスチャの読み出しはホスト側の責務です。
- `reset=1`で追跡を初期化します。動画のシークや入力元の変更時に指定します。
- 最大2つの手について、それぞれ21関節を返します。手の永続IDはありません。
- 2D座標は入力画像のピクセル座標です。画像外の推定値もあり、範囲内には丸めません。
- 3D座標はモデルが推定したカメラ座標系のメートル単位です。画像に対して右がX正、下がY正、前方がZ正です。
- 3D座標はエンジンのワールド座標ではありません。軸、原点、カメラ姿勢の変換はホスト側で行います。
- 構造体はネイティブのアラインメントを使用します。`Pack=1`は指定しません。

## OSごとにビルドする

コマンドはリポジトリのルートで実行します。生成物は`engines/build`に配置します。
モデルの準備は[Windows](../windows/README.md)または[Apple](../apple/README.md)の手順に従います。
モデルはビルド成果物へ自動で同梱しません。

| OS | 対象 | 推論バックエンド | 成果物 |
|---|---|---|---|
| Windows | x64 | CUDA／DirectML／CPU／Vulkan | `windows/fah_engine.dll`、`fah_backend.dll` |
| Linux | x86_64 | Vulkan／CPU | `linux/libfah_engine.so`、`libfah_backend.so` |
| Android | arm64-v8a、API 26以降 | Vulkan／CPU | `android/libfah_engine.so`、`libfah_backend.so`、`libc++_shared.so` |
| macOS | Apple silicon、macOS 15以降 | Metal／Core ML | `macos/libfah_engine.dylib`、`libFAHAppleNative.dylib`、リソースバンドル |
| iOS | arm64、iOS 18以降 | Metal／Core ML | `ios/libfah_engine.a`とSwiftパッケージの`FAHAppleNative` |

バックエンドの値は、Auto=0、CPU=1、Vulkan=2、CUDA=3、DirectML=4、Metal=5です。
Autoの候補はWindowsがCUDA→DirectML→CPU、AndroidとLinuxがVulkan→CPU、AppleがMetalです。
AppleのCPUフォールバックはありません。

### Windowsでビルドする

MSVC、Rustと、ビルド済みの`windows/native/build/fah_backend.lib`が必要です。
ネイティブ部分の準備は[Windows手順](../windows/README.md)を参照してください。

```powershell
python engines/build.py --platform windows
```

ホストが`fah_backend.dll`を読み出せるよう、依存DLLの検索パスも設定します。
ONNX Runtimeは別途配置します。`FAH_ORT_DML`と`FAH_ORT_CUDA`で、使用する`onnxruntime.dll`の絶対パスを指定できます。
同じディレクトリへ対応するプロバイダーDLLも配置します。CUDAランタイムとcuDNNの導入は[既存の手順](../windows/README.md#install-inference-runtimes)に従います。
CUDAの利用可否は従来どおりライブラリの存在確認後に判断します。

### Androidでビルドする

Rustの`aarch64-linux-android`ターゲット、NDK 30.0.16248370、CMake、Ninjaが必要です。
`third_party/ncnn-20261020-android-vulkan`も準備します。既存アプリと同じNDKを使います。

```sh
rustup target add aarch64-linux-android
python engines/build.py --platform android --ndk /absolute/path/to/ndk/30.0.16248370
```

生成した3つの`.so`を同じABI向けに組み込みます。既存の`libc++_shared.so`との版の競合を確認します。
モデルはAPK内の圧縮アセットのまま渡さず、アプリから読み出せる実ファイルへ展開します。

### Linuxでビルドする

GCCまたはClang、CMake 3.22以降、Ninja、Rust、Vulkan開発ヘッダーが必要です。
ncnnはWindowsと同じリビジョンを使用します。

```sh
git clone --branch 20261020 --depth 1 --recurse-submodules --shallow-submodules https://github.com/Tencent/ncnn.git third_party/ncnn-src
python3 engines/build.py --platform linux
```

既存の`third_party/ncnn-src`がある場合はcloneを省略します。
使用するリビジョンは`7e36d25cb153092ba87a60530455c7376fedf452`です。
生成した2つの`.so`は同じディレクトリへ置きます。`libfah_engine.so`は自身の配置先からバックエンドを読み込みます。
実行先には互換性のあるglibc、libstdc++、libgompが必要です。Vulkanを使う場合はVulkanローダーとGPUドライバーも必要です。
配布先より新しいglibcを要求しない環境でビルドします。別アーキテクチャ向けにビルドした成果物は使用できません。

Cホストの確認例です。最後の値はCPU=1、Vulkan=2、Auto=0です。

```sh
cc engines/examples/check.c -Iengines/include -Lengines/build/linux -lfah_engine -Wl,-rpath,'$ORIGIN' -o engines/build/linux/engine-check
engines/build/linux/engine-check models models/sample.rgba 640 426 2
```

Steam Deck向けの検証では、WSLのUbuntu 24.04でビルドした成果物をSteamOSへ転送します。
WSLには`build-essential`、`cmake`、`ninja-build`、`pkg-config`、`libvulkan-dev`、`python3`、Rustを導入します。
ビルド速度のため、ソースは`/mnt/c`ではなくWSLのホームディレクトリ配下に置きます。
`python3 engines/build.py --platform linux --jobs 12`のように並列数を指定できます（既定は4）。
GPU推論の検証は、SteamOSのGPUドライバーを使って実機上で実施します。

カメラがない端末では、FFmpegで動画を連続RGBAフレームに変換します。
Cホストはファイルの終端まで同じセッションで追跡し、フレームごとにJSONを出力します。

```sh
ffmpeg -i sample.mp4 -vf 'fps=10,scale=640:384' -frames:v 60 -pix_fmt rgba -f rawvideo sample-video.rgba
./engine-check models sample-video.rgba 640 384 2 > video-results.jsonl
```

Linux向けのGPUI画面やカメラ入力は、このコア用ビルドには含みません。

### Macでビルドする

Apple silicon Mac、Xcode、Rustが必要です。GPUIとMetal Toolchainの追加ダウンロードはコアのビルドには不要です。

```sh
python3 engines/build.py --platform macos
```

`FahConfig.native_library_utf8`に`libFAHAppleNative.dylib`の絶対パスを渡します。
`fah-metal_FAHMetal.bundle`はこのSwiftライブラリと同じディレクトリに置きます。
SwiftライブラリはObjective-Cクラス登録が残るため、プロセス終了までロード状態を保ちます。
モデルやセッションは通常どおり破棄できます。
配布用アプリの署名時には、同梱したライブラリも対象にします。

### iOSでビルド・検証する

Macで次を実行すると、共通C ABIの静的ライブラリを生成します。

```sh
python3 engines/ios/build.py
```

ホストへ`libfah_engine.a`と`engines/include/fah_engine.h`を組み込みます。
Swiftパッケージ`apple`の`FAHAppleNative`製品もリンク・埋め込みします。
Swiftのフレームワークとシェーダーバンドルもアプリに含めます。Rustライブラリ単独では推論できません。
C#のiOS実機ビルドでは、呼び出し先ライブラリ名を`__Internal`にします。

実機確認用アプリをビルドする場合は、Apple DevelopmentのチームIDを指定します。
モデルの準備後、Macにログインした状態のターミナルで実行します。

```sh
python3 engines/ios/build.py --team YOUR_TEAM_ID
```

`engines/ios/FAHEngineCheck.xcodeproj`を生成し、署名付きのアプリをビルドします。
サンプル画像は640×426の`sample.rgba`を使用します。Core MLモデルは初回起動時に端末上でコンパイルします。
SSH経由で`errSecInternalComponent`が出る場合は、ログイン中のMacのターミナルまたはXcodeから署名します。

```sh
xcrun devicectl device install app --device DEVICE_ID engines/build/ios/app/Build/Products/Release-iphoneos/FAHEngineCheck.app
xcrun devicectl device process launch --device DEVICE_ID dev.fah.enginecheck
```

画面に`C ABI OK`と手の数が表示されます。結果はアプリのDocumentsに`engine-result.json`として保存します。
この確認アプリは静止画像用です。iOSカメラとUnity／Godot本体の統合テストは含みません。

## Unity・Godotへ接続する

Unity向けの呼び出し例は[`FahSession.cs`](examples/FahSession.cs)です。
プラットフォームに対応するライブラリをネイティブプラグインとして配置し、ワーカースレッドから`ProcessJson`を呼びます。
`Dispose`で各セッションを破棄し、全タスク終了後に`FahSession.Shutdown()`を呼びます。
Unityオブジェクトの更新はメインスレッドへ戻してから行います。
この例は.NETのP/Invokeで検証しています。Unity Editor、IL2CPPでのビルドは未検証です。

GodotではGDExtension側から同じCヘッダーをインクルードします。
呼び出し順序とバッファ管理の例は[`check.c`](examples/check.c)にあります。
結果をGodotの型へ変換し、メインスレッドへ返します。
GDExtensionのクラス登録、`.gdextension`設定、エクスポート設定はまだ含みません。

## 検証範囲

検証記録は[`validation.json`](validation.json)を参照してください。
Cホスト、C# P/Invoke、iOS確認アプリは共通C ABIを通して推論します。
Androidは共有ライブラリとCホストのクロスビルドまでです。今回の共通C ABIによるAndroid実機推論は未検証です。
