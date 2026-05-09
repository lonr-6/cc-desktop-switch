param(
  [Parameter(Mandatory = $true)]
  [string]$InputDirectory,

  [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$inputRoot = Resolve-Path -LiteralPath $InputDirectory

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
  $date = Get-Date -Format "yyyy-MM-dd"
  $OutputPath = Join-Path $repoRoot "project-docs\handoff\$date-windows-real-desktop-smoke-evidence-summary.md"
}

function Assert-Contains {
  param(
    [string]$Content,
    [string]$Needle,
    [string]$Context
  )

  if (-not $Content.Contains($Needle)) {
    throw "$Context is missing required marker: $Needle"
  }
}

function Assert-ResultPass {
  param(
    [string]$Content,
    [string]$Context
  )

  if ($Content -notmatch "(?ms)^## Result\s*\r?\n\s*Pass\s*(\r?\n|$)") {
    throw "$Context is missing required result block: ## Result / Pass"
  }
}

function Get-Field {
  param(
    [string]$Content,
    [string]$Name,
    [string]$Context
  )

  $pattern = "(?m)^" + [regex]::Escape($Name) + ":\s*(.+?)\s*$"
  $match = [regex]::Match($Content, $pattern)
  if (-not $match.Success) {
    throw "$Context is missing required field: $Name"
  }
  return $match.Groups[1].Value.Trim()
}

function Assert-FieldValue {
  param(
    [string]$Content,
    [string]$Name,
    [string]$Expected,
    [string]$Context
  )

  $actual = Get-Field -Content $Content -Name $Name -Context $Context
  if ($actual -ne $Expected) {
    throw "$Context field $Name expected '$Expected' but got '$actual'"
  }
}

function Resolve-LogPath {
  param(
    [string]$LogPath,
    [string]$EvidencePath
  )

  if ([string]::IsNullOrWhiteSpace($LogPath) -or $LogPath -eq "not-run") {
    throw "$EvidencePath is missing a real cargo test log path"
  }

  if ([System.IO.Path]::IsPathRooted($LogPath)) {
    return $LogPath
  }

  return Join-Path (Split-Path -Parent $EvidencePath) $LogPath
}

function Find-PassEvidence {
  param([object[]]$EvidenceFiles)

  $failures = @()
  foreach ($file in $EvidenceFiles) {
    try {
      $content = Get-Content -Raw -Encoding UTF8 -LiteralPath $file.FullName
      $context = "Windows real Desktop evidence $($file.FullName)"

      Assert-ResultPass -Content $content -Context $context
      Assert-Contains -Content $content -Needle "fingerprint: desktop.real_windows_local_config_smoke" -Context $context
      Assert-Contains -Content $content -Needle "test_name: windows_real_desktop_local_config_smoke" -Context $context
      Assert-FieldValue -Content $content -Name "mode" -Expected "run" -Context $context
      Assert-FieldValue -Content $content -Name "exit_code" -Expected "0" -Context $context
      Assert-Contains -Content $content -Needle "command: cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture" -Context $context
      Assert-Contains -Content $content -Needle "Readiness Markers" -Context $context
      Assert-Contains -Content $content -Needle "windows_real_desktop_local_config_smoke" -Context $context
      Assert-Contains -Content $content -Needle "loopback gateway" -Context $context
      Assert-Contains -Content $content -Needle "restored" -Context $context

      $logPath = Resolve-LogPath -LogPath (Get-Field -Content $content -Name "log" -Context $context) -EvidencePath $file.FullName
      if (-not (Test-Path -LiteralPath $logPath)) {
        throw "$context references a missing log file: $logPath"
      }

      $logContent = Get-Content -Raw -Encoding UTF8 -LiteralPath $logPath
      Assert-Contains -Content $logContent -Needle "windows_real_desktop_local_config_smoke" -Context "cargo test log $logPath"
      Assert-Contains -Content $logContent -Needle "test result: ok" -Context "cargo test log $logPath"

      return [pscustomobject]@{
        EvidencePath = $file.FullName
        LogPath = $logPath
        Command = Get-Field -Content $content -Name "command" -Context $context
        ConfigLibraryPath = Get-Field -Content $content -Name "configLibraryPath" -Context $context
      }
    } catch {
      $failures += "$($file.FullName): $($_.Exception.Message)"
    }
  }

  throw "No windows-real-desktop-smoke-evidence.md file contained pass evidence. Checked files: $($failures -join ' | ')"
}

$evidenceFiles = @(Get-ChildItem -LiteralPath $inputRoot -Recurse -Filter "windows-real-desktop-smoke-evidence.md" -File)
if ($evidenceFiles.Count -lt 1) {
  throw "Expected at least one windows-real-desktop-smoke-evidence.md file under $inputRoot"
}

$evidence = Find-PassEvidence -EvidenceFiles $evidenceFiles

$outputParent = Split-Path -Parent $OutputPath
if (-not [string]::IsNullOrWhiteSpace($outputParent)) {
  New-Item -ItemType Directory -Path $outputParent -Force | Out-Null
}

$today = Get-Date -Format "yyyy-MM-dd"
$summary = @"
# Windows Real Desktop Smoke Evidence Summary

Date: $today

## Result

Pass

fingerprint: desktop.real_windows_local_config_smoke
test_name: windows_real_desktop_local_config_smoke
command: $($evidence.Command)
evidence: $($evidence.EvidencePath)
log: $($evidence.LogPath)
configLibrary: $($evidence.ConfigLibraryPath)

## Verified Gates

- Wrapper evidence includes ## Result / Pass.
- Wrapper evidence was produced in mode: run.
- Wrapper evidence records exit_code: 0.
- Cargo test log includes windows_real_desktop_local_config_smoke.
- Cargo test log includes test result: ok.
- Rust smoke test covers backup, readback, loopback gateway, and restored Desktop config.

## Readiness Markers

- windows_real_desktop_local_config_smoke
- loopback gateway
- restored

## Notes

This file records completed Windows real Claude Desktop local config smoke evidence only. Preflight evidence is rejected by this collector.
"@

Set-Content -LiteralPath $OutputPath -Value $summary -Encoding UTF8

Write-Output "result=Pass"
Write-Output "output=$OutputPath"
Write-Output "evidence=$($evidence.EvidencePath)"
Write-Output "log=$($evidence.LogPath)"
