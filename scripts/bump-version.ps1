# bump-version.ps1 — Read current versions, prompt for new version, update all files, then build.
# Usage (interactive): .\bump-version.ps1
# Usage (CI):          .\bump-version.ps1 -Version 0.3.0 [-Build] [-Release]
#
# -Build    Run 'pnpm tauri build' locally (Windows only) after bumping
# -Release  Commit the version bump, create tag vX.Y.Z, and push — triggers GitHub Actions
#           (builds Windows + macOS + Linux and publishes the GitHub release automatically)
[CmdletBinding()]
param(
    [string] $Version,
    [switch] $Build,
    [switch] $Release
)

$Root        = Split-Path $PSScriptRoot -Parent
$CargoToml   = Join-Path $Root "src-tauri\Cargo.toml"
$TauriConf   = Join-Path $Root "src-tauri\tauri.conf.json"
$PackageJson = Join-Path $Root "package.json"

function Get-Version {
    param([string]$File, [string]$Pattern)
    $line = Select-String -Path $File -Pattern $Pattern | Select-Object -First 1
    if (-not $line) { return "not found" }
    if ($line.Line -match '"version"\s*:\s*"([^"]+)"') { return $Matches[1] }
    if ($line.Line -match 'version\s*=\s*"([^"]+)"')   { return $Matches[1] }
    return "parse error"
}

$vCargo   = Get-Version $CargoToml   'version\s*='
$vTauri   = Get-Version $TauriConf   '"version"'
$vPackage = Get-Version $PackageJson '"version"'

Write-Host ""
Write-Host "Current versions:" -ForegroundColor Cyan
Write-Host "  Cargo.toml         $vCargo"
Write-Host "  tauri.conf.json    $vTauri"
Write-Host "  package.json       $vPackage"
Write-Host ""

# Resolve new version — param takes priority, else prompt interactively
if ($Version) {
    if ($Version -notmatch '^\d+\.\d+\.\d+$') {
        Write-Error "Invalid semver '$Version'. Use MAJOR.MINOR.PATCH"
        exit 1
    }
    $NewVersion = $Version
} else {
    do {
        $NewVersion = Read-Host "Enter new version (semver, e.g. 0.3.0)"
        $valid = $NewVersion -match '^\d+\.\d+\.\d+$'
        if (-not $valid) { Write-Host "  Invalid semver. Use MAJOR.MINOR.PATCH" -ForegroundColor Yellow }
    } while (-not $valid)
}

Write-Host ""
Write-Host "Updating files to $NewVersion ..." -ForegroundColor Cyan

# --- Cargo.toml (TOML: version = "X.Y.Z" inside [package]) ---
$cargoContent = Get-Content $CargoToml -Raw
# Replace only the first occurrence (the [package] block version, not deps)
$cargoUpdated = $cargoContent -replace '(?m)^(version\s*=\s*)"[^"]+"', "`$1`"$NewVersion`""
Set-Content $CargoToml $cargoUpdated -NoNewline
Write-Host "  [OK] Cargo.toml"

# --- tauri.conf.json (JSON: "version": "X.Y.Z") ---
$tauriContent = Get-Content $TauriConf -Raw
$tauriUpdated = $tauriContent -replace '"version"\s*:\s*"[^"]+"', "`"version`": `"$NewVersion`""
Set-Content $TauriConf $tauriUpdated -NoNewline
Write-Host "  [OK] tauri.conf.json"

# --- package.json (JSON: "version": "X.Y.Z") ---
$pkgContent = Get-Content $PackageJson -Raw
$pkgUpdated  = $pkgContent -replace '"version"\s*:\s*"[^"]+"', "`"version`": `"$NewVersion`""
Set-Content $PackageJson $pkgUpdated -NoNewline
Write-Host "  [OK] package.json"

# Verify
Write-Host ""
Write-Host "Verifying..." -ForegroundColor Cyan
Write-Host "  Cargo.toml      -> $(Get-Version $CargoToml 'version\s*=')"
Write-Host "  tauri.conf.json -> $(Get-Version $TauriConf '"version"')"
Write-Host "  package.json    -> $(Get-Version $PackageJson '"version"')"
Write-Host ""

# --- Local build (Windows only) ---
$shouldBuild = if ($Version) { $Build } else {
    $confirm = Read-Host "Run 'pnpm tauri build' now? [Y/n]"
    $confirm -eq '' -or $confirm -match '^[Yy]'
}

if ($shouldBuild) {
    Write-Host ""
    Write-Host "Building locally (Windows)..." -ForegroundColor Cyan
    Set-Location $Root
    pnpm tauri build
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Local build failed. Aborting release."
        exit 1
    }
} else {
    Write-Host "Local build skipped." -ForegroundColor Yellow
}

# --- Release: commit + tag + push → triggers GitHub Actions ---
$shouldRelease = if ($Version) { $Release } else {
    $confirm = Read-Host "Create git tag v$NewVersion and push to trigger GitHub Actions release? [Y/n]"
    $confirm -eq '' -or $confirm -match '^[Yy]'
}

if ($shouldRelease) {
    Write-Host ""
    Write-Host "Creating release v$NewVersion ..." -ForegroundColor Cyan
    Set-Location $Root

    # Stage the three versioned files
    git add src-tauri/Cargo.toml src-tauri/tauri.conf.json package.json
    if ($LASTEXITCODE -ne 0) { Write-Error "git add failed"; exit 1 }

    git commit -m "chore: bump version to $NewVersion"
    if ($LASTEXITCODE -ne 0) { Write-Error "git commit failed"; exit 1 }
    Write-Host "  [OK] Committed version bump"

    git tag "v$NewVersion"
    if ($LASTEXITCODE -ne 0) { Write-Error "git tag failed (tag may already exist)"; exit 1 }
    Write-Host "  [OK] Tagged v$NewVersion"

    git push origin HEAD --follow-tags
    if ($LASTEXITCODE -ne 0) { Write-Error "git push failed"; exit 1 }
    Write-Host "  [OK] Pushed — GitHub Actions will now build Windows, macOS, and Linux" -ForegroundColor Green
    Write-Host ""
    Write-Host "  Track progress at: https://github.com/maosuarez/crypt-env/actions" -ForegroundColor Cyan
} else {
    Write-Host "Release skipped. Run 'git tag v$NewVersion && git push origin HEAD --follow-tags' manually when ready." -ForegroundColor Yellow
}
