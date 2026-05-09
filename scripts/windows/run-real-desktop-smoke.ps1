param(
  [ValidateSet("preflight", "run")]
  [string]$Mode = "preflight",

  [switch]$AllowRealDesktopWrite,

  [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$maintenanceScript = Join-Path $PSScriptRoot "ccds-managed-policy-maintenance.ps1"

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $OutputDirectory = Join-Path $repoRoot "target\real-desktop-smoke"
}

function Join-Lines {
  param([string[]]$Lines)

  if ($null -eq $Lines -or $Lines.Count -eq 0) {
    return ""
  }
  return ($Lines -join [Environment]::NewLine)
}

function Write-Evidence {
  param(
    [string]$Path,
    [string]$Result,
    [string]$PolicyStatus,
    [string]$ConfigLibraryPath,
    [bool]$ConfigLibraryExists,
    [string]$Command,
    [string]$LogPath,
    [Nullable[int]]$ExitCode
  )

  $exitText = "not-run"
  if ($null -ne $ExitCode) {
    $exitText = [string]$ExitCode
  }

  $body = @"
# Windows Real Desktop Smoke Evidence

## Result

$Result

fingerprint: desktop.real_windows_local_config_smoke
test_name: windows_real_desktop_local_config_smoke
mode: $Mode
command: $Command
exit_code: $exitText
log: $LogPath
configLibraryPath: $ConfigLibraryPath

## Preflight

~~~text
$PolicyStatus
configLibraryExists=$ConfigLibraryExists
configLibraryPath=$ConfigLibraryPath
~~~

## Notes

- preflight is read-only and must not be treated as pass evidence.
- run requires -AllowRealDesktopWrite and sets CCDS_ALLOW_REAL_DESKTOP_WRITE=1 only for the test process.
- A pass still needs backup, readback, loopback gateway, and restore evidence from the Rust ignored test.

## Readiness Markers

- windows_real_desktop_local_config_smoke
- loopback gateway
- restored
"@

  Set-Content -LiteralPath $Path -Value $body -Encoding UTF8
}

Push-Location $repoRoot
try {
  New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

  $policyLines = @(& powershell -NoProfile -ExecutionPolicy Bypass -File $maintenanceScript -Mode status)
  $policyStatus = Join-Lines -Lines $policyLines
  $configLibraryPath = Join-Path $env:LOCALAPPDATA "Claude-3p\configLibrary"
  $configLibraryExists = Test-Path -LiteralPath $configLibraryPath

  $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
  $evidencePath = Join-Path $OutputDirectory "windows-real-desktop-smoke-evidence.md"
  $logPath = Join-Path $OutputDirectory "windows-real-desktop-smoke-$timestamp.log"
  $commandText = "not-run"
  $exitCode = $null
  $result = "Preflight"

  if ($Mode -eq "run") {
    if (-not $AllowRealDesktopWrite) {
      throw "run mode requires -AllowRealDesktopWrite"
    }

    $commandText = "cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture"
    $previousValue = $env:CCDS_ALLOW_REAL_DESKTOP_WRITE
    $previousErrorActionPreference = $ErrorActionPreference
    try {
      $env:CCDS_ALLOW_REAL_DESKTOP_WRITE = "1"
      $ErrorActionPreference = "Continue"
      $output = & cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture 2>&1
      $exitCode = $LASTEXITCODE
      $output | Set-Content -LiteralPath $logPath -Encoding UTF8
    } finally {
      $ErrorActionPreference = $previousErrorActionPreference
      if ($null -eq $previousValue) {
        Remove-Item Env:CCDS_ALLOW_REAL_DESKTOP_WRITE -ErrorAction SilentlyContinue
      } else {
        $env:CCDS_ALLOW_REAL_DESKTOP_WRITE = $previousValue
      }
    }

    if ($exitCode -eq 0) {
      $result = "Pass"
    } else {
      $result = "Fail"
    }
  }

  Write-Evidence `
    -Path $evidencePath `
    -Result $result `
    -PolicyStatus $policyStatus `
    -ConfigLibraryPath $configLibraryPath `
    -ConfigLibraryExists $configLibraryExists `
    -Command $commandText `
    -LogPath $logPath `
    -ExitCode $exitCode

  Write-Output "result=$result"
  Write-Output "evidence=$evidencePath"
  Write-Output "policyStatus=$($policyLines -join ';')"
  Write-Output "configLibraryExists=$configLibraryExists"

  if ($Mode -eq "run" -and $exitCode -ne 0) {
    exit $exitCode
  }
} finally {
  Pop-Location
}
