$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$distRoot = Join-Path $projectRoot 'dist'
New-Item -ItemType Directory -Path $distRoot -Force | Out-Null
$archivePath = Join-Path $distRoot 'drip.tar'
$temporaryName = 'dist/.drip-' + [Guid]::NewGuid().ToString('N') + '.tar'
$temporaryPath = Join-Path $projectRoot $temporaryName

try {
    & wsl.exe -d Halio --cd $projectRoot -- docker build --tag drip:latest .
    if ($LASTEXITCODE -ne 0) { throw "Docker build failed with exit code $LASTEXITCODE." }
    & wsl.exe -d Halio --cd $projectRoot -- docker image save --output $temporaryName drip:latest
    if ($LASTEXITCODE -ne 0) { throw "Docker image export failed with exit code $LASTEXITCODE." }
    [IO.File]::Move($temporaryPath, $archivePath, $true)
    Write-Host "Docker image saved to $archivePath"
} finally {
    [IO.File]::Delete($temporaryPath)
}
