# Builds the Android APK for the day planner on Windows.
# Usage:
#   .\build-apk.ps1                 # signed release, arm64 + armv7
#   .\build-apk.ps1 -DebugBuild          # debug APK (also installable)
#   .\build-apk.ps1 -AllAbis        # arm64, armv7, x86, x86_64
#
# The script only sets environment variables for its own process; nothing is
# written to the system environment.

[CmdletBinding()]
param(
    [switch]$DebugBuild,
    [switch]$AllAbis,
    [string[]]$Targets = @('aarch64', 'armv7')
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$app = Join-Path $root 'app'

function Find-Jdk {
    $candidates = @()
    if ($env:JAVA_HOME) { $candidates += $env:JAVA_HOME }
    $candidates += (Get-ChildItem 'C:\Program Files\Eclipse Adoptium' -Directory -Filter 'jdk-17*' -ErrorAction SilentlyContinue | ForEach-Object FullName)
    $candidates += (Get-ChildItem 'C:\Program Files\Java' -Directory -Filter 'jdk-17*' -ErrorAction SilentlyContinue | ForEach-Object FullName)
    $candidates += (Get-ChildItem 'C:\Program Files\Android\openjdk' -Directory -Filter 'jdk-17*' -ErrorAction SilentlyContinue | ForEach-Object FullName)
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path (Join-Path $candidate 'bin\javac.exe'))) { return $candidate }
    }
    throw 'JDK 17 was not found. Install it or set JAVA_HOME.'
}

$jdk = Find-Jdk
$env:JAVA_HOME = $jdk

$sdk = $env:ANDROID_HOME
if (-not $sdk -or -not (Test-Path (Join-Path $sdk 'cmdline-tools'))) {
    $sdk = Join-Path $env:LOCALAPPDATA 'Android\Sdk'
}
if (-not (Test-Path (Join-Path $sdk 'cmdline-tools'))) {
    throw "Android SDK was not found. Expected: $sdk"
}
$env:ANDROID_HOME = $sdk
$env:ANDROID_SDK_ROOT = $sdk

$ndk = Get-ChildItem (Join-Path $sdk 'ndk') -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like '27.*' } |
    Sort-Object Name -Descending |
    Select-Object -First 1
if (-not $ndk) {
    $ndk = Get-ChildItem (Join-Path $sdk 'ndk') -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending |
        Select-Object -First 1
}
if (-not $ndk) {
    throw "Android NDK was not found in $sdk\ndk. Install it with: sdkmanager `"ndk;27.2.12479018`""
}
$env:NDK_HOME = $ndk.FullName
$env:ANDROID_NDK_HOME = $ndk.FullName

# Rust's MSVC host toolchain needs Visual Studio C++ Build Tools. If they are
# missing, the GNU host toolchain works without extra privileges.
$targetMap = @{
    aarch64 = 'aarch64-linux-android'
    armv7   = 'armv7-linux-androideabi'
    i686    = 'i686-linux-android'
    x86_64  = 'x86_64-linux-android'
}

if ($AllAbis) { $Targets = @('aarch64', 'armv7', 'i686', 'x86_64') }
foreach ($target in $Targets) {
    if (-not $targetMap.ContainsKey($target)) {
        throw "Unknown target '$target'. Use aarch64, armv7, i686 or x86_64."
    }
}

$gnuToolchain = 'stable-x86_64-pc-windows-gnu'
$installedToolchains = & rustup toolchain list
if (-not ($installedToolchains | Select-String -SimpleMatch $gnuToolchain)) {
    Write-Host "Installing Rust toolchain $gnuToolchain..."
    & rustup toolchain install $gnuToolchain --profile minimal
}

$installedTargets = & rustup target list --toolchain $gnuToolchain --installed
foreach ($target in $Targets) {
    $rustTarget = $targetMap[$target]
    if (-not ($installedTargets | Select-String -SimpleMatch $rustTarget)) {
        Write-Host "Adding Rust target $rustTarget..."
        & rustup target add --toolchain $gnuToolchain $rustTarget
    }
}

$mingwPackage = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages') -Directory -Filter 'BrechtSanders.WinLibs*' -ErrorAction SilentlyContinue |
    Select-Object -First 1
if (-not $mingwPackage) {
    throw 'WinLibs (MinGW) was not found. Install it with: winget install BrechtSanders.WinLibs.POSIX.UCRT'
}
$mingwBin = Join-Path $mingwPackage.FullName 'mingw64\bin'
if (-not (Test-Path (Join-Path $mingwBin 'as.exe'))) {
    throw "MinGW assembler was not found in $mingwBin"
}

$rustSelfContained = Join-Path $env:USERPROFILE ".rustup\toolchains\$gnuToolchain\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained"
if (-not (Test-Path (Join-Path $rustSelfContained 'dlltool.exe'))) {
    throw "Rust MinGW helper tools were not found in $rustSelfContained"
}

$env:RUSTUP_TOOLCHAIN = $gnuToolchain
$env:PATH = @(
    $mingwBin,
    $rustSelfContained,
    (Join-Path $sdk 'platform-tools'),
    (Join-Path $sdk 'cmdline-tools\latest\bin'),
    (Join-Path $jdk 'bin'),
    $env:PATH
) -join ';'

Push-Location $app
try {
    if (-not (Test-Path (Join-Path $app 'node_modules'))) {
        Write-Host 'Installing npm dependencies...'
        & npm.cmd install
        if ($LASTEXITCODE -ne 0) { throw 'npm install failed.' }
    }

    if (-not (Test-Path (Join-Path $app 'src-tauri\icons'))) {
        Write-Host 'Generating application icons...'
        & npm.cmd run icons
        if ($LASTEXITCODE -ne 0) { throw 'Icon generation failed.' }
    }

    if (-not (Test-Path (Join-Path $app 'src-tauri\gen\android'))) {
        Write-Host 'Generating the Android project...'
        & npx.cmd tauri android init --ci
        if ($LASTEXITCODE -ne 0) { throw 'tauri android init failed.' }
        Write-Warning 'The Android project was regenerated. Re-add the signing config from BUILD_ANDROID.md if needed.'
    }

    $buildArgs = @('tauri', 'android', 'build', '--apk', '--ci')
    if ($DebugBuild) { $buildArgs += '--debug' }
    $buildArgs += '--target'
    $buildArgs += $Targets

    Write-Host "Building APK ($($Targets -join ', '), $(if ($DebugBuild) { 'debug' } else { 'release' }))..."
    & npx.cmd @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'tauri android build failed.' }

    $variant = if ($DebugBuild) { 'debug' } else { 'release' }
    $apkDir = Join-Path $app "src-tauri\gen\android\app\build\outputs\apk\universal\$variant"
    $apk = Get-ChildItem $apkDir -Filter '*.apk' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $apk) { throw "APK was not found in $apkDir" }

    $version = (Get-Content (Join-Path $app 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
    $suffix = if ($DebugBuild) { '-debug' } else { '' }
    $dist = Join-Path $root '..\..\dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $output = Join-Path $dist "dayplanner-android-$version$suffix.apk"
    Copy-Item $apk.FullName $output -Force

    Write-Host ''
    Write-Host "APK: $output"
    Write-Host "Size: $([math]::Round((Get-Item $output).Length / 1MB, 1)) MB"
    Write-Host ''

    $apksigner = Get-ChildItem (Join-Path $sdk 'build-tools') -Filter 'apksigner.bat' -Recurse -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $DebugBuild -and $apksigner) {
        & $apksigner.FullName verify $output
        if ($LASTEXITCODE -ne 0) {
            Write-Warning 'The release APK is not signed. See BUILD_ANDROID.md.'
        } else {
            Write-Host 'Signature: OK'
        }
    }
}
finally {
    Pop-Location
}

exit 0



