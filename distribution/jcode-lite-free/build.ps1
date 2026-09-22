param(
  [Parameter(Mandatory = $true)][ValidateSet("macos", "windows")][string]$Platform,
  [Parameter(Mandatory = $true)][string]$Binary,
  [string]$Output,
  [switch]$SkipMcpNpmCi
)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$sharedPreset = Join-Path $root "..\jcode-lite\common\preset"
$release = Get-Content (Join-Path $root "release.json") -Raw | ConvertFrom-Json
if (!(Test-Path $Binary -PathType Leaf)) { throw "Jcode binary not found: $Binary" }
if (!$Output) { $Output = Join-Path $root "jcode-lite-free-$Platform-$($release.version).zip" }
$stage = Join-Path ([IO.Path]::GetTempPath()) ("jcode-lite-free-build-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $stage | Out-Null
try {
  New-Item -ItemType Directory -Force -Path (Join-Path $stage "preset") | Out-Null
  Copy-Item (Join-Path $root "preset\config.toml") (Join-Path $stage "preset\config.toml")
  Copy-Item (Join-Path $root "preset\swarm-prompt.md") (Join-Path $stage "preset\swarm-prompt.md")
  Copy-Item (Join-Path $sharedPreset "skills") (Join-Path $stage "preset\skills") -Recurse
  Copy-Item (Join-Path $sharedPreset "roles") (Join-Path $stage "preset\roles") -Recurse
  Copy-Item (Join-Path $sharedPreset "knowledge-os") (Join-Path $stage "preset\knowledge-os") -Recurse
  Copy-Item (Join-Path $sharedPreset "mcp") (Join-Path $stage "preset\mcp") -Recurse
  # Free does NOT inherit shared/preset/swarm-prompt.md because the shared
  # prompt pins private Lite model routes. Free ships a provider-neutral
  # override copied above from $root/preset/swarm-prompt.md.
  Copy-Item (Join-Path $root "$Platform\*") $stage -Recurse -Force
  Copy-Item (Join-Path $root "..\jcode-lite\common\verify-update-manifest.mjs") (Join-Path $stage "verify-update-manifest.mjs")
  Copy-Item (Join-Path $root "..\jcode-lite\common\update-trust.json") (Join-Path $stage "update-trust.json")
  Copy-Item (Join-Path $root "..\jcode-lite\common\check-update.mjs") (Join-Path $stage "check-update.mjs")
  Copy-Item (Join-Path $root "release.json") (Join-Path $stage "release.json")
  Copy-Item (Join-Path $root "lite-manifest.json") (Join-Path $stage "lite-manifest.json")
  # Plain-language instructions at the archive root. The recipient may have
  # no terminal instincts and nobody to ask, so this must be the first thing
  # visible after extracting, not buried under preset/.
  Copy-Item (Join-Path $root "preset\START-HERE.md") (Join-Path $stage "START-HERE.md")
  New-Item -ItemType Directory -Force -Path (Join-Path $stage "bin") | Out-Null
  $binaryName = if ($Platform -eq "windows") { "jcode.exe" } else { "jcode" }
  Copy-Item $Binary (Join-Path $stage "bin\$binaryName")

  if (-not $SkipMcpNpmCi) {
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw "Node.js is required to vendor MCP packages." }
    if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { throw "npm is required to vendor MCP packages (use -SkipMcpNpmCi if node_modules is pre-staged)." }
    Write-Host "Vendoring MCP Node packages via npm ci..."
    $mcpDir = Join-Path $stage "preset\mcp"
    Push-Location $mcpDir
    try {
      # PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 keeps the package deterministic:
      # the Playwright wrapper insists on a system Chrome/Edge and we never
      # want npm to download a browser binary into node_modules.
      $env:PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1"
      & npm ci --omit=dev --ignore-scripts --no-audit --no-fund
      if ($LASTEXITCODE -ne 0) { throw "npm ci failed: $LASTEXITCODE" }
    } finally {
      Pop-Location
    }
  }

  # Credential/routing scan (mirrors build.sh's grep guard). Free must ship
  # with no Stables credential or routing material anywhere.
  $forbidden = @('llmr_[A-Za-z0-9]{44}', 'STABLES_API_KEY', 'router\.legrin-tech\.net')
  $binaryLeafNames = @("jcode", "jcode.exe")
  $textFiles = Get-ChildItem $stage -Recurse -File | Where-Object { $_.Name -notin $binaryLeafNames }
  foreach ($pattern in $forbidden) {
    foreach ($f in $textFiles) {
      try {
        $content = [IO.File]::ReadAllText($f.FullName)
      } catch { continue }
      if ($content -match $pattern) {
        throw "Credential or Stables routing material found in free package: $($f.FullName) matches $pattern"
      }
    }
  }

  $files = Get-ChildItem $stage -Recurse -File | ForEach-Object { $_.FullName.Substring($stage.Length + 1).Replace("\", "/") } | Sort-Object
  Set-Content (Join-Path $stage "allowlist.txt") ($files + "allowlist.txt" | Sort-Object -Unique) -Encoding ASCII
  if (Test-Path $Output) { Remove-Item $Output -Force }
  Add-Type -AssemblyName System.IO.Compression
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  # ZipFile.CreateFromDirectory on Windows PowerShell 5.1 (.NET Framework)
  # writes backslash-separated entry names, which violates the zip spec (entry
  # names must use '/') and breaks every zip reader that checks for path
  # traversal or safe separators (see tests/verify-package.py's "unsafe
  # archive path" guard, and Python's zipfile module). Build entries manually
  # with explicit forward-slash names instead of trusting the convenience API.
  $allEntries = Get-ChildItem $stage -Recurse -File | Sort-Object FullName
  $zipStream = [IO.File]::Open($Output, [IO.FileMode]::Create)
  try {
    $archive = New-Object IO.Compression.ZipArchive($zipStream, [IO.Compression.ZipArchiveMode]::Create)
    try {
      foreach ($entryFile in $allEntries) {
        $entryName = $entryFile.FullName.Substring($stage.Length + 1).Replace("\", "/")
        $entry = $archive.CreateEntry($entryName, [IO.Compression.CompressionLevel]::Optimal)
        $entryStream = $entry.Open()
        try {
          $fileStream = [IO.File]::OpenRead($entryFile.FullName)
          try { $fileStream.CopyTo($entryStream) } finally { $fileStream.Dispose() }
        } finally {
          $entryStream.Dispose()
        }
      }
    } finally {
      $archive.Dispose()
    }
  } finally {
    $zipStream.Dispose()
  }
  Write-Host "Built $Output"
  Get-FileHash $Output -Algorithm SHA256

  $python = Get-Command python -ErrorAction SilentlyContinue
  if (-not $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
  if (-not $python) { throw "python3 is required to run tests/verify-package.py" }
  & $python.Source (Join-Path $root "tests\verify-package.py") $Output
  if ($LASTEXITCODE -ne 0) { throw "verify-package.py failed: $LASTEXITCODE" }
} finally {
  Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}
