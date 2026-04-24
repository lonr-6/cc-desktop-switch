param(
    [Parameter(Mandatory = $true)][string]$File,
    [string]$Signature,
    [string]$PublicKey = "release/CC-Desktop-Switch-release-public.pem"
)

$ErrorActionPreference = "Stop"

function Convert-KeyFileToBytes {
    param([string]$Text)
    $body = ($Text -split "`n" | Where-Object { $_ -notmatch "^-----" -and $_.Trim() }) -join ""
    return [Convert]::FromBase64String($body)
}

if (-not $Signature) {
    $Signature = "$File.sig"
}

$rsa = New-Object System.Security.Cryptography.RSACryptoServiceProvider
$rsa.PersistKeyInCsp = $false
$pem = Get-Content -LiteralPath $PublicKey -Raw -Encoding ascii
$rsa.ImportCspBlob((Convert-KeyFileToBytes -Text $pem))

$bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $File).Path)
$sigBytes = [Convert]::FromBase64String((Get-Content -LiteralPath $Signature -Raw -Encoding ascii).Trim())
$ok = $rsa.VerifyData($bytes, [System.Security.Cryptography.CryptoConfig]::MapNameToOID("SHA256"), $sigBytes)

if ($ok) {
    Write-Host "SIGNATURE_OK $File"
    exit 0
}

Write-Error "SIGNATURE_INVALID $File"
exit 1
