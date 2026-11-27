<#
.SYNOPSIS
    Generates Microsoft Store icon tiles from assets\velvet.png, compiles the
    optimized dist binary, and packs dist\Velvet-<version>-x64.msix with makeappx.exe.
.PARAMETER SkipCargo
    Reuse the existing target\dist\velvet.exe without running `cargo build --profile dist`.
#>
param(
    [switch]$SkipCargo
)

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$MsixDir = $PSScriptRoot
$AssetsOutDir = Join-Path $MsixDir "Assets"
$StoreDir = Join-Path $RepoRoot "packaging\store"
$SourcePng = Join-Path $RepoRoot "assets\velvet.png"
$ManifestSrc = Join-Path $MsixDir "AppxManifest.xml"
$StagingDir = Join-Path $RepoRoot "target\msix-staging"
$DistDir = Join-Path $RepoRoot "dist"

# Parse version from Cargo.toml
$CargoToml = Get-Content (Join-Path $RepoRoot "Cargo.toml") -Raw
if ($CargoToml -match '(?m)^version\s*=\s*"([^"]+)"') {
    $CrateVersion = $Matches[1]
} else {
    throw "Could not parse package version from Cargo.toml"
}
$MsixVersion = "$CrateVersion.0"

if (-not $SkipCargo) {
    Write-Host "Building dist binary (cargo build --profile dist)..."
    Push-Location $RepoRoot
    try {
        cargo build --profile dist
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build --profile dist failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

$DistExe = Join-Path $RepoRoot "target\dist\velvet.exe"
if (-not (Test-Path $DistExe)) {
    throw "Expected binary not found at $DistExe. Run without -SkipCargo."
}
if (-not (Test-Path $SourcePng)) {
    throw "Source icon not found at $SourcePng."
}

# Render MSIX tile assets from assets\velvet.png
Write-Host "Generating MSIX tile PNG assets from assets\velvet.png..."
New-Item -ItemType Directory -Force -Path $AssetsOutDir | Out-Null
New-Item -ItemType Directory -Force -Path $StoreDir | Out-Null

Add-Type -AssemblyName System.Drawing

function New-ResizedIcon {
    param(
        [System.Drawing.Image]$SourceImage,
        [int]$Width,
        [int]$Height,
        [int]$IconSize,
        [string]$OutputPath
    )

    $bmp = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $gfx.Clear([System.Drawing.Color]::Transparent)
        $gfx.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceOver
        $gfx.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $gfx.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $gfx.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $gfx.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

        $x = [int](($Width - $IconSize) / 2)
        $y = [int](($Height - $IconSize) / 2)
        $destRect = New-Object System.Drawing.Rectangle($x, $y, $IconSize, $IconSize)
        $gfx.DrawImage($SourceImage, $destRect)
        $bmp.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $gfx.Dispose()
        $bmp.Dispose()
    }
}

$srcImg = [System.Drawing.Image]::FromFile($SourcePng)
try {
    New-ResizedIcon -SourceImage $srcImg -Width 50  -Height 50  -IconSize 46  -OutputPath (Join-Path $AssetsOutDir "StoreLogo.png")
    New-ResizedIcon -SourceImage $srcImg -Width 44  -Height 44  -IconSize 40  -OutputPath (Join-Path $AssetsOutDir "Square44x44Logo.png")
    New-ResizedIcon -SourceImage $srcImg -Width 24  -Height 24  -IconSize 24  -OutputPath (Join-Path $AssetsOutDir "Square44x44Logo.targetsize-24_altform-unplated.png")
    New-ResizedIcon -SourceImage $srcImg -Width 48  -Height 48  -IconSize 48  -OutputPath (Join-Path $AssetsOutDir "Square44x44Logo.targetsize-48_altform-unplated.png")
    New-ResizedIcon -SourceImage $srcImg -Width 150 -Height 150 -IconSize 104 -OutputPath (Join-Path $AssetsOutDir "Square150x150Logo.png")
    New-ResizedIcon -SourceImage $srcImg -Width 310 -Height 150 -IconSize 104 -OutputPath (Join-Path $AssetsOutDir "Wide310x150Logo.png")
    New-ResizedIcon -SourceImage $srcImg -Width 300 -Height 300 -IconSize 270 -OutputPath (Join-Path $StoreDir "StoreIcon-300x300.png")
} finally {
    $srcImg.Dispose()
}

# Stage files for makeappx
if (Test-Path $StagingDir) {
    Remove-Item -Recurse -Force $StagingDir
}
New-Item -ItemType Directory -Force -Path (Join-Path $StagingDir "Assets") | Out-Null
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

Copy-Item $DistExe (Join-Path $StagingDir "velvet.exe")
Copy-Item (Join-Path $AssetsOutDir "*.png") (Join-Path $StagingDir "Assets\")

# Patch only the Identity Version attribute (avoid touching MinVersion/MaxVersionTested)
$ManifestContent = Get-Content $ManifestSrc -Raw
$ManifestContent = $ManifestContent -creplace '(\s)Version="[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+"', "`$1Version=`"$MsixVersion`""
[System.IO.File]::WriteAllText((Join-Path $StagingDir "AppxManifest.xml"), $ManifestContent, [System.Text.UTF8Encoding]::new($false))

# Find makeappx.exe from Windows 10/11 SDK
$MakeAppx = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Filter "makeappx.exe" -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\makeappx\.exe$' } |
    Sort-Object FullName -Descending |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $MakeAppx) {
    throw "makeappx.exe (x64) not found under C:\Program Files (x86)\Windows Kits\10\bin"
}

$OutMsix = Join-Path $DistDir "Velvet-$CrateVersion-x64.msix"
Write-Host "Packing $OutMsix using $MakeAppx..."
& $MakeAppx pack /o /d $StagingDir /p $OutMsix
if ($LASTEXITCODE -ne 0) {
    throw "makeappx.exe failed with exit code $LASTEXITCODE"
}

$SizeMB = [Math]::Round((Get-Item $OutMsix).Length / 1MB, 2)
Write-Host "Created MSIX package: $OutMsix ($SizeMB MB)"
