param([Parameter(ValueFromRemainingArguments = $true)][string[]]$AppArgs)
$ErrorActionPreference = 'Stop'
if (Test-Path "$PSScriptRoot\.tools\cargo\bin\cargo.exe") {
    $env:CARGO_HOME = "$PSScriptRoot\.tools\cargo"
    $env:RUSTUP_HOME = "$PSScriptRoot\.tools\rustup"
    $env:PATH = "$PSScriptRoot\.tools\cargo\bin;$PSScriptRoot\.tools\w64devkit\bin;$env:PATH"
    $env:RUSTFLAGS = '-C link-self-contained=yes'
}
Push-Location $PSScriptRoot
try {
    & cargo run -- @AppArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally { Pop-Location }
