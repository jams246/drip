param([switch]$Production)
$ErrorActionPreference = 'Stop'
if ($Production -and $env:PERRY_WORKSPACE_ROOT -and
    -not (Test-Path -LiteralPath (Join-Path $env:PERRY_WORKSPACE_ROOT 'Cargo.toml'))) {
    throw 'Production build requires optimized runtime libraries.'
}
$projectRoot = Split-Path $PSScriptRoot -Parent
$stagingDirectory = $null
. "$PSScriptRoot/build-artifacts.ps1"
Push-Location $projectRoot
try {
    & "$PSScriptRoot/build-native-libs.ps1"
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
    if (-not $env:PERRY_LLVM_LIB -and (Test-Path -LiteralPath 'C:\Program Files\LLVM\bin\llvm-lib.exe')) {
        $env:PERRY_LLVM_LIB = 'C:\Program Files\LLVM\bin\llvm-lib.exe'
    }
    if (-not $Production) {
        $arguments += '--debug-symbols'
    }
    & ./.perry/perry.exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "Perry compilation failed with exit code $LASTEXITCODE." }
    $outputDirectory = Join-Path $distRoot $configuration
    Publish-BuildArtifacts -StagingDirectory $stagingDirectory -OutputDirectory $outputDirectory
} finally {
    try {
        if ($stagingDirectory) {
            Remove-BuildStagingDirectory -Path $stagingDirectory -ParentDirectory $distRoot -RunId $runId
        }
    } finally {
        Pop-Location
    }
}
