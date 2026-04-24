param(
    [Parameter(Mandatory = $true)][string[]]$Files,
    [string]$CertificatePath,
    [string]$CertificatePassword,
    [string]$CertificateBase64,
    [string]$TimestampServer = "http://timestamp.digicert.com"
)

$ErrorActionPreference = "Stop"

function Import-ReleaseCertificate {
    param(
        [string]$Path,
        [string]$Password,
        [string]$Base64
    )

    $tempPath = $null
    if ($Base64) {
        $tempPath = Join-Path $env:TEMP ("ccds-codesign-" + [Guid]::NewGuid().ToString("N") + ".pfx")
        [System.IO.File]::WriteAllBytes($tempPath, [Convert]::FromBase64String($Base64))
        $Path = $tempPath
    }

    if (-not $Path -or -not (Test-Path -LiteralPath $Path)) {
        throw "Code signing certificate not found. Provide -CertificatePath or -CertificateBase64."
    }

    if ($null -eq $Password) {
        $Password = ""
    }
    $securePassword = ConvertTo-SecureString -String $Password -AsPlainText -Force
    $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2
    $cert.Import(
        (Resolve-Path -LiteralPath $Path).Path,
        $securePassword,
        [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable
    )

    if (-not $cert.HasPrivateKey) {
        throw "Code signing certificate does not contain a private key."
    }

    if ($tempPath -and (Test-Path -LiteralPath $tempPath)) {
        Remove-Item -LiteralPath $tempPath -Force
    }

    return $cert
}

$cert = Import-ReleaseCertificate -Path $CertificatePath -Password $CertificatePassword -Base64 $CertificateBase64

foreach ($file in $Files) {
    if (-not (Test-Path -LiteralPath $file)) {
        throw "File not found for code signing: $file"
    }

    $result = Set-AuthenticodeSignature `
        -LiteralPath $file `
        -Certificate $cert `
        -TimestampServer $TimestampServer `
        -HashAlgorithm SHA256

    if ($result.Status -ne "Valid") {
        throw "Code signing failed for $file. Status: $($result.Status). Message: $($result.StatusMessage)"
    }

    Write-Host "AUTHENTICODE_SIGNED $file"
}
