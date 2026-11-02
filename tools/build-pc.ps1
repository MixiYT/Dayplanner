[CmdletBinding()]
param([switch]$NoBundle)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $root 'build\env.ps1')
$app = Join-Path $root 'build\dayplanner-pc\app'
$dist = Join-Path $root 'dist'
$version = (Get-Content (Join-Path $app 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
Push-Location $app
try {
    if (-not (Test-Path 'node_modules')) {
        & npm.cmd ci
        if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    }
    if (-not (Test-Path 'src-tauri\icons\icon.ico')) {
        & npm.cmd run icons
        if ($LASTEXITCODE -ne 0) { throw 'Icon generation failed' }
    }
    $buildArgs = @('run', 'tauri', '--', 'build')
    if ($NoBundle) { $buildArgs += '--no-bundle' }
    else { $buildArgs += @('--bundles', 'nsis,msi') }
    & npm.cmd @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'Windows build failed' }

    New-Item -ItemType Directory -Force $dist | Out-Null
    Copy-Item 'src-tauri\target\release\dayplanner.exe' (Join-Path $dist "dayplanner-pc-$version-x64.exe") -Force
    if (-not $NoBundle) {
        foreach ($spec in @(@('nsis', 'exe', 'x64-setup.exe'), @('msi', 'msi', 'x64.msi'))) {
            $installer = Get-ChildItem "src-tauri\target\release\bundle\$($spec[0])" -Filter "*$version*.$($spec[1])" |
                Sort-Object LastWriteTime -Descending | Select-Object -First 1
            if (-not $installer) { throw "Installer not found: $($spec[0])" }
            Copy-Item $installer.FullName (Join-Path $dist "dayplanner-pc-$version-$($spec[2])") -Force
        }
    }
    Write-Host "Windows $version -> $dist"
} finally {
    Pop-Location
}
