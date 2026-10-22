param([string]$BuildDir = "")
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (!$BuildDir) { $BuildDir = Join-Path $Root "build/native" }
function Run-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed: $LASTEXITCODE" }
}
Run-Checked "cmake" @("-S", $Root, "-B", $BuildDir, "-DBUILD_TESTING=ON")
Run-Checked "cmake" @("--build", $BuildDir, "--config", "Release", "--parallel", "4")
Run-Checked "ctest" @("--test-dir", $BuildDir, "-C", "Release", "--output-on-failure")
Run-Checked (Join-Path $BuildDir "Release/daw_session.exe") @("--render", (Join-Path $BuildDir "native-demo.wav"), "--seconds", "8")
