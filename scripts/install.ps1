<#
.SYNOPSIS
  Installs nv-service-tray: copies the exe to Program Files and registers a logon task
  that starts it elevated (no UAC prompt), then starts it.
.DESCRIPTION
  Run from an elevated PowerShell. The exe goes to an admin-only location on purpose:
  the task runs it with highest privileges, so it must not be replaceable by a normal process.
  Undo: scripts\uninstall.ps1
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1
  powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -WhatIf
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$Exe,
    [string]$TaskName = 'nv-service-tray'
)
$ErrorActionPreference = 'Stop'
# $PSScriptRoot can be empty in param() defaults on Windows PowerShell 5.1 — resolve here.
if (-not $Exe) {
    $here = Split-Path -Parent $MyInvocation.MyCommand.Path
    $Exe = Join-Path $here '..\target\release\nv-service-tray.exe'
}

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Run this script from an elevated PowerShell (Run as administrator).' }

$Exe = (Resolve-Path $Exe).Path
$destDir = Join-Path $env:ProgramFiles 'nv-service-tray'
$dest = Join-Path $destDir 'nv-service-tray.exe'
$user = "$env:USERDOMAIN\$env:USERNAME"

if ($PSCmdlet.ShouldProcess($dest, "copy $Exe")) {
    Get-Process nv-service-tray -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 500
    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
    Copy-Item $Exe $dest -Force
    Write-Host "Copied: $dest"
}

if ($PSCmdlet.ShouldProcess($TaskName, "register logon task for $user (RunLevel Highest)")) {
    $action = New-ScheduledTaskAction -Execute $dest
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal `
        -Settings $settings -Description 'nv-service-tray: NVIDIA services off in quiet G-Helper modes' -Force | Out-Null
    Write-Host "Task registered: $TaskName"
    Start-ScheduledTask -TaskName $TaskName
    Start-Sleep -Seconds 2
    $p = Get-Process nv-service-tray -ErrorAction SilentlyContinue
    Write-Host ("Running: " + [bool]$p)
}
Write-Host 'Undo: powershell -ExecutionPolicy Bypass -File scripts\uninstall.ps1 [-RestoreServices]'
