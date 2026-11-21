#requires -Version 7
<#
.SYNOPSIS
  Builds speech.cpp backend variants into app/src-tauri/backends/<variant>/.
.EXAMPLE
  pwsh scripts/build-backends.ps1 -Variants vulkan
  pwsh scripts/build-backends.ps1 -Variants vulkan,cuda -Clean
#>
param(
  [string[]]$Variants = @('vulkan'),
  [string]$Config = 'Release',
  [switch]$Clean
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Assert-Native([string]$What) {
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# Validate variants and the CUDA toolkit up front, before any long build starts.
foreach ($v in $Variants) {
  if ($v -notin @('vulkan', 'cuda', 'cpu')) {
    throw "Unknown variant '$v'. Supported on Windows: vulkan, cuda, cpu."
  }
}
if ('cuda' -in $Variants) {
  if (-not $env:CUDA_PATH) {
    throw "CUDA_PATH is not set. Install the CUDA 12 toolkit (nvcc, cudart, cublas) to build the 'cuda' variant."
  }
  if (-not (Test-Path $env:CUDA_PATH)) {
    throw "CUDA_PATH points to a missing folder: $env:CUDA_PATH"
  }
}

git submodule update --init --recursive extern/speech.cpp
Assert-Native 'git submodule update'

foreach ($v in $Variants) {
  Write-Host "=== Backend variant: $v ($Config) ===" -ForegroundColor Cyan
  $buildDir = "build/speech-$v"
  if ($Clean -and (Test-Path $buildDir)) {
    Remove-Item -Recurse -Force $buildDir
  }

  $cfg = @(
    '-S', 'extern/speech.cpp', '-B', $buildDir, '-A', 'x64',
    '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded',
    '-DGGML_NATIVE=OFF', '-DGGML_OPENMP=OFF', '-DBUILD_SHARED_LIBS=OFF',
    '-DGGML_BUILD_TESTS=OFF', '-DGGML_BUILD_EXAMPLES=OFF'
  )
  switch ($v) {
    'vulkan' { $cfg += '-DGGML_VULKAN=ON' }
    'cuda'   { $cfg += '-DGGML_CUDA=ON'; $cfg += '-DCMAKE_CUDA_ARCHITECTURES=75;80;86;89' }
    'cpu'    { }
  }
  cmake @cfg
  Assert-Native "cmake configure ($v)"

  cmake --build $buildDir --config $Config --target speech-cli -j
  Assert-Native "cmake build ($v)"

  $src = Join-Path $buildDir "$Config/speech.exe"
  if (-not (Test-Path $src)) { throw "Build output not found: $src" }
  $dest = "app/src-tauri/backends/$v"
  New-Item -ItemType Directory -Force $dest | Out-Null
  Copy-Item $src $dest -Force

  if ($v -eq 'cuda') {
    # CUDA 12 keeps DLLs in bin; CUDA 13 moved them to bin\x64.
    $dirs = @("$env:CUDA_PATH\bin", "$env:CUDA_PATH\bin\x64") | Where-Object { Test-Path $_ }
    foreach ($pattern in 'cudart64_*.dll', 'cublas64_*.dll', 'cublasLt64_*.dll') {
      $found = $dirs | ForEach-Object { Get-ChildItem -Path $_ -Filter $pattern -ErrorAction SilentlyContinue } | Select-Object -First 1
      if (-not $found) { throw "$pattern not found in $env:CUDA_PATH\bin" }
      Copy-Item $found.FullName $dest -Force
    }
  }

  $exe = Join-Path $dest 'speech.exe'
  Write-Host "--- $exe --version" -ForegroundColor Cyan
  & $exe --version
  Assert-Native "speech --version ($v)"
  Write-Host "--- $exe devices --json" -ForegroundColor Cyan
  & $exe devices --json
  Assert-Native "speech devices ($v)"
}
