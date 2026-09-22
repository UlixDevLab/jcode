$ErrorActionPreference = "Stop"
$bundledNode = Join-Path $PSScriptRoot "runtime\node"
if (Test-Path (Join-Path $bundledNode "node.exe")) { $env:Path = "$bundledNode;$env:Path" }
Write-Host "Checking for Jcode Lite Free updates..."
$manifestUrl = if ($env:JCODE_LITE_FREE_WINDOWS_MANIFEST_URL) { $env:JCODE_LITE_FREE_WINDOWS_MANIFEST_URL } else { "https://files.legrin-tech.net/jcode-lite-free/windows/manifest.json" }
$manifest = (Invoke-WebRequest -Uri $manifestUrl -UseBasicParsing).Content | ConvertFrom-Json
foreach ($field in "version", "url", "sha256", "size_bytes") {
  if (!$manifest.$field) { throw "Manifest is missing '$field'." }
}
$release = Get-Content (Join-Path $PSScriptRoot "release.json") -Raw | ConvertFrom-Json
$manifestFile = Join-Path ([IO.Path]::GetTempPath()) ("jcode-lite-free-manifest-" + [guid]::NewGuid() + ".json")
try {
  $manifest | ConvertTo-Json -Depth 10 | Set-Content $manifestFile -Encoding UTF8
  $channel = if ($env:JCODE_LITE_FREE_UPDATE_CHANNEL) { $env:JCODE_LITE_FREE_UPDATE_CHANNEL } else { $release.channel }
  $verifyArgs = @((Join-Path $PSScriptRoot "verify-update-manifest.mjs"), $manifestFile, (Join-Path $PSScriptRoot "update-trust.json"), "--product", "jcode-lite-free", "--platform", "windows", "--channel", $channel, "--current-version", $release.version)
  if ($env:JCODE_LITE_FREE_ALLOW_DOWNGRADE -eq "1") { $verifyArgs += "--allow-downgrade" }
  & node @verifyArgs
  if ($LASTEXITCODE -ne 0) { throw "Update manifest verification failed." }
} finally {
  Remove-Item $manifestFile -Force -ErrorAction SilentlyContinue
}
if ($manifest.version -eq $release.version -and $env:JCODE_LITE_FREE_FORCE_REINSTALL -ne "1") {
  Write-Host "Jcode Lite Free $($manifest.version) is already current."
  exit 0
}
if (!$manifest.url.StartsWith("https://files.legrin-tech.net/jcode-lite-free/", [StringComparison]::Ordinal)) {
  throw "Manifest package URL is not trusted."
}
Write-Host "Latest version: $($manifest.version)"
$temp = Join-Path ([IO.Path]::GetTempPath()) ("jcode-lite-free-update-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temp | Out-Null
try {
  $package = Join-Path $temp "package.zip"
  Write-Host "Downloading package..."
  Invoke-WebRequest -Uri $manifest.url -OutFile $package -UseBasicParsing
  Write-Host "Verifying package integrity..."
  if ((Get-FileHash $package -Algorithm SHA256).Hash -ne $manifest.sha256.ToUpperInvariant()) {
    throw "Downloaded package hash mismatch."
  }
  if ((Get-Item $package).Length -ne [int64]$manifest.size_bytes) { throw "Downloaded package size mismatch." }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $archive = [IO.Compression.ZipFile]::OpenRead($package)
  try {
    foreach ($entry in $archive.Entries) {
      $name = $entry.FullName
      $parts = $name -split "/", -1
      if (!$name -or $name.Contains("\") -or [IO.Path]::IsPathRooted($name) -or
          $name.Contains(":") -or $parts -contains "" -or $parts -contains "." -or $parts -contains "..") {
        throw "Downloaded package contains an unsafe archive path."
      }
    }
  } finally {
    $archive.Dispose()
  }
  Write-Host "Extracting package..."
  $stage = Join-Path $temp "stage"
  Expand-Archive -Path $package -DestinationPath $stage
  $allowlist = Join-Path $stage "allowlist.txt"
  if (!(Test-Path $allowlist -PathType Leaf)) { throw "Downloaded package has no allowlist." }
  $actualFiles = Get-ChildItem $stage -File -Recurse | ForEach-Object {
    $_.FullName.Substring($stage.Length + 1).Replace("\", "/")
  } | Sort-Object
  $allowedFiles = Get-Content $allowlist | Sort-Object
  if (Compare-Object $actualFiles $allowedFiles) {
    throw "Downloaded package contents do not match its allowlist."
  }
  & (Join-Path $stage "install.ps1")
} finally {
  Remove-Item $temp -Recurse -Force -ErrorAction SilentlyContinue
}
