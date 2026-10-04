$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
& "$PSScriptRoot/prepare-runtime.ps1"
$runtimeSource = Join-Path $projectRoot '.perry/source'
$targetDirectory = 'target/drip-coherent-ui-nolto'
$features = @(
    'perry-runtime/full',
    'perry-runtime/stdlib',
    'perry-runtime/keepalive-anchors',
    'perry-runtime/alloc-mimalloc',
    'perry-runtime/global-json',
    'perry-runtime/global-math',
    'perry-runtime/global-text',
    'perry-runtime/regex-engine',
    'perry-runtime/global-webcrypto',
    'perry-runtime/global-webfetch',
    'perry-runtime/external-fetch-symbols',
    'perry-runtime/global-url',
    'perry-runtime/url-engine',
    'perry-stdlib/async-runtime',
    'perry-stdlib/web-fetch',
    'perry-stdlib/database-sqlite',
    'perry-ui-windows/ffi-exports'
) -join ','
# UI and worker bindings must share one Rust dependency graph and runtime state.
# Mixing the downloaded UI archive with a patched runtime splits native getters.
$env:CARGO_TARGET_DIR = $targetDirectory
$env:CARGO_PROFILE_RELEASE_PANIC = 'unwind'
# Thin LTO promotes private runtime slots differently inside each static archive.
# Preserve shared dependency objects so UI trimming can use one registry copy.
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:RUSTFLAGS = '-C force-unwind-tables=yes'
$buildLog = Join-Path $projectRoot 'dist/native-libs.log'
New-Item -ItemType Directory -Path (Split-Path $buildLog -Parent) -Force | Out-Null
Write-Output 'Building the coherent Perry source libraries.'
Push-Location $runtimeSource
try {
    & cargo.exe build --locked --release --target x86_64-pc-windows-msvc -p perry-runtime-static -p perry-stdlib-static -p perry-ui-windows --no-default-features --features $features *> $buildLog
    if ($LASTEXITCODE -ne 0) {
        Get-Content -LiteralPath $buildLog -Tail 100 | Out-Host
        throw 'Coherent Perry source library build failed.'
    }
} finally {
    Pop-Location
    $env:CARGO_TARGET_DIR = $null
    $env:CARGO_PROFILE_RELEASE_PANIC = $null
    $env:CARGO_PROFILE_RELEASE_LTO = $null
    $env:RUSTFLAGS = $null
}
$libraryDirectory = Join-Path $runtimeSource "$targetDirectory/x86_64-pc-windows-msvc/release"
foreach ($library in @('perry_runtime.lib', 'perry_stdlib.lib', 'perry_ui_windows.lib', 'libperry_ui_windows.rlib')) {
    if (-not (Test-Path -LiteralPath (Join-Path $libraryDirectory $library))) {
        throw "Source build did not produce $library."
    }
}
$env:PERRY_RUNTIME_DIR = $libraryDirectory
# Disable automatic partial rebuilds only after the complete source trio exists.
$env:PERRY_NO_AUTO_OPTIMIZE = '1'
$env:PERRY_SIZE_OPT = $null
$env:PERRY_LL_SIZE_OPT = '0'
Write-Output "Using coherent source libraries: $libraryDirectory"
