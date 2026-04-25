param(
    [string]$Version = "1.0.2",
    [string]$OutputDir = "release",
    [switch]$Build,
    [switch]$TryInstaller,
    [switch]$CodeSign,
    [string]$CodeSigningCertificatePath,
    [string]$CodeSigningCertificatePassword,
    [string]$CodeSigningCertificateBase64,
    [string]$TimestampServer = "http://timestamp.digicert.com",
    [string]$Repository = $env:GITHUB_REPOSITORY
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

function Get-Makensis {
    $cmd = Get-Command makensis -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $candidates = @(
        "C:\Program Files (x86)\NSIS\makensis.exe",
        "C:\Program Files\NSIS\makensis.exe"
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }
    return $null
}

function Add-Asset {
    param(
        [System.Collections.Generic.List[object]]$Assets,
        [string]$Path,
        [string]$PrivateKeyPath
    )

    $sha = Get-Sha256 -Path $Path
    $shaPath = "$Path.sha256"
    Set-Content -LiteralPath $shaPath -Value "$sha  $(Split-Path -Leaf $Path)" -Encoding ascii
    $sigPath = Sign-File -Path $Path -PrivateKeyPath $PrivateKeyPath

    $Assets.Add([ordered]@{
        name = Split-Path -Leaf $Path
        url = Get-AssetUrl -FileName (Split-Path -Leaf $Path)
        signature = Split-Path -Leaf $sigPath
        sha256 = $sha
        size = (Get-Item -LiteralPath $Path).Length
    }) | Out-Null
}

function Get-AssetUrl {
    param([string]$FileName)
    if ($Repository) {
        return "https://github.com/$Repository/releases/download/v$Version/$FileName"
    }
    return $FileName
}

function Invoke-OptionalCodeSigning {
    param([string[]]$Files)

    if (-not $CodeSign) {
        return
    }

    $base64 = $CodeSigningCertificateBase64
    if (-not $base64 -and $env:WINDOWS_CODESIGN_PFX_BASE64) {
        $base64 = $env:WINDOWS_CODESIGN_PFX_BASE64
    }

    $password = $CodeSigningCertificatePassword
    if (-not $password -and $env:WINDOWS_CODESIGN_PFX_PASSWORD) {
        $password = $env:WINDOWS_CODESIGN_PFX_PASSWORD
    }

    $args = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", (Join-Path $PSScriptRoot "Invoke-CodeSigning.ps1"),
        "-Files"
    ) + $Files + @("-TimestampServer", $TimestampServer)

    if ($CodeSigningCertificatePath) {
        $args += @("-CertificatePath", $CodeSigningCertificatePath)
    }
    if ($password) {
        $args += @("-CertificatePassword", $password)
    }
    if ($base64) {
        $args += @("-CertificateBase64", $base64)
    }

    & powershell @args
    if ($LASTEXITCODE -ne 0) {
        throw "Authenticode signing failed."
    }
}

$root = Get-ProjectRoot
$releaseDir = Join-Path $root $OutputDir
$distDir = Join-Path $root "dist"
$folderDist = Join-Path $distDir "CC-Desktop-Switch"
$oneFileExe = Join-Path $distDir "CC-Desktop-Switch.exe"
$setupExe = Join-Path $root "CC-Desktop-Switch-Setup-$Version.exe"

Set-Location $root
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
foreach ($pattern in @(
    "CC-Desktop-Switch-v*-Windows-*",
    "CC-Desktop-Switch-release-public.pem",
    "latest.json",
    "latest.json.sha256",
    "latest.json.sig"
)) {
    Get-ChildItem -LiteralPath $releaseDir -File -Filter $pattern -ErrorAction SilentlyContinue |
        Remove-Item -Force
}

if ($Build) {
    $env:CCDS_ONEFILE = ""
    python -m PyInstaller --noconfirm --clean build.spec
    $env:CCDS_ONEFILE = "1"
    python -m PyInstaller --noconfirm --clean build.spec
    Remove-Item Env:\CCDS_ONEFILE -ErrorAction SilentlyContinue
}

if (-not (Test-Path -LiteralPath $folderDist)) {
    throw "Folder build not found: $folderDist"
}
if (-not (Test-Path -LiteralPath $oneFileExe)) {
    throw "One-file exe not found: $oneFileExe"
}

$licensePath = Join-Path $root "LICENSE.txt"
if (Test-Path -LiteralPath $licensePath) {
    Copy-Item -LiteralPath $licensePath -Destination (Join-Path $folderDist "LICENSE.txt") -Force
}

$folderExe = Join-Path $folderDist "CC-Desktop-Switch.exe"
Invoke-OptionalCodeSigning -Files @($folderExe, $oneFileExe)

if ($TryInstaller) {
    $makensis = Get-Makensis
    if ($makensis) {
        & $makensis installer.nsi
        if ($LASTEXITCODE -ne 0) {
            throw "NSIS failed with exit code $LASTEXITCODE"
        }
    } else {
        Write-Warning "NSIS makensis not found. Skipping Setup installer generation."
    }
}

$keyDir = Join-Path $root ".release-signing"
$privateKey = Get-OrCreateSigningKey -KeyDir $keyDir -ReleaseDir $releaseDir
$assets = [System.Collections.Generic.List[object]]::new()

$portableZip = Join-Path $releaseDir "CC-Desktop-Switch-v$Version-Windows-Portable.zip"
if (Test-Path -LiteralPath $portableZip) { Remove-Item -LiteralPath $portableZip -Force }
Compress-Archive -Path (Join-Path $folderDist "*") -DestinationPath $portableZip -Force
Add-Asset -Assets $assets -Path $portableZip -PrivateKeyPath $privateKey

$releaseExe = Join-Path $releaseDir "CC-Desktop-Switch-v$Version-Windows-x64.exe"
Copy-Item -LiteralPath $oneFileExe -Destination $releaseExe -Force

$releaseSetup = $null
if (Test-Path -LiteralPath $setupExe) {
    $releaseSetup = Join-Path $releaseDir "CC-Desktop-Switch-v$Version-Windows-Setup.exe"
    Copy-Item -LiteralPath $setupExe -Destination $releaseSetup -Force
}

if ($CodeSign -and $releaseSetup -and (Test-Path -LiteralPath $releaseSetup)) {
    Invoke-OptionalCodeSigning -Files @($releaseSetup)
}

Add-Asset -Assets $assets -Path $releaseExe -PrivateKeyPath $privateKey
if ($releaseSetup -and (Test-Path -LiteralPath $releaseSetup)) {
    Add-Asset -Assets $assets -Path $releaseSetup -PrivateKeyPath $privateKey
}

$latest = [ordered]@{
    name = "CC Desktop Switch"
    version = $Version
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    notes = "Windows release for CC Desktop Switch v$Version."
    update_protocol = 1
    minimum_supported_version = "1.0.0"
    platforms = [ordered]@{
        "windows-x64" = [ordered]@{
            assets = $assets
        }
    }
    signature = [ordered]@{
        algorithm = "RSA-CSP-BLOB-SHA256"
        public_key = "CC-Desktop-Switch-release-public.pem"
        format = "base64 raw signature over file bytes"
    }
}

$latestPath = Join-Path $releaseDir "latest.json"
$latest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $latestPath -Encoding utf8
Add-Asset -Assets ([System.Collections.Generic.List[object]]::new()) -Path $latestPath -PrivateKeyPath $privateKey | Out-Null

Get-ChildItem -LiteralPath $releaseDir -File | Sort-Object Name | Select-Object Name, Length
