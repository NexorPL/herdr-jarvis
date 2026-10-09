# herdr [[build]] step (windows): fetch the release binary, verify SHA-256, else build from source.
# Windows PowerShell 5.1 compatible.
$ErrorActionPreference = 'Stop'
$Repo = 'NexorPL/herdr-jarvis'
$Root = Join-Path $PSScriptRoot '..'
$Out = Join-Path $Root 'target\release\jarvis.exe'

# A running collector keeps jarvis.exe locked; Windows still allows renaming it, so move it aside
# and remove older leftovers that are no longer running. cargo links target\release\deps\jarvis.exe
# and hard-links target\release\jarvis.exe to it, so both names point at the locked file.
function Move-Aside {
    foreach ($exe in @($Out, (Join-Path (Split-Path -Parent $Out) 'deps\jarvis.exe'))) {
        if (Test-Path $exe) {
            Rename-Item -Path $exe -NewName ('jarvis.exe.old-' + [guid]::NewGuid().ToString('N'))
        }
        Get-ChildItem -Path (Split-Path -Parent $exe) -Filter 'jarvis.exe.old-*' -ErrorAction SilentlyContinue |
            ForEach-Object { Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue }
    }
}

function Build-FromSource([string]$Reason) {
    [Console]::Error.WriteLine("jarvis: $Reason - building from source")
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        [Console]::Error.WriteLine('jarvis: cargo not found; install Rust from https://rustup.rs')
        exit 1
    }
    Move-Aside
    Push-Location $Root
    & cargo build --release
    $code = $LASTEXITCODE
    Pop-Location
    exit $code
}

function Get-File([string]$Url, [string]$Dest) {
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing -ErrorAction Stop
        return $true
    } catch {
        return $false
    }
}

if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { Build-FromSource "no prebuilt binary for Windows/$($env:PROCESSOR_ARCHITECTURE)" }
$match = Select-String -Path (Join-Path $Root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $match) { Build-FromSource 'could not read version from Cargo.toml' }
$Version = $match.Matches[0].Groups[1].Value
$Asset = 'jarvis-x86_64-pc-windows-msvc.exe'
$Base = "https://github.com/$Repo/releases/download/v$Version"
$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ('jarvis-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Tmp -Force | Out-Null
try {
    if (-not (Get-File "$Base/$Asset" (Join-Path $Tmp $Asset))) { Build-FromSource "no release asset $Asset for v$Version" }
    if (-not (Get-File "$Base/SHA256SUMS" (Join-Path $Tmp 'SHA256SUMS'))) { Build-FromSource "no SHA256SUMS for v$Version" }
    $pattern = '^([0-9a-fA-F]{64}) [ *]' + [regex]::Escape($Asset) + '$'
    $line = Get-Content (Join-Path $Tmp 'SHA256SUMS') | Where-Object { $_ -match $pattern } | Select-Object -First 1
    if (-not $line) { Build-FromSource "no checksum listed for $Asset" }
    $expected = ([regex]::Match($line, $pattern)).Groups[1].Value.ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 -Path (Join-Path $Tmp $Asset)).Hash.ToLowerInvariant()
    if ($expected -ne $actual) { Build-FromSource "checksum mismatch for $Asset" }
    New-Item -ItemType Directory -Path (Split-Path -Parent $Out) -Force | Out-Null
    Move-Aside
    Move-Item -Force (Join-Path $Tmp $Asset) $Out
    Write-Output "jarvis: installed prebuilt v$Version (x86_64-pc-windows-msvc)"
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
