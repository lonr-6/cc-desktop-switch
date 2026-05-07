param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$StagingDir,
    [string]$Repository = $env:GITHUB_REPOSITORY,
    [string]$Notes,
    [string]$NotesFile,
    [string[]]$RequiredPlatforms = @("windows-x64", "macos-arm64"),
    [string]$KeyDir
)

$ErrorActionPreference = "Stop"

function Get-ProjectRoot {
    return (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
}

function Convert-BytesToKeyFile {
    param(
        [byte[]]$Bytes,
        [string]$Label
    )
    $base64 = [Convert]::ToBase64String($Bytes)
    $lines = for ($i = 0; $i -lt $base64.Length; $i += 64) {
        $base64.Substring($i, [Math]::Min(64, $base64.Length - $i))
    }
    return "-----BEGIN $Label-----`n$($lines -join "`n")`n-----END $Label-----`n"
}

function Convert-KeyFileToBytes {
    param([string]$Text)
    $body = ($Text -split "`n" | Where-Object { $_ -notmatch "^-----" -and $_.Trim() }) -join ""
    return [Convert]::FromBase64String($body)
}

function Get-OrCreateSigningKey {
    param(
        [string]$KeyDir,
        [string]$ReleaseDir
    )

    New-Item -ItemType Directory -Force -Path $KeyDir | Out-Null
    $privatePath = Join-Path $KeyDir "release-private-key.pem"
    $publicPath = Join-Path $KeyDir "release-public-key.pem"

    if (-not (Test-Path -LiteralPath $privatePath)) {
        $rsa = New-Object System.Security.Cryptography.RSACryptoServiceProvider 3072
        $rsa.PersistKeyInCsp = $false
        $privateText = Convert-BytesToKeyFile -Bytes $rsa.ExportCspBlob($true) -Label "RSA PRIVATE KEY BLOB"
        $publicText = Convert-BytesToKeyFile -Bytes $rsa.ExportCspBlob($false) -Label "RSA PUBLIC KEY BLOB"
        Set-Content -LiteralPath $privatePath -Value $privateText -Encoding ascii -NoNewline
        Set-Content -LiteralPath $publicPath -Value $publicText -Encoding ascii -NoNewline
        Write-Host "Created local release signing key: $privatePath"
    }

    Copy-Item -LiteralPath $publicPath -Destination (Join-Path $ReleaseDir "CC-Desktop-Switch-release-public.pem") -Force
    return $privatePath
}

function Get-Sha256 {
    param([string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Sign-File {
    param(
        [string]$Path,
        [string]$PrivateKeyPath
    )

    $rsa = New-Object System.Security.Cryptography.RSACryptoServiceProvider
    $rsa.PersistKeyInCsp = $false
    $pem = Get-Content -LiteralPath $PrivateKeyPath -Raw -Encoding ascii
    $rsa.ImportCspBlob((Convert-KeyFileToBytes -Text $pem))
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $signature = $rsa.SignData($bytes, [System.Security.Cryptography.CryptoConfig]::MapNameToOID("SHA256"))
    $sigPath = "$Path.sig"
    Set-Content -LiteralPath $sigPath -Value ([Convert]::ToBase64String($signature)) -Encoding ascii -NoNewline
    return $sigPath
}

function Get-AssetUrl {
    param(
        [string]$FileName,
        [string]$Version,
        [string]$Repository
    )
    if ($Repository) {
        return "https://github.com/$Repository/releases/download/v$Version/$FileName"
    }
    return $FileName
}

function Get-PlatformForAsset {
    param(
        [string]$Name,
        [string]$Version
    )

    if ($Name -in @(
            "CC-Desktop-Switch-v$Version-Windows-x64.exe",
            "CC-Desktop-Switch-v$Version-Windows-Portable.zip",
            "CC-Desktop-Switch-v$Version-Windows-Setup.exe"
        )) {
        return "windows-x64"
    }

    $escapedVersion = [regex]::Escape($Version)
    if ($Name -match "^CC-Desktop-Switch-v$escapedVersion-macOS-(?<arch>[^.]+)\.(pkg|dmg)$") {
        return "macos-$($Matches["arch"])"
    }

    return $null
}

function Add-SignedAsset {
    param(
        [System.Collections.IDictionary]$Platforms,
        [string]$Platform,
        [System.IO.FileInfo]$File,
        [string]$PrivateKeyPath,
        [string]$Version,
        [string]$Repository
    )

    if (-not $Platforms.Contains($Platform)) {
        $Platforms[$Platform] = [ordered]@{
            assets = [System.Collections.Generic.List[object]]::new()
        }
    }

    $sha = Get-Sha256 -Path $File.FullName
    Set-Content -LiteralPath "$($File.FullName).sha256" -Value "$sha  $($File.Name)" -Encoding ascii
    $sigPath = Sign-File -Path $File.FullName -PrivateKeyPath $PrivateKeyPath

    $Platforms[$Platform].assets.Add([ordered]@{
        name = $File.Name
        url = Get-AssetUrl -FileName $File.Name -Version $Version -Repository $Repository
        signature = Split-Path -Leaf $sigPath
        sha256 = $sha
        size = $File.Length
    }) | Out-Null
}

function Get-ReleaseNotes {
    param(
        [string]$Notes,
        [string]$NotesFile,
        [string]$Version
    )

    if ($NotesFile) {
        if (-not (Test-Path -LiteralPath $NotesFile)) {
            throw "Release notes file not found: $NotesFile"
        }
        return Get-Content -LiteralPath $NotesFile -Raw -Encoding utf8
    }

    if ($Notes) {
        return $Notes
    }

    return "Release for CC Desktop Switch v$Version."
}

$root = Get-ProjectRoot
if (-not $KeyDir) {
    $KeyDir = Join-Path $root ".release-signing"
}

if ([System.IO.Path]::IsPathRooted($StagingDir)) {
    $releaseDir = $StagingDir
} else {
    $releaseDir = Join-Path $root $StagingDir
}

if (-not (Test-Path -LiteralPath $releaseDir)) {
    throw "Release staging directory not found: $releaseDir"
}

foreach ($pattern in @("*.sha256", "*.sig", "latest.json", "CC-Desktop-Switch-release-public.pem")) {
    Get-ChildItem -LiteralPath $releaseDir -File -Filter $pattern -ErrorAction SilentlyContinue |
        Remove-Item -Force
}

$requiredAssetNames = @(
    "CC-Desktop-Switch-v$Version-Windows-x64.exe",
    "CC-Desktop-Switch-v$Version-Windows-Portable.zip",
    "CC-Desktop-Switch-v$Version-Windows-Setup.exe",
    "CC-Desktop-Switch-v$Version-macOS-arm64.pkg",
    "CC-Desktop-Switch-v$Version-macOS-arm64.dmg"
)

foreach ($assetName in $requiredAssetNames) {
    if (-not (Test-Path -LiteralPath (Join-Path $releaseDir $assetName))) {
        throw "Required release asset missing: $assetName"
    }
}

$privateKey = Get-OrCreateSigningKey -KeyDir $KeyDir -ReleaseDir $releaseDir
$platforms = [ordered]@{}
$assetFiles = Get-ChildItem -LiteralPath $releaseDir -File |
    Where-Object { $_.Name -like "CC-Desktop-Switch-v$Version-*" } |
    Sort-Object Name

foreach ($assetFile in $assetFiles) {
    $platform = Get-PlatformForAsset -Name $assetFile.Name -Version $Version
    if (-not $platform) {
        throw "Unrecognized release asset name: $($assetFile.Name)"
    }
    Add-SignedAsset -Platforms $platforms -Platform $platform -File $assetFile -PrivateKeyPath $privateKey -Version $Version -Repository $Repository
}

foreach ($platform in $RequiredPlatforms) {
    if (-not $platforms.Contains($platform)) {
        throw "Required release platform missing: $platform"
    }
    if ($platforms[$platform].assets.Count -lt 1) {
        throw "Required release platform has no assets: $platform"
    }
}

$latest = [ordered]@{
    name = "CC Desktop Switch"
    version = $Version
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    notes = Get-ReleaseNotes -Notes $Notes -NotesFile $NotesFile -Version $Version
    update_protocol = 1
    minimum_supported_version = "1.0.0"
    platforms = $platforms
    signature = [ordered]@{
        algorithm = "RSA-CSP-BLOB-SHA256"
        public_key = "CC-Desktop-Switch-release-public.pem"
        format = "base64 raw signature over file bytes"
    }
}

$latestPath = Join-Path $releaseDir "latest.json"
$latestJson = $latest | ConvertTo-Json -Depth 8
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($latestPath, $latestJson, $utf8NoBom)
$latestItem = Get-Item -LiteralPath $latestPath
$latestSha = Get-Sha256 -Path $latestItem.FullName
Set-Content -LiteralPath "$($latestItem.FullName).sha256" -Value "$latestSha  $($latestItem.Name)" -Encoding ascii
Sign-File -Path $latestItem.FullName -PrivateKeyPath $privateKey | Out-Null

Get-ChildItem -LiteralPath $releaseDir -File | Sort-Object Name | Select-Object Name, Length
