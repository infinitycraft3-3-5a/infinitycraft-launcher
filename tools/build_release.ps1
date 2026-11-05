# build_release.ps1
# Package a 3.3.5a / WotLK WoW client for the InfinityCraft launcher distribution.
#
# Produces, in <OutDir>:
#   assets\
#     skeleton.7z.001           <- non-MPQ client data, 7z multi-volume, byte-split
#     skeleton.7z.002           <-   (each part <= SplitSize, default 1.9e9)
#     <basename>.001            <- each big MPQ, byte-split into parts
#     <basename>.002            <-   (parts are plain byte chunks of the MPQ)
#   version.json                <- manifest matching launcher's core/version.rs
#   upload.ps1                  <- helper that uploads every asset via gh CLI
#
# Run:
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\build_release.ps1
#
# Prereqs:
#   7-Zip installed          (winget install -e --id 7zip.7zip)
#   gh CLI for upload step   (winget install -e --id Github.GitHubCLI)

[CmdletBinding()]
param(
    [string]$Source = "C:\Users\larit\Documents\WOW\InfinityCraft_3.3.5a-base",
    [string]$OutDir = (Join-Path (Split-Path (Split-Path $PSCommandPath -Parent) -Parent) "out"),
    [string]$Tag    = "v1.0.0",
    [int]$Build     = 0,
    [int]$MinClientBuild = 0,
    [long]$SplitSize = 1900000000,
    [string]$Owner  = "infinitycraft3-3-5a",
    [string]$Repo   = "infinitycraft-client",
    [switch]$KeepWork
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

function Fail([string]$m) { Write-Host "`n[FAIL] $m" -ForegroundColor Red; exit 1 }
function Info([string]$m) { Write-Host "[i] $m" -ForegroundColor Cyan }
function Ok([string]$m)   { Write-Host "[+] $m" -ForegroundColor Green }

# ── Locate 7-Zip ─────────────────────────────────────────────────────────────
$sevenZip = @(
    "$env:ProgramFiles\7-Zip\7z.exe",
    "$env:ProgramFiles(x86)\7-Zip\7z.exe",
    "${env:LOCALAPPDATA}\Programs\7-Zip\7z.exe",
    "${env:LOCALAPPDATA}\7-Zip\7z.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $sevenZip) {
    $cmd = Get-Command 7z.exe -ErrorAction SilentlyContinue
    if ($cmd) { $sevenZip = $cmd.Source }
}
if (-not $sevenZip) { Fail "7-Zip not found. Install:  winget install -e --id 7zip.7zip" }
Ok "7-Zip: $sevenZip"

# ── Sanity ───────────────────────────────────────────────────────────────────
if (-not (Test-Path -LiteralPath $Source)) { Fail "Source not found: $Source" }
if (-not (Test-Path -LiteralPath (Join-Path $Source "wow.exe"))) {
    Write-Host "WARN: wow.exe not found in $Source - the skeleton will contain only non-MPQ files." -ForegroundColor Yellow
}
if ($SplitSize -le 0)                { Fail "SplitSize must be > 0." }
if ($SplitSize -gt 2147400000)       { Fail "SplitSize must be < 2,147,400,000 to fit GitHub Releases (2 GiB cap)." }
if ($MinClientBuild -eq 0)           { $MinClientBuild = $Build }
if ($Build -eq 0)                    {
    # auto-derive: if the client has a Patch-N.MPQ (N numeric), use N+1.
    # Otherwise default to 746 (vanilla 3.3.5a build).
    $big = 0
    $dataDir = Join-Path $Source "Data"
    if (Test-Path -LiteralPath $dataDir) {
        Get-ChildItem -LiteralPath $dataDir -File -Filter *.MPQ -ErrorAction SilentlyContinue |
            ForEach-Object { if ($_.Name -imatch 'patch-(\d+)\.mpq$') { $b = [int]$Matches[1]; if ($b -gt $big) { $big = $b } } }
    }
    if ($big -gt 0) { $Build = $big + 1; Info "Auto-derived skeleton build: $Build (= max Patch-N.MPQ build $big + 1)" }
    else            { $Build = 746;     Info "No Patch-N.MPQ detected. Using default Build=746 (override with -Build)." }
}

# ── Prep out dir ─────────────────────────────────────────────────────────────
$assetsDir = Join-Path $OutDir "assets"
$workDir   = Join-Path $OutDir ".work"
if (Test-Path -LiteralPath $OutDir)     { Remove-Item -LiteralPath $OutDir -Recurse -Force }
New-Item -ItemType Directory -Path $assetsDir -Force | Out-Null
New-Item -ItemType Directory -Path $workDir   -Force | Out-Null
$stage   = Join-Path $workDir "stage"
$volsDir = Join-Path $workDir "vols"
New-Item -ItemType Directory -Path $stage   -Force | Out-Null
New-Item -ItemType Directory -Path $volsDir -Force | Out-Null

# ── SHA-256 (GB-safe, streaming) ─────────────────────────────────────────────
function Get-FileSha256([string]$path) {
    $sp = [System.Security.Cryptography.SHA256]::Create()
    $fs = [System.IO.File]::OpenRead($path)
    try {
        $final = [System.Byte[]]$sp.ComputeHash($fs)
        return (([System.BitConverter]::ToString($final) -replace "-","").ToLowerInvariant())
    } finally { $fs.Dispose() }
}

# ── Discover MPQs ────────────────────────────────────────────────────────────
# Note: scan the SOURCE tree, EXCLUDING the launcher's .dl working dir
# (partial downloads / old skeleton volumes are not part of the client).
$srcAbs = (Resolve-Path -LiteralPath $Source).Path.TrimEnd('\')
$dlDirName = ".dl"
$mpqFiles = Get-ChildItem -LiteralPath $Source -Recurse -File |
    Where-Object {
        $_.Extension -ieq '.mpq' -and
        -not ($_.FullName.IndexOf("$srcAbs\$dlDirName\") -ge 0)
    } |
    Sort-Object FullName

if ($mpqFiles.Count -eq 0) { Fail "No .MPQ files found under $Source." }
Write-Host ("Found {0} MPQ files in {1}" -f $mpqFiles.Count, $Source) -ForegroundColor DarkGray

# ── Split MPQs into byte chunks ──────────────────────────────────────────────
# Naming: <basename>.001, <basename>.002, ...   (3-digit zero pad, same as 7z)
# The launcher's recombine::concat_parts() reads parts in order — the 7z-style
# suffix is just a convention here, the launcher only relies on the ORDER in
# version.json's `parts` array.
$patches = New-Object System.Collections.ArrayList
foreach ($mpq in $mpqFiles) {
    $size = $mpq.Length
    $rel  = $mpq.FullName.Substring($srcAbs.Length + 1).Replace('\','/')
    $base = [System.IO.Path]::GetFileNameWithoutExtension($mpq.Name)
    $finalSha = (Get-FileSha256 $mpq.FullName)

    $parts = New-Object System.Collections.ArrayList
    $reader = [System.IO.File]::OpenRead($mpq.FullName)
    try {
        $seq = 0
        while ($true) {
            $seq = $seq + 1
            $partName = "{0}.{1:D3}" -f $base, $seq
            $partPath = Join-Path $workDir $partName
            $written  = 0L
            $pw = [System.IO.File]::Create($partPath)
            try {
                $buf = New-Object byte[] 16MB
                while ($written -lt [long]$SplitSize) {
                    $want = [long]([Math]::Min([int]$buf.Length, ($SplitSize - $written)))
                    $got  = $reader.Read($buf, 0, [int]$want)
                    if ($got -le 0) { break }
                    $pw.Write($buf, 0, $got)
                    $written = $written + $got
                    if ($got -lt $want) { break }
                }
            } finally { $pw.Dispose() }
            if ($written -le 0) { Remove-Item -LiteralPath $partPath -Force; break }
            $parts.Add([pscustomobject]@{
                file   = $partName
                size   = [long]$written
                sha256 = (Get-FileSha256 $partPath)
            }) | Out-Null
        }
    } finally { $reader.Dispose() }

    $patches.Add([pscustomobject]@{
        file    = $mpq.Name
        target  = $rel
        sha256  = $finalSha
        parts   = $parts
    }) | Out-Null
    Write-Host ("  MPQ  {0,-32} {1,12:N0} B  -> {2} part(s, last={3})" -f $mpq.Name, $size, $parts.Count, $parts[-1].file) -ForegroundColor DarkGray
}
Ok ("MPQ patches generated: {0}" -f $patches.Count)

# ── Stage the skeleton (everything except .MPQ) ──────────────────────────────
# Use PowerShell Copy-Item with -Exclude '*.MPQ'. Robust, no external deps.
$allFiles = Get-ChildItem -LiteralPath $Source -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Name -inotlike '*.MPQ' -and
        -not ($_.FullName.IndexOf("$srcAbs\$dlDirName\") -ge 0)
    }
$copied = 0
foreach ($f in $allFiles) {
    $rel  = $f.FullName.Substring($srcAbs.Length + 1)
    $dest = Join-Path $stage $rel
    if (-not (Test-Path -LiteralPath (Split-Path $dest -Parent))) {
        New-Item -ItemType Directory -Path (Split-Path $dest -Parent) -Force | Out-Null
    }
    Copy-Item -LiteralPath $f.FullName -Destination $dest
    $copied++
}
$stagedBytes = (Get-ChildItem -LiteralPath $stage -Recurse -File | Measure-Object -Property Length -Sum).Sum
Ok ("Skeleton staged: {0} file(s), {1:N0} B of non-MPQ content." -f $copied, $stagedBytes)

# ── Skeleton file manifest (name + size) for the launcher's repair logic ─────
# If ANY of these files is missing or truncated on disk, the launcher
# re-downloads the whole 7z. Computed from the staged tree (relative paths,
# forward slashes — matches the 7z internals and core/version.rs).
$skeletonFiles = @(
    Get-ChildItem -LiteralPath $stage -Recurse -File |
        Sort-Object FullName |
        ForEach-Object {
            [pscustomobject]@{
                name = ($_.FullName.Substring($stage.Length + 1)).Replace('\','/')
                size = [long]$_.Length
            }
        }
)
Ok ("Skeleton file manifest: {0} entries." -f $skeletonFiles.Count)

# ── 7z multi-volume archive (relative paths — important!) ────────────────────
# Push-Location into $stage and archive '.' so 7z stores RELATIVE paths inside.
# Extracting from the first volume gives files at the out-dir root.
$volsOut = Join-Path $volsDir "skeleton.7z"
Push-Location $stage
try {
    & $sevenZip a -t7z -mx=1 -spf "$volsOut" "-v$SplitSize" . 2>&1 | Select-Object -Last 6 |
        ForEach-Object { Write-Host "    " $_ }
    if ($LASTEXITCODE -ne 0) { Fail "7z archive creation failed (exit $LASTEXITCODE)." }
} finally { Pop-Location }

$skeletonParts = Get-ChildItem -LiteralPath $volsDir -File |
    Where-Object { $_.Name -like "skeleton.7z.*" } |
    Sort-Object Name
if ($skeletonParts.Count -lt 1) { Fail "No skeleton.7z.* volumes produced." }
foreach ($p in $skeletonParts) {
    Write-Host ("  SKEL {0,-22} {1,12:N0} B" -f $p.Name, $p.Length) -ForegroundColor DarkGray
}
$skeletonPartObjs = @()
foreach ($p in $skeletonParts) {
    $skeletonPartObjs += [pscustomobject]@{
        file   = $p.Name
        size   = [long]$p.Length
        sha256 = (Get-FileSha256 $p.FullName)
    }
}

# ── sha256 of the (virtual) single archive — informational ───────────────────
# The launcher records vj.skeleton.sha256 as the skeleton identity. We compute
# the sha256 of the concatenation of all volumes (what a single-volume archive
# would be, byte-wise).
$concatPath = Join-Path $workDir "skeleton.7z.concat"
$w = [System.IO.File]::Create($concatPath)
try {
    foreach ($p in $skeletonPartObjs | Select-Object file) {
        $full = Join-Path $volsDir $p.file
        $r = [System.IO.File]::OpenRead($full)
        try { $r.CopyTo($w) } finally { $r.Dispose() }
    }
    $w.Flush()
} finally { $w.Dispose() }
$skeletonSha256 = Get-FileSha256 $concatPath
Remove-Item -LiteralPath $concatPath -Force -ErrorAction SilentlyContinue

# ── Copy final assets flat into <OutDir>/assets ──────────────────────────────
foreach ($p in $skeletonPartObjs) {
    Copy-Item -LiteralPath (Join-Path $volsDir $p.file) -Destination (Join-Path $assetsDir $p.file)
}
foreach ($patch in $patches) {
    foreach ($p in $patch.parts) {
        $srcPath = Join-Path $workDir $p.file
        if (-not (Test-Path -LiteralPath $srcPath)) { Fail "missing part file $srcPath" }
        Copy-Item -LiteralPath $srcPath -Destination (Join-Path $assetsDir $p.file)
    }
}
Ok ("Copied asset files into {0}" -f $assetsDir)

# ── version.json (exact shape expected by core/version.rs) ────────────────────
$versionJson = [pscustomobject]@{
    release        = $Tag
    latestVersion  = $Build
    minClientBuild = $MinClientBuild
    skeleton       = [pscustomobject]@{
        sha256 = $skeletonSha256
        parts  = @($skeletonPartObjs)
        files  = @($skeletonFiles)
    }
    patches        = @($patches)
}

$versionJsonPath = Join-Path $OutDir "version.json"
$json = $versionJson | ConvertTo-Json -Depth 10 -Compress:$false
[System.IO.File]::WriteAllBytes($versionJsonPath, ([System.Text.Encoding]::UTF8).GetBytes($json))
Ok "version.json -> $versionJsonPath"

# ── Summary ──────────────────────────────────────────────────────────────────
$allAssets = Get-ChildItem -LiteralPath $assetsDir -File
$sumBytes  = ($allAssets | Measure-Object -Property Length -Sum).Sum
$largest   = ($allAssets | Measure-Object -Property Length -Max).Maximum
Write-Host ""
Write-Host ("═" * 56) -ForegroundColor Cyan
Write-Host ("  Tag             : {0}" -f $Tag)
Write-Host ("  Build (skeleton): {0}" -f $Build)
Write-Host ("  Min client      : {0}" -f $MinClientBuild)
Write-Host ("  Skeleton parts  : {0}" -f $skeletonPartObjs.Count)
Write-Host ("  MPQ patches     : {0}" -f $patches.Count)
Write-Host ("  Total assets    : {0}" -f $allAssets.Count)
Write-Host ("  Total size      : {0:0.##} GB" -f ([double]$sumBytes / 1GB))
Write-Host ("  Largest part    : {0:0.##} GB / 2 GiB limit" -f ([double]$largest / 1GB))
Write-Host ("  Assets dir      : {0}" -f $assetsDir)
Write-Host ("═" * 56) -ForegroundColor Cyan

# ── Upload helper (uses gh CLI) ──────────────────────────────────────────────
$uploadScript = Join-Path $OutDir "upload.ps1"
$uploadBody = @'
$ErrorActionPreference = "Stop"
$owner = "infinitycraft3-3-5a"
$repo  = "infinitycraft-client"
$tag   = "TAG_PLACEHOLDER"
$assetDir = "ASSETS_DIR_PLACEHOLDER"
$versionJson = "VERSION_JSON_PLACEHOLDER"

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Write-Host "ERROR: 'gh' CLI not found. Install:  winget install -e --id Github.GitHubCLI"
    exit 1
}

# --- Load GH_TOKEN from .env if not already set (env var wins) ---
if (-not $env:GH_TOKEN) {
    $root = Split-Path -Parent $PSScriptRoot
    foreach ($cand in @((Join-Path $root ".env"), (Join-Path $PSScriptRoot ".env"))) {
        if (Test-Path -LiteralPath $cand) {
            Get-Content -LiteralPath $cand | ForEach-Object {
                $line = $_.Trim()
                if ($line -and $line -notlike "#*" -and $line -match '^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$') {
                    $k = $Matches[1]; $v = $Matches[2].Trim().Trim('"').Trim("'")
                    if (-not (Get-Item "Env:$k" -ErrorAction SilentlyContinue)) { Set-Item "Env:$k" -Value $v -Force }
                }
            }
            break
        }
    }
}
if (-not (gh auth status 2>$null) -and -not $env:GH_TOKEN) {
    Write-Host "No GH_TOKEN: put it in `.env` (see .env.example) or run `gh auth login`."
}

# Create release (idempotent)
Write-Host "Ensuring release `$tag exists..."
$existing = gh api repos/$owner/$repo/releases/tags/$tag 2>$null
if (-not $LASTEXITCODE) {
    $out = gh api -X POST repos/$owner/$repo/releases -f "tag_name=$tag" -f "name=$tag" -f "draft=false" -f "generate_release_notes=true"
} else {
    Write-Host "Release already exists."
}

# Commit version.json to the repo root (raw.githubusercontent.com default URL)
$parentRepo = Split-Path -Parent $assetDir
# (user is responsible for git add/commit/push of version.json; this is the
#  part that needs the real repo, not the release.)
Write-Host "  1. Run `git add $versionJson; git commit -m "$tag"; git push` in the repo"
Write-Host "  2. (Optional) upload these files to the release via the GitHub web UI, OR:"

# Upload all assets
$files = Get-ChildItem -LiteralPath $assetDir -File
$i = 0
foreach ($f in $files) {
    $i++
    $sizeGB = [Math]::Round($f.Length / 1GB, 2)
    Write-Host ("[{0}/{1}] uploading {2}  ({3} GB)" -f $i, $files.Count, $f.Name, $sizeGB)
    $r = gh release upload $tag $f.FullName --clobber --repo "$owner/$repo" 2>&1
    if ($LASTEXITCODE -ne 0) { Write-Host "    UPLOAD FAILED: $r"; exit 1 }
}
Write-Host "All {0} assets uploaded." -f $files.Count
Write-Host "Release: https://github.com/$owner/$repo/releases/tag/$tag"
Write-Host "Launcher will fetch: https://github.com/$owner/$repo/releases/download/$TAG_UPPER/skeleton.7z.001"
'@
$TAG_UPPER = $Tag.TrimStart('v').ToUpperInvariant()
$uploadBody = $uploadBody.Replace("TAG_PLACEHOLDER", $Tag)
$uploadBody = $uploadBody.Replace("ASSETS_DIR_PLACEHOLDER", $assetsDir)
$uploadBody = $uploadBody.Replace("VERSION_JSON_PLACEHOLDER", $versionJsonPath)
$uploadBody = $uploadBody.Replace("$TAG_UPPER", $TAG_UPPER)
[System.IO.File]::WriteAllBytes($uploadScript, ([System.Text.Encoding]::UTF8).GetBytes($uploadBody))
Ok "upload helper -> $uploadScript"

# ── Clean up work dirs unless -KeepWork ──────────────────────────────────────
if (-not $KeepWork) {
    Remove-Item -LiteralPath $workDir -Recurse -Force
    Ok "Removed work dirs (use -KeepWork to keep for inspection)."
}

Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Review version.json:"
Write-Host "       Get-Content -LiteralPath $versionJsonPath -TotalCount 20"
Write-Host "  2. Create the GitHub private repo:  infinitycraft3-3-5a/infinitycraft-client"
Write-Host "  3. cd the repo, add version.json, commit and push."
Write-Host "  4. Install gh CLI (if not already):  winget install -e --id Github.GitHubCLI"
Write-Host "  5. Run:  powershell -NoProfile -ExecutionPolicy Bypass -File $uploadScript"
Write-Host "  6. Open the launcher -> settings -> owner/repo -> install."
