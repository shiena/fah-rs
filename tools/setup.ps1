param([string]$Sdk = "$env:LOCALAPPDATA\Android\Sdk")
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    New-Item -ItemType Directory -Force models,third_party,artifacts | Out-Null
    if (!(Test-Path .venv/Scripts/python.exe)) {
        python -m venv .venv
        if ($LASTEXITCODE) { throw 'Virtual environment creation failed' }
    }
    & .venv/Scripts/python.exe -m pip install -r requirements-export.txt
    if ($LASTEXITCODE) { throw 'Python dependency installation failed' }
    if (!(Test-Path third_party/FAH/.git)) {
        git clone https://github.com/noahcao/FAH.git third_party/FAH
        if ($LASTEXITCODE) { throw 'FAH clone failed' }
        git -C third_party/FAH checkout 2b13dd9c7fc04ad173a76ab89beac9266d2d59ac
        if ($LASTEXITCODE) { throw 'FAH checkout failed' }
    }
    $revision = git -C third_party/FAH rev-parse HEAD
    if ($revision -ne '2b13dd9c7fc04ad173a76ab89beac9266d2d59ac') { throw 'Unexpected FAH revision' }
    function Fetch-Verified($url, $path, $sha) {
        if (!(Test-Path $path)) { Invoke-WebRequest $url -OutFile $path }
        if ((Get-FileHash $path -Algorithm SHA256).Hash -ne $sha) { throw "Checksum mismatch: $path" }
    }
    Fetch-Verified 'https://huggingface.co/noahcao/FAH/resolve/main/fah-s/model.pt' 'models/model.pt' 'F469C80356391C2993C58B4421EF3EE81C36549AF932CFFED1824A038147484C'
    Fetch-Verified 'https://huggingface.co/noahcao/FAH/resolve/main/shared/detector.pt' 'models/detector.pt' '6924E002703C7B3951BB8C19F9F75DCBFAF395B7CA28490F19BB30AAB8561627'
    Fetch-Verified 'https://github.com/Tencent/ncnn/releases/download/20261020/ncnn-20261020-android-vulkan.zip' 'third_party/ncnn.zip' '6E0C5F9B397FAF32B923DE2EF6210AF9BF375851869DCEFF06A8005331981086'
    if (!(Test-Path third_party/ncnn-20261020-android-vulkan)) { Expand-Archive third_party/ncnn.zip third_party }
    Set-Content local.properties ('sdk.dir=' + $Sdk.Replace('\','/').Replace(':','\:'))
    foreach ($kind in @('pose','detector')) {
        & .venv/Scripts/python.exe tools/export.py $kind
        if ($LASTEXITCODE) { throw "$kind export failed" }
    }
    & .venv/Scripts/python.exe tools/make_fixtures.py
    if ($LASTEXITCODE) { throw 'Fixture generation failed' }
    & .venv/Scripts/python.exe tools/check_host.py
    if ($LASTEXITCODE) { throw 'Host parity check failed' }
} finally { Pop-Location }
