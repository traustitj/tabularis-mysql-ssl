$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$releaseBinary = Join-Path $root 'target\release\mysqlssl-plugin.exe'
$distDir = Join-Path $root 'dist\mysqlssl'
$manifestPath = Join-Path $root 'manifest.json'

if (-not (Test-Path $releaseBinary)) {
    throw "Release binary not found at $releaseBinary. Run 'cargo build --release' first."
}

$manifest = Get-Content $manifestPath | ConvertFrom-Json
$zipPath = Join-Path $root ("dist\mysqlssl-v{0}-win-x64.zip" -f $manifest.version)

if (Test-Path $distDir) {
    Remove-Item $distDir -Recurse -Force
}

if (Test-Path $zipPath) {
    Remove-Item $zipPath -Force
}

New-Item -ItemType Directory -Force -Path $distDir | Out-Null
Copy-Item $manifestPath $distDir -Force
Copy-Item $releaseBinary (Join-Path $distDir 'mysqlssl-plugin.exe') -Force
Copy-Item (Join-Path $root 'README.md') $distDir -Force

$configExample = Join-Path $root 'mysqlssl-plugin.config.json.example'
if (Test-Path $configExample) {
    Copy-Item $configExample (Join-Path $distDir 'mysqlssl-plugin.config.json.example') -Force
}

Compress-Archive -Path $distDir -DestinationPath $zipPath -CompressionLevel Optimal

Write-Output "Plugin package ready in $distDir"
Write-Output "Plugin zip ready at $zipPath"