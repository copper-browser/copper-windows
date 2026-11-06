<#
Copper updating itself, end to end, on a Windows machine (CI: update-e2e.yml).

    ./tools/update-e2e.ps1 -A A.exe -B B.exe -Out e2e

A and B are two copper.exe builds, B with the higher version. The script
installs A, serves B from a local feed (COPPER_UPDATE_FEED; plain http is
accepted only with that override) and runs the installed copy's update
self-test (crates/copper-win/src/update_selftest.rs) three times:

  1. refuse          the feed's SHA-256 doesn't match: nothing is staged and A stays.
  2. restart         A downloads, verifies and stages B, runs Restart to Update;
                     B is installed, opens by itself and reports "updated to B".
  3. update-on-quit  A again (reinstalled by hand); it stages B and just quits;
                     B is installed at quit, and reports it when opened.

After each install the registry's DisplayVersion and the installed exe's
SHA-256 must be the expected build's. Every wait is bounded.
#>
param(
    [Parameter(Mandatory)] [string] $A,
    [Parameter(Mandatory)] [string] $B,
    [Parameter(Mandatory)] [string] $Out,
    [int] $Port = 8765
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$A = (Resolve-Path -LiteralPath $A).Path
$B = (Resolve-Path -LiteralPath $B).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).Path
$installed = Join-Path $env:LOCALAPPDATA 'Programs\Copper\copper.exe'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Copper'
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }

function Fail([string] $why) { Write-Host "::error::$why"; throw $why }
function Sha([string] $path) { (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant() }
function InstalledVersion { (Get-ItemProperty -Path $uninstallKey -ErrorAction SilentlyContinue).DisplayVersion }

function VersionOf([string] $exe) {
    $file = Join-Path $Out "version-$([guid]::NewGuid()).txt"
    $p = Start-Process -FilePath $exe -ArgumentList '--version' -RedirectStandardOutput $file -PassThru -WindowStyle Hidden
    if (-not $p.WaitForExit(30000)) { $p.Kill(); Fail "$exe --version hung" }
    ((Get-Content -LiteralPath $file -Raw) -replace '^copper\s+', '').Trim()
}

# Start `exe args` and wait for that process (not its children) to end.
function RunAndWait([string] $exe, [string[]] $arguments, [int] $seconds, [string] $what) {
    $p = if ($arguments) { Start-Process -FilePath $exe -ArgumentList $arguments -PassThru } else { Start-Process -FilePath $exe -PassThru }
    $null = $p.Handle  # without it ExitCode reads empty once the process is gone
    if (-not $p.WaitForExit($seconds * 1000)) {
        try { $p.Kill() } catch {}
        Fail "$what didn't finish in $seconds s"
    }
    $p.ExitCode
}

function WaitFor([scriptblock] $test, [int] $seconds, [string] $what) {
    $until = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $until) {
        if (& $test) { return }
        Start-Sleep -Milliseconds 500
    }
    Fail "timed out after $seconds s waiting for: $what"
}

function NoCopperRunning {
    -not (Get-Process -Name copper -ErrorAction SilentlyContinue)
}

# The self-test's report: present, finished and without failures.
function Report([string] $file, [string] $what) {
    WaitFor { (Test-Path -LiteralPath $file) -and ((Get-Content -LiteralPath $file -Raw) -match 'update selftest: \d+ failure') } 300 "$what report $file"
    $text = Get-Content -LiteralPath $file -Raw
    Write-Host "--- $what ($file)`n$text"
    if ($text -notmatch 'update selftest: 0 failure\(s\)') { Fail "$what reported failures" }
}

function Expect([string] $version, [string] $exe, [string] $what) {
    WaitFor { (InstalledVersion) -eq $version -and (Test-Path -LiteralPath $installed) -and ((Sha $installed) -eq (Sha $exe)) } 180 "$what (installed $version)"
    Write-Host "OK $what`: installed $version, $installed is that build (sha256 $(Sha $installed))"
}

function Install([string] $exe) {
    $code = RunAndWait $exe @('--install', '--silent') 300 "installing $exe"
    if ($code -ne 0) { Fail "installing $exe exited $code" }
}

function Selftest([string] $mode, [string] $feed, [string] $dir) {
    $env:COPPER_UPDATE_FEED = "http://127.0.0.1:$Port/$feed"
    $env:COPPER_SELFTEST = $mode
    $env:COPPER_SELFTEST_OUT = $dir
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    RunAndWait $installed @() 420 "the $mode self-test" | Out-Null
}

$versionA = VersionOf $A
$versionB = VersionOf $B
Write-Host "A = $versionA ($(Sha $A)), B = $versionB ($(Sha $B)), arch $arch"
if (-not $versionA -or -not $versionB -or $versionA -eq $versionB) { Fail 'A and B must be two different versions' }

# The feed: B as the release's installer, under a manifest that matches it
# and one whose SHA-256 doesn't.
$feedDir = Join-Path $Out 'feed'
New-Item -ItemType Directory -Force -Path $feedDir | Out-Null
$asset = "CopperSetup-$arch.exe"
Copy-Item -LiteralPath $B -Destination (Join-Path $feedDir $asset)
function Manifest([string] $sha) {
    [ordered]@{
        schemaVersion = 1; product = 'copper'; platform = 'windows'
        version = $versionB; releaseId = "v$versionB"; commit = 'e2e'
        publishedAt = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        notes = 'The update end-to-end test.'
        assets = @([ordered]@{
                kind = 'installer'; arch = $arch; name = $asset
                size = (Get-Item -LiteralPath $B).Length; sha256 = $sha
                url = "http://127.0.0.1:$Port/$asset"
            })
    } | ConvertTo-Json -Depth 5
}
Manifest (Sha $B) | Set-Content -LiteralPath (Join-Path $feedDir 'good.json') -Encoding utf8NoBOM
Manifest (Sha $A) | Set-Content -LiteralPath (Join-Path $feedDir 'bad.json') -Encoding utf8NoBOM

$server = Start-Process -FilePath python -ArgumentList @('-m', 'http.server', "$Port", '--bind', '127.0.0.1', '--directory', $feedDir) -PassThru -WindowStyle Hidden
try {
    WaitFor { try { (Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/good.json").StatusCode -eq 200 } catch { $false } } 30 'the feed server'

    Install $A
    Expect $versionA $A 'A installed'

    # 1. A download that doesn't match the feed is refused; A stays.
    Selftest 'update-refuse' 'bad.json' (Join-Path $Out 'refuse')
    Report (Join-Path $Out 'refuse\update-a.txt') 'refuse'
    Start-Sleep -Seconds 5
    Expect $versionA $A 'nothing installed after the refusal'

    # 2. Restart to Update: B is installed and opens by itself.
    Selftest 'update' 'good.json' (Join-Path $Out 'restart')
    Report (Join-Path $Out 'restart\update-a.txt') 'restart, before'
    Expect $versionB $B 'Restart to Update installed B'
    Report (Join-Path $Out 'restart\update-b.txt') 'restart, after (B opened by the installer)'
    WaitFor { NoCopperRunning } 120 'B to quit'

    # 3. Installed when Copper quits, nothing opened; B says so next start.
    Install $A
    Expect $versionA $A 'A reinstalled'
    Selftest 'update-on-quit' 'good.json' (Join-Path $Out 'quit')
    Report (Join-Path $Out 'quit\update-a.txt') 'on quit, before'
    Expect $versionB $B 'quitting installed B'
    WaitFor { NoCopperRunning } 120 'the installer and its warm-up to finish'
    Selftest 'update-on-quit' 'good.json' (Join-Path $Out 'quit')
    Report (Join-Path $Out 'quit\update-b.txt') 'on quit, after'

    Write-Host 'update end-to-end: all three rounds passed'
}
finally {
    if ($server -and -not $server.HasExited) { $server.Kill() }
    Remove-Item Env:COPPER_SELFTEST, Env:COPPER_UPDATE_FEED, Env:COPPER_SELFTEST_OUT -ErrorAction SilentlyContinue
    $logs = Join-Path $Out 'logs'
    New-Item -ItemType Directory -Force -Path $logs | Out-Null
    $copy = @(
        (Join-Path $env:TEMP 'CopperSetup.log'),
        (Join-Path $env:LOCALAPPDATA 'Copper\Updates\state.json'),
        (Join-Path $env:LOCALAPPDATA 'Copper\Data\logs\copper.log'),
        (Join-Path $env:LOCALAPPDATA 'Copper\Data\logs\copper.previous.log')
    )
    foreach ($f in $copy) {
        if (Test-Path -LiteralPath $f) { Copy-Item -LiteralPath $f -Destination $logs }
    }
}
