param(
  [Parameter(Mandatory = $true)]
  [string]$InputDirectory,

  [string]$Phase = "P83",

  [string]$ExpectedCommit = "",

  [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$inputRoot = Resolve-Path -LiteralPath $InputDirectory

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
  $date = Get-Date -Format "yyyy-MM-dd"
  $OutputPath = Join-Path $repoRoot "project-docs\handoff\$date-macos-real-desktop-smoke-evidence-summary.md"
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

function Resolve-ExpectedCommit {
  param([string]$Commit)

  if (-not [string]::IsNullOrWhiteSpace($Commit)) {
    return $Commit.Trim()
  }

  $resolved = (& git -C $repoRoot rev-parse HEAD 2>$null)
  if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($resolved)) {
    throw "ExpectedCommit was not provided and git rev-parse HEAD failed under $repoRoot"
  }
  return $resolved.Trim()
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

function Assert-AllowedFieldValue {
  param(
    [string]$Content,
    [string]$Name,
    [string[]]$Allowed,
    [string]$Context
  )

  $actual = Get-Field -Content $Content -Name $Name -Context $Context
  if (-not ($Allowed -contains $actual)) {
    throw "$Context field $Name expected one of '$($Allowed -join ', ')' but got '$actual'"
  }
  return $actual
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
  param(
    [object[]]$EvidenceFiles,
    [string]$Arch
  )

  $failures = @()
  foreach ($file in $EvidenceFiles) {
    try {
      $content = Get-Content -Raw -Encoding UTF8 -LiteralPath $file.FullName
      $context = "macOS real Desktop evidence $($file.FullName)"

      Assert-ResultPass -Content $content -Context $context
      Assert-Contains -Content $content -Needle "fingerprint: desktop.real_macos_local_config_smoke" -Context $context
      Assert-Contains -Content $content -Needle "description: macOS real Claude Desktop local config smoke" -Context $context
      Assert-Contains -Content $content -Needle "test_name: macos_real_desktop_local_config_smoke" -Context $context
      Assert-FieldValue -Content $content -Name "platform" -Expected "Darwin" -Context $context
      $actualArch = Assert-AllowedFieldValue -Content $content -Name "arch" -Allowed @("arm64", "x86_64") -Context $context
      if ($actualArch -ne $Arch) {
        throw "$context arch expected '$Arch' but got '$actualArch'"
      }
      $commit = Get-Field -Content $content -Name "commit" -Context $context
      if ($commit -ne $ExpectedCommit) {
        throw "$context commit expected '$ExpectedCommit' but got '$commit'"
      }
      Assert-FieldValue -Content $content -Name "mode" -Expected "run" -Context $context
      Assert-FieldValue -Content $content -Name "exit_code" -Expected "0" -Context $context
      Assert-Contains -Content $content -Needle "command: cargo test -p cc-desktop-switch --lib macos_real_desktop_local_config_smoke -- --ignored --nocapture" -Context $context
      Assert-Contains -Content $content -Needle "Readiness Markers" -Context $context
      Assert-Contains -Content $content -Needle "macOS real Claude Desktop local config smoke" -Context $context
      Assert-Contains -Content $content -Needle "configLibrary" -Context $context
      Assert-Contains -Content $content -Needle "safe route" -Context $context

      $logPath = Resolve-LogPath -LogPath (Get-Field -Content $content -Name "log" -Context $context) -EvidencePath $file.FullName
      if (-not (Test-Path -LiteralPath $logPath)) {
        throw "$context references a missing log file: $logPath"
      }

      $logContent = Get-Content -Raw -Encoding UTF8 -LiteralPath $logPath
      Assert-Contains -Content $logContent -Needle "macos_real_desktop_local_config_smoke" -Context "cargo test log $logPath"
      Assert-Contains -Content $logContent -Needle "test result: ok" -Context "cargo test log $logPath"

      return [pscustomobject]@{
        EvidencePath = $file.FullName
        LogPath = $logPath
        Command = Get-Field -Content $content -Name "command" -Context $context
        Platform = Get-Field -Content $content -Name "platform" -Context $context
        Arch = $actualArch
        Commit = $commit
        ConfigLibraryPath = Get-Field -Content $content -Name "configLibraryPath" -Context $context
      }
    } catch {
      $failures += "$($file.FullName): $($_.Exception.Message)"
    }
  }

  throw "No macos-real-desktop-smoke-evidence.md file contained pass evidence for arch '$Arch'. Checked files: $($failures -join ' | ')"
}

$ExpectedCommit = Resolve-ExpectedCommit -Commit $ExpectedCommit

$evidenceFiles = @(Get-ChildItem -LiteralPath $inputRoot -Recurse -Filter "macos-real-desktop-smoke-evidence.md" -File)
if ($evidenceFiles.Count -lt 2) {
  throw "Expected at least two macos-real-desktop-smoke-evidence.md files under $inputRoot"
}

$arm64 = Find-PassEvidence -EvidenceFiles $evidenceFiles -Arch "arm64"
$x64 = Find-PassEvidence -EvidenceFiles $evidenceFiles -Arch "x86_64"

$outputParent = Split-Path -Parent $OutputPath
if (-not [string]::IsNullOrWhiteSpace($outputParent)) {
  New-Item -ItemType Directory -Path $outputParent -Force | Out-Null
}

$today = Get-Date -Format "yyyy-MM-dd"
$summary = @"
# macOS Real Desktop Smoke Evidence Summary

Date: $today

## Result

Pass

fingerprint: desktop.real_macos_local_config_smoke
phase: $Phase
expected_commit: $ExpectedCommit
description: macOS real Claude Desktop local config smoke
test_name: macos_real_desktop_local_config_smoke
platform: Darwin
arch_arm64: $($arm64.Arch)
arch_x64: $($x64.Arch)
commit_arm64: $($arm64.Commit)
commit_x64: $($x64.Commit)
command_arm64: $($arm64.Command)
command_x64: $($x64.Command)
evidence_arm64: $($arm64.EvidencePath)
evidence_x64: $($x64.EvidencePath)
log_arm64: $($arm64.LogPath)
log_x64: $($x64.LogPath)
configLibrary_arm64: $($arm64.ConfigLibraryPath)
configLibrary_x64: $($x64.ConfigLibraryPath)

## Verified Gates

- Both wrapper evidence files include ## Result / Pass.
- Both wrapper evidence files were produced on platform: Darwin.
- Both wrapper evidence files were produced in mode: run.
- Both wrapper evidence files record exit_code: 0.
- Both wrapper evidence files match expected_commit.
- arm64 wrapper evidence records arch: arm64.
- x64 wrapper evidence records arch: x86_64.
- Both cargo test logs include macos_real_desktop_local_config_smoke.
- Both cargo test logs include test result: ok.
- Rust smoke test covers backup, readback, loopback gateway, safe route checks, Default suppression, and restored Desktop config.

## Readiness Markers

- macOS real Claude Desktop local config smoke
- configLibrary
- safe route

## Notes

This file records completed macOS real Claude Desktop local config smoke evidence only. Preflight and UnsupportedPlatform evidence are rejected by this collector.
"@

Set-Content -LiteralPath $OutputPath -Value $summary -Encoding UTF8

Write-Output "result=Pass"
Write-Output "output=$OutputPath"
Write-Output "arm64=$($arm64.EvidencePath)"
Write-Output "x64=$($x64.EvidencePath)"
