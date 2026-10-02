param([switch]$Production)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try {
    $env:PERRY_RUNTIME_DIR = Join-Path $projectRoot '.perry'
    $configuration = if ($Production) { 'production' } else { 'testing' }
    $outputDirectory = Join-Path $projectRoot "dist/$configuration"
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    $buildPath = Join-Path $outputDirectory 'drip.build.exe'
    $arguments = @('compile', 'src/main.ts', '-o', $buildPath, '--target', 'windows', '--march', 'generic')
    if ($Production) {
        if (-not $env:PERRY_WORKSPACE_ROOT) {
            $env:PERRY_WORKSPACE_ROOT = Join-Path $projectRoot '.perry/source'
        }
        if (-not (Test-Path '.perry/source/Cargo.toml')) {
            & git -c advice.detachedHead=false clone --depth 1 --branch v0.5.1520 https://github.com/PerryTS/perry.git .perry/source
            if ($LASTEXITCODE -ne 0) { throw 'Could not fetch the pinned Perry runtime source.' }
        }
        $env:PERRY_SIZE_OPT = 'z'
        $env:PERRY_NO_AUTO_OPTIMIZE = $null
        $env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
    } else {
        $arguments += @('--debug-symbols', '--no-auto-optimize')
    }
    & ./.perry/perry.exe @arguments 2>&1 | Tee-Object -Variable buildOutput | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Perry compilation failed with exit code $LASTEXITCODE." }
    if ($Production -and $buildOutput -match 'workspace source not found|using prebuilt|Skipping auto-optimize') {
        Remove-Item -LiteralPath $buildPath
        throw 'Production build requires optimized runtime libraries.'
    }
    Copy-Item -LiteralPath 'src/index.html' -Destination $outputDirectory -Force
    Move-Item -LiteralPath $buildPath -Destination (Join-Path $outputDirectory 'drip.exe') -Force
} finally {
    Pop-Location
}
