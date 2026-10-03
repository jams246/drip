function Publish-BuildArtifacts {
    param(
        [Parameter(Mandatory)][string]$StagingDirectory,
        [Parameter(Mandatory)][string]$OutputDirectory
    )
    $stagingRoot = [IO.Path]::GetFullPath($StagingDirectory)
    $outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
    if ($stagingRoot -eq $outputRoot) { throw 'Build staging must differ from the output directory.' }
    if (-not [IO.File]::Exists((Join-Path $stagingRoot 'drip.exe'))) {
        throw 'Perry compilation did not produce drip.exe.'
    }
    New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
    $promoted = [Collections.Generic.List[object]]::new()
    try {
        foreach ($name in @('drip.pdb', 'drip.exe')) {
            $candidate = Join-Path $stagingRoot $name
            if (-not [IO.File]::Exists($candidate)) { continue }
            $destination = Join-Path $outputRoot $name
            $backup = $null
            if ([IO.File]::Exists($destination)) {
                $backup = Join-Path $stagingRoot ($name + '.previous-' + [Guid]::NewGuid().ToString('N'))
                [IO.File]::Replace($candidate, $destination, $backup)
            } else {
                [IO.File]::Move($candidate, $destination)
            }
            $promoted.Add([pscustomobject]@{ destination = $destination; backup = $backup })
        }
    } catch {
        $promotionError = $_
        $rollbackErrors = [Collections.Generic.List[string]]::new()
        for ($index = $promoted.Count - 1; $index -ge 0; $index--) {
            $artifact = $promoted[$index]
            try {
                if ($artifact.backup) {
                    [IO.File]::Replace($artifact.backup, $artifact.destination, [NullString]::Value)
                } else {
                    [IO.File]::Delete($artifact.destination)
                }
            } catch {
                $rollbackErrors.Add($_.Exception.Message)
            }
        }
        if ($rollbackErrors.Count -gt 0) {
            throw "Build promotion failed: $($promotionError.Exception.Message) Rollback failed: $($rollbackErrors -join '; ')"
        }
        throw $promotionError
    }
}

function Remove-BuildStagingDirectory {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$ParentDirectory,
        [Parameter(Mandatory)][Guid]$RunId
    )
    $resolvedPath = [IO.Path]::GetFullPath($Path)
    $resolvedParent = [IO.Path]::GetFullPath($ParentDirectory).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $name = [IO.Path]::GetFileName($resolvedPath)
    $expectedSuffix = '-build-' + $RunId.ToString('N')
    if ([IO.Path]::GetDirectoryName($resolvedPath) -ne $resolvedParent -or
        -not $name.StartsWith('.') -or -not $name.EndsWith($expectedSuffix)) {
        throw 'Build cleanup target is not the owned staging directory.'
    }
    if (-not (Test-Path -LiteralPath $resolvedPath)) { return }
    if ((Get-Item -LiteralPath $resolvedPath -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'Build cleanup target is a reparse point.'
    }
    Remove-Item -LiteralPath $resolvedPath -Recurse -Force
}
