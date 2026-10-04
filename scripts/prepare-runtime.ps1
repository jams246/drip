$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$runtimeSource = Join-Path $projectRoot '.perry/source'
if (-not (Test-Path -LiteralPath (Join-Path $runtimeSource 'Cargo.toml'))) {
    & git -c advice.detachedHead=false clone --depth 1 --branch v0.5.1520 https://github.com/PerryTS/perry.git $runtimeSource
    if ($LASTEXITCODE -ne 0) { throw 'Could not fetch the pinned Perry runtime source.' }
}
$revision = & git -C $runtimeSource rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne '381045a8735ff325621c5dfb26a3bd4f5a8798c5') {
    throw 'Runtime patches require Perry v0.5.1520 at the pinned revision.'
}

foreach ($patchName in @('worker-agent', 'webview-callback', 'native-handle', 'sqlite-statement-cleanup', 'sync-fetch', 'fetch-promise', 'json-closure', 'crash-handler')) {
    $runtimePatch = Join-Path $PSScriptRoot "runtime/$patchName.patch"
    & git -C $runtimeSource apply --check $runtimePatch 2>$null
    if ($LASTEXITCODE -eq 0) {
        & git -C $runtimeSource apply $runtimePatch
        if ($LASTEXITCODE -ne 0) { throw "Could not apply $patchName runtime patch." }
    } else {
        & git -C $runtimeSource apply --reverse --check $runtimePatch 2>$null
        if ($LASTEXITCODE -ne 0) { throw "Pinned $patchName runtime patch no longer matches source." }
    }
}

$direntPath = Join-Path $runtimeSource 'crates/perry-runtime/src/fs/dirent.rs'
$source = [IO.File]::ReadAllText($direntPath).Replace("`r`n", "`n")

function Replace-RuntimeBlock([string]$Before, [string]$After) {
    if ($script:source.Contains($Before)) {
        $script:source = $script:source.Replace($Before, $After)
        return
    }
    if (-not $script:source.Contains($After)) { throw 'Pinned filesystem patch no longer matches runtime source.' }
}

Replace-RuntimeBlock @'
                    for e in entries.flatten() {
                        if let Some(name) = e.file_name().to_str() {
                            if let Ok(ft) = e.file_type() {
                                items.push((name.to_string(), ft));
                            }
                        }
                    }
'@ @'
                    for entry in entries {
                        let e = match entry {
                            Ok(e) => e,
                            Err(err) => crate::exception::js_throw(build_fs_error_value(&err, "scandir", &path_str)),
                        };
                        if let Some(name) = e.file_name().to_str() {
                            let ft = match e.file_type() {
                                Ok(ft) => ft,
                                Err(err) => crate::exception::js_throw(build_fs_error_value(&err, "scandir", &path_str)),
                            };
                            items.push((name.to_string(), ft));
                        }
                    }
'@

Replace-RuntimeBlock @'
                    for e in entries.flatten() {
                        if let Some(name) = e.file_name().to_str() {
                            names.push(name.to_string());
                        }
                    }
'@ @'
                    for entry in entries {
                        let e = match entry {
                            Ok(e) => e,
                            Err(err) => crate::exception::js_throw(build_fs_error_value(&err, "scandir", &path_str)),
                        };
                        if let Some(name) = e.file_name().to_str() {
                            names.push(name.to_string());
                        }
                    }
'@

Replace-RuntimeBlock @'
            Err(_) => {
                let arr = js_array_alloc(0);
                f64::from_bits(i64::cast_unsigned(arr as i64))
            }
'@ @'
            Err(err) => crate::exception::js_throw(build_fs_error_value(&err, "scandir", &path_str)),
'@

$source = $source.Replace('/// Returns an empty array on error.', '/// Throws filesystem errors for the non-recursive scan paths.')
if (-not $source.Contains('// DRIP filesystem error patch preserves the Perry v0.5.1520 ABI.')) {
    $source = $source.Replace('/// Throws filesystem errors for the non-recursive scan paths.', "// DRIP filesystem error patch preserves the Perry v0.5.1520 ABI.`n/// Throws filesystem errors for the non-recursive scan paths.")
}
if ($source -ne [IO.File]::ReadAllText($direntPath).Replace("`r`n", "`n")) {
    [IO.File]::WriteAllText($direntPath, $source, [Text.UTF8Encoding]::new($false))
}
$env:PERRY_RUNTIME_DIR = Join-Path $projectRoot '.perry'
$env:PERRY_WORKSPACE_ROOT = $runtimeSource
$env:PERRY_NO_AUTO_OPTIMIZE = $null
# Filesystem errors and worker ownership preserve the pinned compiler/runtime ABI.
$env:PERRY_BUILD_COMMIT = '381045a8735ff325621c5dfb26a3bd4f5a8798c5'
# Perry v0.5.1520 RS4GC cannot lower Windows exception pads; use its shadow-stack GC.
$env:PERRY_RS4GC = '0'
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
