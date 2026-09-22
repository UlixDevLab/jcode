$ErrorActionPreference = "Stop"
$bundledNode = Join-Path $PSScriptRoot "runtime\node"
if (Test-Path (Join-Path $bundledNode "node.exe")) { $env:Path = "$bundledNode;$env:Path" }
$target = $PSScriptRoot
$installRoot = Join-Path $env:LOCALAPPDATA "LeGrin\JcodeLiteFree"
$targetParent = [IO.Path]::GetFullPath((Split-Path $target -Parent)).TrimEnd('\')
$installedRoot = [IO.Path]::GetFullPath($installRoot).TrimEnd('\')
if (![String]::Equals($targetParent, $installedRoot, [StringComparison]::OrdinalIgnoreCase)) {
  & (Join-Path $target "install.ps1") @args
  exit $LASTEXITCODE
}
$root = Split-Path $target -Parent
$homeDir = Join-Path $root "home"

$node = Get-Command node -ErrorAction SilentlyContinue
if (!$node) { throw "Jcode Lite Free requires Node.js 20 or newer for Mage." }
$nodeMajor = [int]((& node -p "process.versions.node.split('.')[0]").Trim())
if ($nodeMajor -lt 20) { throw "Jcode Lite Free requires Node.js 20 or newer." }

if ($env:JCODE_LITE_FREE_UPDATE_CHECK -ne "0") {
  $release = Get-Content (Join-Path $target "release.json") -Raw | ConvertFrom-Json
  $checkUrl = if ($env:JCODE_LITE_FREE_UPDATE_CHECK_URL) { $env:JCODE_LITE_FREE_UPDATE_CHECK_URL } else { "https://files.legrin-tech.net/jcode-lite-free/windows/manifest.json" }
  $trust = if ($env:JCODE_LITE_FREE_UPDATE_TRUST) { $env:JCODE_LITE_FREE_UPDATE_TRUST } else { Join-Path $target "update-trust.json" }
  $interval = if ($env:JCODE_LITE_FREE_UPDATE_CHECK_INTERVAL_HOURS) { $env:JCODE_LITE_FREE_UPDATE_CHECK_INTERVAL_HOURS } else { "24" }
  & node (Join-Path $target "check-update.mjs") --manifest-url $checkUrl --trust $trust --verifier (Join-Path $target "verify-update-manifest.mjs") --product jcode-lite-free --platform windows --channel $release.channel --current-version $release.version --state (Join-Path $homeDir "update-check.json") --interval-hours $interval --name "Jcode Lite Free" --update-command (Join-Path $target "update.ps1")
}

$env:JCODE_HOME = $homeDir
$env:JCODE_RUNTIME_DIR = Join-Path $root "runtime"
New-Item -ItemType Directory -Force $env:JCODE_RUNTIME_DIR | Out-Null
$env:KNOWLEDGE_OS_BIN = Join-Path $target "preset\knowledge-os\bin\knowledge-os-lite.mjs"
$env:JCODE_NO_TELEMETRY = "1"
Remove-Item Env:JCODE_PROVIDER_ALLOWLIST -ErrorAction SilentlyContinue

# jcode's TUI needs a real console. Right-clicking a .ps1 file and choosing
# "Run with PowerShell" launches powershell.exe with -NonInteractive, which
# redirects stdin. jcode.exe then fails fast with a TTY error and the
# console window closes before anyone can read it, looking like "nothing
# happens". Detect that case here and explain it instead of letting the
# window vanish.
if ([Console]::IsInputRedirected -and $args.Count -eq 0) {
  Write-Host ""
  Write-Host "Jcode Lite Free needs an interactive terminal window." -ForegroundColor Yellow
  Write-Host "This looks like it was started via 'Run with PowerShell', which does not"
  Write-Host "provide one. Instead:"
  Write-Host "  1. Open a Command Prompt or PowerShell window yourself, or double-click"
  Write-Host "     jcode-free.cmd in the installed folder."
  Write-Host "  2. Then run: jcodef"
  Write-Host ""
  if ($env:JCODE_LITE_FREE_NO_PAUSE -ne "1") {
    try { Read-Host "Press Enter to close this window" | Out-Null } catch {}
  }
  exit 1
}

& (Join-Path $target "bin\jcode.exe") --no-update @args
exit $LASTEXITCODE
