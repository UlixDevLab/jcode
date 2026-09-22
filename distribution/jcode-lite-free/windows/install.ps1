$ErrorActionPreference = "Stop"
$bundledNode = Join-Path $PSScriptRoot "runtime\node"
if (Test-Path (Join-Path $bundledNode "node.exe")) { $env:Path = "$bundledNode;$env:Path" }

# Right-clicking install.ps1 and choosing "Run with PowerShell" launches
# powershell.exe with -NonInteractive, which redirects stdin and closes the
# console window the moment the script exits, before anyone can read any
# output. Detect that here (instead of failing deep inside install or the
# jcode.exe TTY check) so the message is visible.
if ([Console]::IsInputRedirected -and $env:JCODE_LITE_FREE_ALLOW_NONINTERACTIVE -ne "1") {
  Write-Host ""
  Write-Host "This installer needs an interactive console window." -ForegroundColor Yellow
  Write-Host "Right-clicking install.ps1 and choosing 'Run with PowerShell' does not"
  Write-Host "provide one and the window closes immediately after running."
  Write-Host ""
  Write-Host "Instead, double-click install.cmd in this folder, or open a Command"
  Write-Host "Prompt / PowerShell window yourself and run install.ps1 from there."
  Write-Host ""
  if ($env:JCODE_LITE_FREE_NO_PAUSE -ne "1") {
    try { Read-Host "Press Enter to close this window" | Out-Null } catch {}
  }
  exit 1
}

if (![Environment]::Is64BitOperatingSystem -or [Environment]::OSVersion.Version.Major -lt 10) {
  throw "Jcode Lite Free requires 64-bit Windows 10 or newer."
}
$node = Get-Command node -ErrorAction SilentlyContinue
if (!$node) { throw "Install Node.js 20 or newer, then retry." }
$release = Get-Content (Join-Path $PSScriptRoot "release.json") -Raw | ConvertFrom-Json
$nodeMajor = [int]((& node -p "process.versions.node.split('.')[0]").Trim())
if ($nodeMajor -lt $release.minimum_node_major) { throw "Install Node.js $($release.minimum_node_major) or newer, then retry." }

$root = Join-Path $env:LOCALAPPDATA "LeGrin\JcodeLiteFree"
$target = Join-Path $root $release.version
$stageTarget = Join-Path $root (".staging-" + $release.version + "-" + [guid]::NewGuid())
$homeDir = Join-Path $root "home"
$stateFile = Join-Path $root "state.json"
$previous = ""
if (Test-Path $stateFile -PathType Leaf) {
  $state = Get-Content $stateFile -Raw | ConvertFrom-Json
  $previous = if ($state.current -ne $release.version) { $state.current } else { $state.previous }
}
New-Item -ItemType Directory -Force -Path $stageTarget, $homeDir | Out-Null

Write-Host "Installing Jcode Lite Free $($release.version)..."

# The package has thousands of small files (vendored MCP node_modules).
# Looping Copy-Item per allowlist entry takes 30-60+ seconds with no visible
# progress, which looks hung. robocopy /MT copies the whole tree in a few
# seconds and ships in every Windows 10+ install.
try {
  $temp = [IO.Path]::GetTempPath()
  $robocopyLog = Join-Path $temp ("jcode-lite-free-robocopy-" + [guid]::NewGuid() + ".log")
  & robocopy.exe $PSScriptRoot $stageTarget /E /MT:8 /NFL /NDL /NJH /NJS /NP /R:2 /W:1 /LOG:$robocopyLog | Out-Null
  $robocopyExit = $LASTEXITCODE
  Remove-Item $robocopyLog -ErrorAction SilentlyContinue
  # robocopy exit codes 0-7 all indicate success (bit flags for files
  # copied/skipped/mismatched); 8+ means a real failure.
  if ($robocopyExit -ge 8) {
    throw "Copying package files failed (robocopy exit code $robocopyExit)."
  }
  foreach ($_ in (Get-Content (Join-Path $PSScriptRoot "allowlist.txt") | Where-Object { $_ })) {
    if (!(Test-Path (Join-Path $stageTarget $_) -PathType Leaf)) {
      throw "Package file missing after copy: $_"
    }
  }
  $env:JCODE_HOME = $homeDir
  & (Join-Path $stageTarget "bin\jcode.exe") --no-update version | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "Candidate Jcode Lite Free binary failed its startup health check." }
  if (Test-Path $target) { Remove-Item $target -Recurse -Force }
  Move-Item $stageTarget $target
} finally {
  if (Test-Path $stageTarget) { Remove-Item $stageTarget -Recurse -Force }
}

$configFile = Join-Path $homeDir "config.toml"
if (!(Test-Path $configFile -PathType Leaf)) {
  Copy-Item (Join-Path $target "preset\config.toml") $configFile -Force
}
& node (Join-Path $target "preset\install-assets.mjs") (Join-Path $target "preset") $homeDir
if ($LASTEXITCODE -ne 0) { throw "Managed asset migration failed. See the installer output before retrying." }

$stateNew = "$stateFile.new.$PID"
@{ current = $release.version; previous = $previous } | ConvertTo-Json -Compress | Set-Content $stateNew -Encoding UTF8
Move-Item $stateNew $stateFile -Force

$cliDir = Join-Path $root "bin"
New-Item -ItemType Directory -Force -Path $cliDir | Out-Null
@'
$ErrorActionPreference = "Stop"
$root = Join-Path $env:LOCALAPPDATA "LeGrin\JcodeLiteFree"
$stateFile = Join-Path $root "state.json"
if ($args.Count -gt 0 -and $args[0] -eq "remove") {
  $removeScript = $null
  if (Test-Path $stateFile -PathType Leaf) {
    $state = Get-Content $stateFile -Raw | ConvertFrom-Json
    $candidate = Join-Path $root (Join-Path $state.current "remove.ps1")
    if (Test-Path $candidate -PathType Leaf) { $removeScript = $candidate }
  }
  if (!$removeScript) { throw "Jcode Lite Free is not installed." }
  # remove.ps1 deletes $root, which contains this process's own working
  # directory ($root\bin). Move out first so PowerShell/cmd.exe do not lose
  # their CWD mid-deletion.
  Set-Location $env:USERPROFILE
  & $removeScript @($args | Select-Object -Skip 1)
  exit $LASTEXITCODE
}
if (!(Test-Path $stateFile -PathType Leaf)) { throw "Jcode Lite Free is not installed." }
$state = Get-Content $stateFile -Raw | ConvertFrom-Json
if ($args.Count -gt 0 -and $args[0] -eq "update") {
  & (Join-Path $root (Join-Path $state.current "update.ps1")) @($args | Select-Object -Skip 1)
  exit $LASTEXITCODE
}
$launcher = Join-Path $root (Join-Path $state.current "jcode-free.ps1")
if (!(Test-Path $launcher -PathType Leaf)) { throw "The current Jcode Lite Free launcher is missing." }
& $launcher @args
exit $LASTEXITCODE
'@ | Set-Content (Join-Path $cliDir "jcodef.ps1") -Encoding UTF8
@'
@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0jcodef.ps1" %*
exit /b %ERRORLEVEL%
'@ | Set-Content (Join-Path $cliDir "jcodef.cmd") -Encoding ASCII

if ($env:JCODE_LITE_FREE_INSTALL_NO_PATH_UPDATE -ne "1") {
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $pathEntries = @($userPath -split ';' | Where-Object { $_ })
  $alreadyPresent = $pathEntries | Where-Object { [String]::Equals($_.TrimEnd('\'), $cliDir.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) }
  if (!$alreadyPresent) {
    $newPath = if ($userPath) { $userPath.TrimEnd(';') + ';' + $cliDir } else { $cliDir }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
  }
}

Write-Host "Installed Jcode Lite Free $($release.version)."
Write-Host "Installed terminal command: jcodef (available in new terminal sessions)."
Write-Host "No provider is bundled. Jcode will guide you through provider setup on first use."
if ($env:JCODE_LITE_FREE_INSTALL_NO_LAUNCH -eq "1") {
  Write-Host "Start with: $(Join-Path $target 'jcode-free.cmd')"
  exit 0
}
Write-Host "Launching Jcode Lite Free..."
Set-Location $env:USERPROFILE
& (Join-Path $target "jcode-free.ps1") @args
exit $LASTEXITCODE
