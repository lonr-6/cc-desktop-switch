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
  $OutputPath = Join-Path $repoRoot "project-docs\handoff\$date-macos-platform-smoke-evidence-summary.md"
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

function Find-Evidence {
  param(
    [object[]]$EvidenceFiles,
    [string]$Arch,
    [string]$Runner,
    [string]$ExpectedUname
  )

  $platformMarker = "platform: macOS $Arch"
  foreach ($file in $EvidenceFiles) {
    $content = Get-Content -Raw -Encoding UTF8 -LiteralPath $file.FullName
    if (-not $content.Contains($platformMarker)) {
      continue
    }

    $context = "$Arch evidence $($file.FullName)"
    Assert-Contains -Content $content -Needle "## Result" -Context $context
    Assert-Contains -Content $content -Needle "Pass" -Context $context
    Assert-Contains -Content $content -Needle "fingerprint: platform.macos_arm64_x64_smoke_path" -Context $context
    Assert-Contains -Content $content -Needle "runner: $Runner" -Context $context
    Assert-Contains -Content $content -Needle "expected_uname: $ExpectedUname" -Context $context
    Assert-Contains -Content $content -Needle "actual_uname: $ExpectedUname" -Context $context
    Assert-Contains -Content $content -Needle "workflow_run:" -Context $context
    Assert-Contains -Content $content -Needle "cargo fmt --all -- --check" -Context $context
    Assert-Contains -Content $content -Needle "cargo test --workspace" -Context $context
    Assert-Contains -Content $content -Needle "cargo clippy --workspace --all-targets -- -D warnings" -Context $context
    Assert-Contains -Content $content -Needle "trunk build --release" -Context $context
    Assert-Contains -Content $content -Needle "cargo tauri build" -Context $context
    Assert-Contains -Content $content -Needle "DMG passed hdiutil verify" -Context $context
    Assert-Contains -Content $content -Needle "PKG was created with pkgbuild" -Context $context
    Assert-Contains -Content $content -Needle "PKG expanded with pkgutil --expand" -Context $context

    return [pscustomobject]@{
      Arch = $Arch
      Runner = $Runner
      ExpectedUname = $ExpectedUname
      ActualUname = Get-Field -Content $content -Name "actual_uname" -Context $context
      WorkflowRun = Get-Field -Content $content -Name "workflow_run" -Context $context
      Commit = Get-Field -Content $content -Name "commit" -Context $context
      Version = Get-Field -Content $content -Name "version" -Context $context
      EvidencePath = $file.FullName
      ExpectedArtifact = "rust-mainline-macos-$Arch"
    }
  }

  throw "No platform-smoke-evidence.md file matched platform: macOS $Arch"
}

$evidenceFiles = @(Get-ChildItem -LiteralPath $inputRoot -Recurse -Filter "platform-smoke-evidence.md" -File)
if ($evidenceFiles.Count -lt 2) {
  throw "Expected at least two platform-smoke-evidence.md files under $inputRoot"
}

$expectedMatrix = @(
  [pscustomobject]@{
    Arch = "arm64"
    PlatformMarker = "platform: macOS arm64"
    Runner = "macos-14"
    RunnerMarker = "runner: macos-14"
    ExpectedUname = "arm64"
    ActualUnameMarker = "actual_uname: arm64"
  },
  [pscustomobject]@{
    Arch = "x64"
    PlatformMarker = "platform: macOS x64"
    Runner = "macos-15-intel"
    RunnerMarker = "runner: macos-15-intel"
    ExpectedUname = "x86_64"
    ActualUnameMarker = "actual_uname: x86_64"
  }
)

$arm64Spec = $expectedMatrix | Where-Object { $_.Arch -eq "arm64" }
$x64Spec = $expectedMatrix | Where-Object { $_.Arch -eq "x64" }
$arm64 = Find-Evidence -EvidenceFiles $evidenceFiles -Arch $arm64Spec.Arch -Runner $arm64Spec.Runner -ExpectedUname $arm64Spec.ExpectedUname
$x64 = Find-Evidence -EvidenceFiles $evidenceFiles -Arch $x64Spec.Arch -Runner $x64Spec.Runner -ExpectedUname $x64Spec.ExpectedUname

$outputParent = Split-Path -Parent $OutputPath
if (-not [string]::IsNullOrWhiteSpace($outputParent)) {
  New-Item -ItemType Directory -Path $outputParent -Force | Out-Null
}

$today = Get-Date -Format "yyyy-MM-dd"
$summary = @"
# macOS Platform Smoke Evidence Summary

Date: $today

## Result

Pass

fingerprint: platform.macos_arm64_x64_smoke_path
macos-14
macos-15-intel
workflow_run_arm64: $($arm64.WorkflowRun)
workflow_run_x64: $($x64.WorkflowRun)
artifact_arm64: $($arm64.ExpectedArtifact)
artifact_x64: $($x64.ExpectedArtifact)

## arm64

- runner: $($arm64.Runner)
- expected_uname: $($arm64.ExpectedUname)
- actual_uname: $($arm64.ActualUname)
- version: $($arm64.Version)
- commit: $($arm64.Commit)
- evidence: $($arm64.EvidencePath)

## x64

- runner: $($x64.Runner)
- expected_uname: $($x64.ExpectedUname)
- actual_uname: $($x64.ActualUname)
- version: $($x64.Version)
- commit: $($x64.Commit)
- evidence: $($x64.EvidencePath)

## Verified Gates

- Both workflow artifacts include ## Result / Pass.
- Both workflow artifacts include platform.macos_arm64_x64_smoke_path.
- arm64 evidence uses macos-14 and actual_uname: arm64.
- x64 evidence uses macos-15-intel and actual_uname: x86_64.
- Both workflow artifacts include Rust, UI, Tauri, DMG, and PKG smoke markers.

## Notes

This file records downloaded workflow artifact evidence only. It does not publish a release and does not replace real macOS Claude Desktop local config smoke.
"@

Set-Content -LiteralPath $OutputPath -Value $summary -Encoding UTF8

Write-Output "result=Pass"
Write-Output "output=$OutputPath"
Write-Output "arm64=$($arm64.EvidencePath)"
Write-Output "x64=$($x64.EvidencePath)"
