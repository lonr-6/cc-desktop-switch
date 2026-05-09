param(
  [ValidateSet("status", "export", "cleanup", "restore")]
  [string]$Mode = "status",

  [string]$BackupPath = "",

  [switch]$IUnderstandThisModifiesClaudePolicy
)

$ErrorActionPreference = "Stop"

$policyKey = "HKCU\SOFTWARE\Policies\Claude"
$policyProviderPath = "Registry::HKEY_CURRENT_USER\SOFTWARE\Policies\Claude"

function New-DefaultBackupPath {
  $timestamp = Get-Date -Format "yyyyMMddHHmmss"
  $backupDir = Join-Path $env:LOCALAPPDATA "CC Desktop Switch\policy-backups"
  New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
  Join-Path $backupDir "claude-policy-$timestamp.reg"
}

function Get-PolicyStatus {
  if (-not (Test-Path $policyProviderPath)) {
    return [ordered]@{
      exists = $false
      ccdsManaged = $false
      valueNames = @()
    }
  }

  $item = Get-Item $policyProviderPath
  $valueNames = @($item.GetValueNames() | Sort-Object)
  $ccdsManaged = $false
  if ($valueNames -contains "ccds_managed") {
    $ccdsManaged = [string]$item.GetValue("ccds_managed") -eq "true"
  }

  [ordered]@{
    exists = $true
    ccdsManaged = $ccdsManaged
    valueNames = $valueNames
  }
}

function Write-PolicyStatus {
  $status = Get-PolicyStatus
  Write-Output "exists=$($status.exists)"
  Write-Output "ccdsManaged=$($status.ccdsManaged)"
  Write-Output "valueNames=$($status.valueNames -join ',')"
}

function Export-Policy {
  param([string]$Path)

  if (-not (Test-Path $policyProviderPath)) {
    Write-Output "exportSkipped=policyMissing"
    return
  }

  $parent = Split-Path -Parent $Path
  if ($parent) {
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
  }

  reg.exe export $policyKey $Path /y | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "reg export failed with exit code $LASTEXITCODE"
  }
  Write-Output "exported=$Path"
}

switch ($Mode) {
  "status" {
    Write-PolicyStatus
  }
  "export" {
    if ([string]::IsNullOrWhiteSpace($BackupPath)) {
      $BackupPath = New-DefaultBackupPath
    }
    Export-Policy -Path $BackupPath
    Write-PolicyStatus
  }
  "cleanup" {
    if (-not $IUnderstandThisModifiesClaudePolicy) {
      throw "cleanup requires -IUnderstandThisModifiesClaudePolicy"
    }

    $status = Get-PolicyStatus
    if (-not $status.exists) {
      Write-Output "cleanupSkipped=policyMissing"
      return
    }
    if (-not $status.ccdsManaged) {
      throw "cleanup refused: ccds_managed=true marker was not found"
    }
    if ([string]::IsNullOrWhiteSpace($BackupPath)) {
      $BackupPath = New-DefaultBackupPath
    }

    Export-Policy -Path $BackupPath
    reg.exe delete $policyKey /f | Out-Null
    if ($LASTEXITCODE -ne 0) {
      throw "reg delete failed with exit code $LASTEXITCODE; backup remains at $BackupPath"
    }

    Write-Output "deleted=$policyKey"
    Write-Output "backup=$BackupPath"
    Write-PolicyStatus
  }
  "restore" {
    if (-not $IUnderstandThisModifiesClaudePolicy) {
      throw "restore requires -IUnderstandThisModifiesClaudePolicy"
    }
    if ([string]::IsNullOrWhiteSpace($BackupPath)) {
      throw "restore requires -BackupPath"
    }
    if (-not (Test-Path $BackupPath)) {
      throw "backup file not found: $BackupPath"
    }

    reg.exe import $BackupPath | Out-Null
    if ($LASTEXITCODE -ne 0) {
      throw "reg import failed with exit code $LASTEXITCODE"
    }

    Write-Output "restored=$BackupPath"
    Write-PolicyStatus
  }
}
