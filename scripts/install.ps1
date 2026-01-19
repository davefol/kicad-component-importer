param(
  [string]$Version = $env:KCI_VERSION,
  [string]$InstallDir = $env:KCI_INSTALL_DIR
)

$repo = "american-sensing/kicad-component-importer"
$binName = "kicad-component-importer.exe"

if (-not $Version) { $Version = "latest" }
if (-not $InstallDir) { $InstallDir = Join-Path $env:USERPROFILE "bin" }

$arch = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture
switch ($arch) {
  "X64" { $artifact = "kicad-component-importer-windows-x86_64.zip" }
  default { throw "Unsupported arch: $arch" }
}

if ($Version -eq "latest") {
  $base = "https://github.com/$repo/releases/latest/download"
} else {
  $base = "https://github.com/$repo/releases/download/$Version"
}

$url = "$base/$artifact"

$temp = New-TemporaryFile
Remove-Item $temp
$tempZip = "$temp.zip"

Invoke-WebRequest -Uri $url -OutFile $tempZip
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Expand-Archive -Path $tempZip -DestinationPath $InstallDir -Force
Remove-Item $tempZip -Force

$binPath = Join-Path $InstallDir $binName
if ($env:PATH -notlike "*$InstallDir*") {
  Write-Output "Add $InstallDir to PATH to use $binName"
}

Write-Output "Installed $binName to $binPath"
