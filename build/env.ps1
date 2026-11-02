$ErrorActionPreference = 'Stop'
$jdk = 'C:\Program Files\Eclipse Adoptium\jdk-17.0.20.8-hotspot'
$sdk = Join-Path $env:LOCALAPPDATA 'Android\Sdk'
$mingw = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages') -Directory -Filter 'BrechtSanders.WinLibs*' -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty FullName
$rustSelfContained = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained'
$ndk = Get-ChildItem (Join-Path $sdk 'ndk') -Directory |
    Where-Object { $_.Name -like '27.*' } |
    Sort-Object Name -Descending |
    Select-Object -First 1 -ExpandProperty FullName

$env:JAVA_HOME = $jdk
$env:ANDROID_HOME = $sdk
$env:ANDROID_SDK_ROOT = $sdk
$env:NDK_HOME = $ndk
$env:ANDROID_NDK_HOME = $ndk
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$env:PATH = @(
    (Join-Path $mingw 'mingw64\bin'),
    $rustSelfContained,
    (Join-Path $sdk 'platform-tools'),
    (Join-Path $sdk 'cmdline-tools\latest\bin'),
    (Join-Path $jdk 'bin'),
    $env:PATH
) -join ';'
