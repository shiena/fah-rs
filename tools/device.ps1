param(
    [ValidateSet('install','check','video','sample','camera')][string]$Action = 'install',
    [string]$Serial = '',
    [string]$Sdk = "$env:LOCALAPPDATA\Android\Sdk"
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$adbExe = Join-Path $Sdk 'platform-tools/adb.exe'
if (!(Test-Path $adbExe)) { throw "ADB not found: $adbExe" }
if (!$Serial) {
    $connected = @(& $adbExe devices | Where-Object { $_ -match '^\S+\s+device$' } | ForEach-Object { ($_ -split '\s+')[0] })
    if ($connected.Count -ne 1) { throw 'Specify -Serial when exactly one authorized device is not connected' }
    $Serial = $connected[0]
}
function Adb {
    & $adbExe -s $Serial @args
    if ($LASTEXITCODE) { throw "ADB failed: $args" }
}
Push-Location $root
try {
    $remote = '/data/local/tmp/fah-android'
    $appFiles = '/sdcard/Android/data/dev.fah.android/files'
    if ($Action -eq 'install' -or $Action -eq 'check' -or $Action -eq 'video') {
        & ./gradlew.bat :app:assembleDebug
        if ($LASTEXITCODE) { throw 'Android build failed' }
    }
    if ($Action -eq 'install') {
        Adb install -r app/build/outputs/apk/debug/app-debug.apk
        Adb shell mkdir -p "$appFiles/models"
        Adb push models/pose.ncnn.param models/pose.ncnn.bin models/detector.ncnn.param models/detector.ncnn.bin models/uv_grid.f32 models/sample.png "$appFiles/models/"
        Adb shell pm grant dev.fah.android android.permission.CAMERA
        Adb shell am start -n dev.fah.android/.MainActivity
    } elseif ($Action -eq 'check' -or $Action -eq 'video') {
        Adb shell am force-stop dev.fah.android
        Adb shell mkdir -p $remote
        $binary = Get-ChildItem app/build/intermediates/cxx/Debug -Recurse -Filter fah-check | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
        Adb push $binary "$remote/"
        Adb push "$Sdk/ndk/30.0.16248370/toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$remote/"
        Adb shell chmod 755 "$remote/fah-check"
        Adb push models/pose.ncnn.param models/pose.ncnn.bin models/detector.ncnn.param models/detector.ncnn.bin models/uv_grid.f32 models/sample.rgba models/fixtures "$remote/"
        if ($Action -eq 'video') {
            & .venv/Scripts/python.exe tools/check_video.py --adb $adbExe --serial $Serial --remote $remote
            if ($LASTEXITCODE) { throw 'Video validation failed' }
            return
        }
        $fixtureFiles = @('img','rays','cond','reference0','reference1','reference2','reference3','reference4','reference5') | ForEach-Object { "models/$_.f32" }
        Adb push @fixtureFiles "$remote/"
        foreach ($backend in @('cpu','gpu')) {
            Adb shell "LD_LIBRARY_PATH=$remote $remote/fah-check $remote $backend 10" > "artifacts/device-$backend.log"
        }
        foreach ($fixture in @('hand_right','hand_mirrored','hand_lower','wide_crop')) {
            Adb shell "LD_LIBRARY_PATH=$remote $remote/fah-check $remote gpu 1 $remote/fixtures/$fixture" > "artifacts/fixture-$fixture.log"
        }
        Adb shell "LD_LIBRARY_PATH=$remote $remote/fah-check $remote image $remote/sample.rgba 640 426 12" > artifacts/device-image.log
        & .venv/Scripts/python.exe tools/summarize_checks.py
        if ($LASTEXITCODE) { throw 'Validation failed' }
    } else {
        Adb shell am force-stop dev.fah.android
        if ($Action -eq 'sample') { Adb shell am start -n dev.fah.android/.MainActivity --ez sample true }
        else { Adb shell am start -n dev.fah.android/.MainActivity }
    }
} finally { Pop-Location }
