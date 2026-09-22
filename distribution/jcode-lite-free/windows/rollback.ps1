$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$stateFile = Join-Path $root "state.json"
if (!(Test-Path $stateFile -PathType Leaf)) { throw "No Jcode Lite Free state record." }
$state = Get-Content $stateFile -Raw | ConvertFrom-Json
if (!$state.previous -or !(Test-Path (Join-Path $root "$($state.previous)\jcode-free.cmd"))) {
  throw "No rollback version is available."
}
@{ current = $state.previous; previous = $state.current } | ConvertTo-Json -Compress | Set-Content $stateFile -Encoding UTF8
Write-Host "Rolled back Jcode Lite Free from $($state.current) to $($state.previous)."
