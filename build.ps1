param([string]$OutputDirectory = 'dist/release')
$ErrorActionPreference = 'Stop'
if (Test-Path "$PSScriptRoot\.tools\cargo\bin\cargo.exe") {
    $env:CARGO_HOME = "$PSScriptRoot\.tools\cargo"
    $env:RUSTUP_HOME = "$PSScriptRoot\.tools\rustup"
    $env:PATH = "$PSScriptRoot\.tools\cargo\bin;$PSScriptRoot\.tools\w64devkit\bin;$env:PATH"
    $env:RUSTFLAGS = '-C link-self-contained=yes'
}
Push-Location $PSScriptRoot
try {
    $metadata = & cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Could not read Cargo package metadata' }
    $version = ($metadata.packages | Where-Object { $_.name -eq 'cftui' } | Select-Object -First 1).version
    if (-not $version) { throw 'Could not find the cftui package version' }
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    $bundleDir = Join-Path $PSScriptRoot $OutputDirectory
    New-Item -ItemType Directory -Force -Path $bundleDir | Out-Null
    Copy-Item -LiteralPath target\release\cftui.exe -Destination (Join-Path $bundleDir 'cftui.exe')
    Copy-Item -LiteralPath cftui.example.toml,README.md,LICENSE,SECURITY.md -Destination $bundleDir
    $docsDir = Join-Path $bundleDir 'docs'
    New-Item -ItemType Directory -Force -Path $docsDir | Out-Null
    Copy-Item -LiteralPath docs\demo.png -Destination (Join-Path $docsDir 'demo.png')
    @'
@echo off
cd /d "%~dp0"
cftui.exe --demo
if errorlevel 1 pause
'@ | Set-Content -LiteralPath (Join-Path $bundleDir 'demo.cmd') -Encoding ascii
    $bundleFiles = @('cftui.exe','cftui.example.toml','README.md','LICENSE','SECURITY.md','demo.cmd','docs') | ForEach-Object { Join-Path $bundleDir $_ }
    $zipName = "cftui-v$version-windows-x64.zip"
    $zipPath = Join-Path $bundleDir $zipName
    Compress-Archive -LiteralPath $bundleFiles -DestinationPath $zipPath -Force
    $hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $zipName" | Set-Content -LiteralPath (Join-Path $bundleDir "$zipName.sha256") -Encoding ascii
    Write-Host "Executable: $(Join-Path $bundleDir 'cftui.exe')"
    Write-Host "Release ZIP: $zipPath"
} finally { Pop-Location }
