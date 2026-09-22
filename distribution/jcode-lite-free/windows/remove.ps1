$ErrorActionPreference = "Stop"
$root = Join-Path $env:LOCALAPPDATA "LeGrin\JcodeLiteFree"
$purgeData = ($args -contains "--purge-data") -or ($args -contains "-PurgeData") -or ($env:JCODE_LITE_FREE_REMOVE_PURGE_DATA -eq "1")

if ($args | Where-Object { $_ -notin @("--purge-data", "-PurgeData") }) {
  throw "Unknown argument. Use remove.ps1 or remove.ps1 --purge-data."
}
if (!(Test-Path $root)) {
  Write-Host "Jcode Lite Free is not installed."
  exit 0
}

if ($purgeData -and $env:JCODE_LITE_FREE_REMOVE_NO_CONFIRM -ne "1") {
  Write-Host "This permanently deletes all Jcode Lite Free sessions, memory, credentials, and config in:"
  Write-Host "  $(Join-Path $root 'home')"
  if ([Console]::IsInputRedirected) {
    Write-Host "Non-interactive session: set JCODE_LITE_FREE_REMOVE_NO_CONFIRM=1 to confirm."
    exit 1
  }
  $answer = Read-Host "Type DELETE to continue"
  if ($answer -ne "DELETE") {
    Write-Host "Cancelled. Nothing was removed."
    exit 1
  }
}

$cliDir = Join-Path $root "bin"
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath) {
  $pathEntries = @($userPath -split ';' | Where-Object { $_ })
  $kept = $pathEntries | Where-Object { ![String]::Equals($_.TrimEnd('\'), $cliDir.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) }
  if ($kept.Count -ne $pathEntries.Count) {
    [Environment]::SetEnvironmentVariable("Path", ($kept -join ';'), "User")
  }
}

Get-Process jcode -ErrorAction SilentlyContinue | Where-Object {
  $_.Path -and $_.Path.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)
} | Stop-Process -Force -ErrorAction SilentlyContinue

if ($purgeData) {
  $taskName = "JcodeLiteFreeRemove-" + [guid]::NewGuid().ToString("N")
  $scriptPath = Join-Path $env:TEMP "$taskName.ps1"
  @"
Start-Sleep -Seconds 2
Remove-Item -LiteralPath '$($root.Replace("'", "''"))' -Recurse -Force -ErrorAction SilentlyContinue
schtasks.exe /Delete /TN '$taskName' /F
Remove-Item -LiteralPath '$($scriptPath.Replace("'", "''"))' -Force -ErrorAction SilentlyContinue
"@ | Set-Content -LiteralPath $scriptPath -Encoding UTF8
  $taskAction = "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$scriptPath`""
  schtasks.exe /Create /TN $taskName /TR $taskAction /SC ONCE /ST 23:59 /F /RL LIMITED *> $null
  schtasks.exe /Run /TN $taskName *> $null
  Write-Host "Removed Jcode Lite Free and all isolated user data."
} else {
  # The installed jcodef.cmd/jcodef.ps1 wrapper lives below $root. Schedule the
  # cleanup so cmd.exe can finish reading its own wrapper before bin/ is removed.
  $taskName = "JcodeLiteFreeRemoveApp-" + [guid]::NewGuid().ToString("N")
  $scriptPath = Join-Path $env:TEMP "$taskName.ps1"
  @"
Start-Sleep -Seconds 2
`$cleanupRoot = '$($root.Replace("'", "''"))'
Get-ChildItem -LiteralPath `$cleanupRoot -Force -ErrorAction SilentlyContinue | ForEach-Object {
  if (`$_.Name -ne 'home') { Remove-Item -LiteralPath `$_.FullName -Recurse -Force -ErrorAction SilentlyContinue }
}
schtasks.exe /Delete /TN '$taskName' /F
Remove-Item -LiteralPath '$($scriptPath.Replace("'", "''"))' -Force -ErrorAction SilentlyContinue
"@ | Set-Content -LiteralPath $scriptPath -Encoding UTF8
  $taskAction = "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$scriptPath`""
  schtasks.exe /Create /TN $taskName /TR $taskAction /SC ONCE /ST 23:59 /F /RL LIMITED *> $null
  schtasks.exe /Run /TN $taskName *> $null
  Write-Host "Removing Jcode Lite Free app files."
  Write-Host "Kept sessions, memory, credentials, and config in:"
  Write-Host "  $(Join-Path $root 'home')"
  Write-Host "A future install will resume from the same data."
}

Write-Host "Open a new terminal window for the PATH change to take effect."
