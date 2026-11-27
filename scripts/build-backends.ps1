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

# Apply patches/ggml to the pinned ggml submodule, then patches/speech.cpp to speech.cpp itself. The two sets touch
# different repositories, so their order matters for the log only; ggml goes first as the layer speech.cpp builds on.
# A patch already applied is skipped; one that neither applies nor is applied stops the build.
$patchSets = @(
  @{ Name = 'ggml'; Folder = 'patches/ggml'; Repo = 'extern/speech.cpp/ggml' },
  @{ Name = 'speech.cpp'; Folder = 'patches/speech.cpp'; Repo = 'extern/speech.cpp' }
)
foreach ($set in $patchSets) {
  $repoDir = Join-Path $Root $set.Repo
  foreach ($patch in Get-ChildItem (Join-Path $Root $set.Folder) -Filter '*.patch' -ErrorAction SilentlyContinue | Sort-Object Name) {
    git -C $repoDir apply --reverse --check $patch.FullName 2>$null
    if ($LASTEXITCODE -eq 0) {
      Write-Host "$($set.Name) patch already applied: $($patch.Name)"
      continue
    }
    git -C $repoDir apply --check $patch.FullName
    Assert-Native "$($set.Name) patch $($patch.Name) neither applies nor is applied"
    git -C $repoDir apply $patch.FullName
    Assert-Native "git apply $($patch.Name)"
    Write-Host "$($set.Name) patch applied: $($patch.Name)"
  }
}

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

  cmake --build $buildDir --config $Config --target speech-cli worker-battle-check worker-inbox-check -j
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

  if ($v -eq 'cuda') {
    # Diagnostic only: list the DLLs speech.exe imports (never fails the build).
    try {
      $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
      if (Test-Path $vswhere) {
        $vc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        $dumpbin = if ($vc) {
          Get-ChildItem -Path (Join-Path $vc 'VC\Tools\MSVC') -Recurse -Filter dumpbin.exe -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match 'Hostx64\\x64' } | Select-Object -First 1
        }
        if ($dumpbin) {
          Write-Host "--- dumpbin /dependents $exe" -ForegroundColor Cyan
          & $dumpbin.FullName /nologo /dependents $exe
        } else {
          Write-Host "dumpbin not found; skipping dependents listing."
        }
      }
    } catch {
      Write-Host "dumpbin diagnostic skipped: $($_.Exception.Message)"
    }

    $nvcuda = Join-Path $env:SystemRoot 'System32\nvcuda.dll'
    if (-not (Test-Path $nvcuda)) {
      Write-Warning "nvcuda.dll not found ($nvcuda): no NVIDIA driver on this machine. speech.exe imports nvcuda.dll (CUDA driver API), so it cannot start here (STATUS_DLL_NOT_FOUND). Skipping 'speech --version', 'devices --json' and the two checks for the cuda variant; the build output and bundled CUDA DLLs were verified."
      continue
    }
  }

  Write-Host "--- $exe --version" -ForegroundColor Cyan
  & $exe --version
  Assert-Native "speech --version ($v)"
  Write-Host "--- $exe devices --json" -ForegroundColor Cyan
  & $exe devices --json
  Assert-Native "speech devices ($v)"

  # The battle's reading and lockstep, and the inbox's handling of its lines, which need no model (speech.cpp's
  # checks/worker/worker-battle-check.cpp and worker-inbox-check.cpp).
  foreach ($name in 'worker-battle-check', 'worker-inbox-check') {
    $check = Join-Path $buildDir "$Config/$name.exe"
    Write-Host "--- $check" -ForegroundColor Cyan
    & $check
    Assert-Native "$name ($v)"
  }
}
