param([switch]$Production)
$ErrorActionPreference = 'Stop'
if ($Production -and $env:PERRY_WORKSPACE_ROOT -and
    -not (Test-Path -LiteralPath (Join-Path $env:PERRY_WORKSPACE_ROOT 'Cargo.toml'))) {
    throw 'Production build requires optimized runtime libraries.'
}
$projectRoot = Split-Path $PSScriptRoot -Parent
$stagingDirectory = $null
$previousLinkArguments = $env:PERRY_EXTRA_LINK_ARGS
$environmentNames = @(
    'CARGO_TARGET_DIR', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_PANIC',
    'CARGO_PROFILE_RELEASE_DEBUG', 'CARGO_PROFILE_RELEASE_STRIP', 'CARGO_PROFILE_RELEASE_LTO',
    'CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS', 'PERRY_CACHE_DIR'
)
$previousEnvironment = @{}
foreach ($name in $environmentNames) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
. "$PSScriptRoot/build-artifacts.ps1"
Push-Location $projectRoot
try {
    & "$PSScriptRoot/build-native-libs.ps1" -Production:$Production
    & npm.cmd run build
    if ($LASTEXITCODE -ne 0) { throw "React build failed with exit code $LASTEXITCODE." }
    & node.exe scripts/build-workers.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Scan worker generation failed.' }
    $configuration = if ($Production) { 'production' } else { 'testing' }
    $distRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'dist'))
    New-Item -ItemType Directory -Path $distRoot -Force | Out-Null
    $runId = [Guid]::NewGuid().ToString('N')
    $stagingDirectory = Join-Path $distRoot ".$configuration-build-$runId"
    New-Item -ItemType Directory -Path $stagingDirectory | Out-Null
    $buildPath = Join-Path $stagingDirectory 'drip.exe'
    $arguments = @('compile', 'src/desktop/main.ts', '-o', $buildPath, '--target', 'windows', '--march', 'generic', '--embed', 'dist/desktop/index.html')
    $env:PERRY_SIZE_OPT = $null
    $env:PERRY_LL_SIZE_OPT = '0'
    # The icon crate keeps its own optimization and panic policies.
    foreach ($name in $environmentNames) {
        if ($name -ne 'PERRY_CACHE_DIR') { Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue }
    }
    if ($Production) { $env:PERRY_CACHE_DIR = Join-Path $projectRoot '.perry/cache/production-deps-s-sqlite3' }
    if (-not $env:PERRY_LLVM_LIB -and (Test-Path -LiteralPath 'C:\Program Files\LLVM\bin\llvm-lib.exe')) {
        $env:PERRY_LLVM_LIB = 'C:\Program Files\LLVM\bin\llvm-lib.exe'
    }
    $arguments += '--debug-symbols'
    $env:PERRY_EXTRA_LINK_ARGS = "$previousLinkArguments /PDBALTPATH:drip.pdb".Trim()
    if ($Production) { $env:PERRY_EXTRA_LINK_ARGS += ' /OPT:NOICF' }
    & ./.perry/perry.exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "Perry compilation failed with exit code $LASTEXITCODE." }
    $symbolPath = Join-Path $stagingDirectory 'drip.pdb'
    if (-not [IO.File]::Exists($symbolPath)) { throw 'Perry compilation did not produce drip.pdb.' }
    $gitHead = & git.exe rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine the application Git revision.' }
    $gitChanges = & git.exe status --porcelain
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine the application working tree state.' }
    $perryVersion = & ./.perry/perry.exe --version
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine the Perry compiler version.' }
    Push-Location (Join-Path $projectRoot '.perry/source')
    try {
        $nativeRustcVersion = & rustc.exe --version
        if ($LASTEXITCODE -ne 0) { throw 'Could not determine the native Rust compiler version.' }
        $nativeCargoVersion = & cargo.exe --version
        if ($LASTEXITCODE -ne 0) { throw 'Could not determine the native Cargo version.' }
    } finally { Pop-Location }
    $nativeLibraryHashes = [ordered]@{}
    foreach ($library in @('perry_runtime.lib', 'perry_stdlib.lib', 'perry_ui_windows.lib', 'libperry_ui_windows.rlib')) {
        $nativeLibraryHashes[$library] = (Get-FileHash -LiteralPath (Join-Path $env:PERRY_RUNTIME_DIR $library) -Algorithm SHA256).Hash
    }
    $buildInfo = [ordered]@{
        configuration = $configuration
        buildUtc = [DateTime]::UtcNow.ToString('o')
        appVersion = (Get-Content -LiteralPath 'package.json' -Raw | ConvertFrom-Json).version
        gitHead = $gitHead.Trim()
        gitDirty = [bool]$gitChanges
        perryVersion = $perryVersion.Trim() -replace '^perry\s+', ''
        perryRevision = $env:PERRY_BUILD_COMMIT
        perryCompilerSha256 = (Get-FileHash -LiteralPath './.perry/perry.exe' -Algorithm SHA256).Hash
        nativeToolchain = @{ rustc = $nativeRustcVersion.Trim(); cargo = $nativeCargoVersion.Trim() }
        nativeLibrarySha256 = $nativeLibraryHashes
        executableSha256 = (Get-FileHash -LiteralPath $buildPath -Algorithm SHA256).Hash
        pdbSha256 = (Get-FileHash -LiteralPath $symbolPath -Algorithm SHA256).Hash
        executableBytes = (Get-Item -LiteralPath $buildPath).Length
        optimization = [ordered]@{
            application = 'o3'
            native = if ($Production) { 'deps-s-sqlite3' } else { 'current' }
            libraries = $env:PERRY_RUNTIME_DIR
            identicalCodeFolding = if ($Production) { 'disabled' } else { 'compiler-default' }
        }
    }
    $buildInfoPath = Join-Path $stagingDirectory 'build-info.json'
    [IO.File]::WriteAllText($buildInfoPath, ($buildInfo | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    $outputDirectory = Join-Path $distRoot $configuration
    Publish-BuildArtifacts -StagingDirectory $stagingDirectory -OutputDirectory $outputDirectory -RequireSymbols
} finally {
    $env:PERRY_EXTRA_LINK_ARGS = $previousLinkArguments
    foreach ($name in $environmentNames) {
        if ($null -eq $previousEnvironment[$name]) {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        } else {
            [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
        }
    }
    try {
        if ($stagingDirectory) {
            Remove-BuildStagingDirectory -Path $stagingDirectory -ParentDirectory $distRoot -RunId $runId
        }
    } finally {
        Pop-Location
    }
}
