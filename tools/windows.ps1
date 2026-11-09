param(
    [ValidateSet('build','run','test')][string]$Action = 'build',
    [string]$Models = '',
    [int]$Jobs = 8
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    if (!$Models) { $Models = Join-Path $root 'models' }
    $vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
    if (!(Test-Path $vswhere)) { throw 'Install Visual Studio C++ build tools first.' }
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (!$vs) { throw 'Visual Studio Desktop development with C++ is required.' }
    $devCmd = Join-Path $vs 'Common7/Tools/VsDevCmd.bat'
    & cmd.exe /d /c "`"$devCmd`" -no_logo -arch=x64 -host_arch=x64 && set" | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process') }
    }
    if ($LASTEXITCODE) { throw 'Could not initialize the MSVC environment.' }
    $cmake = Get-Command cmake -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source
    if (!$cmake) { $cmake = "$env:LOCALAPPDATA/Android/Sdk/cmake/3.22.1/bin/cmake.exe" }
    if (!(Test-Path $cmake)) { throw 'CMake 3.22 or later is required on PATH.' }
    if (!(Get-Command ninja -ErrorAction SilentlyContinue)) { throw 'Ninja is required on PATH.' }
    if (!(Test-Path third_party/ncnn-src/.git)) {
        git clone --branch 20261020 --depth 1 https://github.com/Tencent/ncnn.git third_party/ncnn-src
        if ($LASTEXITCODE) { throw 'ncnn clone failed.' }
    }
    $revision = git -C third_party/ncnn-src rev-parse HEAD
    if ($revision -ne '7e36d25cb153092ba87a60530455c7376fedf452') { throw 'Expected ncnn tag 20261020. Use a separate checkout for other versions.' }
    if (!(Test-Path third_party/ncnn-src/glslang/CMakeLists.txt)) {
        git -C third_party/ncnn-src submodule update --init --depth 1 glslang
        if ($LASTEXITCODE) { throw 'glslang checkout failed.' }
    }
    & $cmake -S windows/native -B windows/native/build -G Ninja -DCMAKE_BUILD_TYPE=Release
    if ($LASTEXITCODE) { throw 'Native configuration failed.' }
    & $cmake --build windows/native/build --parallel $Jobs
    if ($LASTEXITCODE) { throw 'Native build failed.' }
    cargo build --manifest-path windows/Cargo.toml --release --locked -j $Jobs
    if ($LASTEXITCODE) { throw 'Rust build failed.' }
    Copy-Item windows/native/build/fah_backend.dll windows/target/release/fah_backend.dll -Force
    if ($Action -eq 'test') {
        $env:PATH = (Join-Path $root 'windows/native/build') + ';' + $env:PATH
        cargo test --manifest-path app/src/main/rust/Cargo.toml
        if ($LASTEXITCODE) { throw 'Shared Rust tests failed.' }
        cargo test --manifest-path windows/Cargo.toml --locked -j $Jobs
        if ($LASTEXITCODE) { throw 'Windows tests failed.' }
    } elseif ($Action -eq 'run') {
        & windows/target/release/fah-windows.exe --models $Models
        if ($LASTEXITCODE) { throw 'Windows app failed.' }
    }
} finally { Pop-Location }
