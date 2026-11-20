<#
.SYNOPSIS
  Removes nv-service-tray: stops it, deletes the logon task and the Program Files folder.
.DESCRIPTION
  Settings (%APPDATA%\nv-service-tray) and log (%LOCALAPPDATA%\nv-service-tray) are kept unless -Purge.
  -RestoreServices starts both NVIDIA services and sets NVDisplay.ContainerLocalSystem back to Automatic
  (its default before the igpu-offload task).
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1 -RestoreServices
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$TaskName = 'nv-service-tray',
    [switch]$RestoreServices,
    [switch]$Purge
)
$ErrorActionPreference = 'Continue'

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Run this script from an elevated PowerShell (Run as administrator).' }

if ($PSCmdlet.ShouldProcess($TaskName, 'stop and unregister task, stop process')) {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Get-Process nv-service-tray -ErrorAction SilentlyContinue | Stop-Process -Force
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    Write-Host "Task removed: $TaskName"
}

$destDir = Join-Path $env:ProgramFiles 'nv-service-tray'
if ((Test-Path $destDir) -and $PSCmdlet.ShouldProcess($destDir, 'remove')) {
    Start-Sleep -Milliseconds 500
    Remove-Item $destDir -Recurse -Force
    Write-Host "Removed: $destDir"
}

if ($Purge) {
    foreach ($d in (Join-Path $env:APPDATA 'nv-service-tray'), (Join-Path $env:LOCALAPPDATA 'nv-service-tray')) {
        if ((Test-Path $d) -and $PSCmdlet.ShouldProcess($d, 'remove')) { Remove-Item $d -Recurse -Force; Write-Host "Removed: $d" }
    }
}

if ($RestoreServices -and $PSCmdlet.ShouldProcess('NVIDIA services', 'restore Automatic + start')) {
    Set-Service NVDisplay.ContainerLocalSystem -StartupType Automatic
    Start-Service NVDisplay.ContainerLocalSystem
    Start-Service NvContainerLocalSystem
    Get-Service NVDisplay.ContainerLocalSystem, NvContainerLocalSystem | Format-Table Name, Status, StartType -AutoSize
}
