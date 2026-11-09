# Destructive installer checks: run as administrator in a fresh, disposable Windows x64 VM only.
param([Parameter(Mandatory=$true)][string]$Older, [Parameter(Mandatory=$true)][string]$Newer)
$ErrorActionPreference = 'Stop'
if ($env:TORRO_INSTALLER_SMOKE -ne '1') { throw 'Set TORRO_INSTALLER_SMOKE=1 only in a disposable test VM' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator required' }
$root = Join-Path $env:ProgramFiles 'TorroWorkplace'
if (Test-Path $root) { throw 'Existing installation; refusing destructive tests' }
$apps = (Get-Content (Join-Path $PSScriptRoot '../apps.json') -Raw | ConvertFrom-Json).apps
function Run-Installer($File, $Extra = @()) {
    $process = Start-Process $File -ArgumentList (@('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') + $Extra) -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed: $File ($($process.ExitCode))" }
}
function Setup-For($Folder, $Program) {
    $files = @(Get-ChildItem $Folder -Filter "$Program-*-x86_64-pc-windows-msvc-setup.exe")
    if ($files.Count -ne 1) { throw "Expected exactly one Setup for $Program in $Folder" }
    return $files[0].FullName
}
function Check-Version($Program) {
    $prefix = Join-Path $env:TEMP ('torro-version-' + [guid]::NewGuid())
    $stdout = "$prefix.out"
    $stderr = "$prefix.err"
    try {
        $process = Start-Process (Join-Path $root "$Program.exe") -ArgumentList @('--version') -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $null = $process.Handle
        if (-not $process.WaitForExit(30000)) {
            $process.Kill()
            throw "Version check timed out: $Program (possible loader dialog or unintended GUI startup)"
        }
        $process.Refresh()
        if ($process.ExitCode -ne 0) { throw "Installed program failed to load: $Program ($($process.ExitCode))" }
        $version = (Get-Content (Join-Path $root "$Program.version") -Raw).Trim()
        if ((Get-Content $stdout -Raw).Trim() -ne "$Program $version") { throw "Installed binary and version sidecar disagree: $Program" }
    } finally {
        Remove-Item -LiteralPath @($stdout, $stderr) -Force -ErrorAction SilentlyContinue
    }
}
function Check-Payload($Installed) {
    $expected = @{}
    foreach ($app in $apps | Where-Object { $Installed -contains $_.id }) {
        foreach ($program in @($app.program, 'torroworkplace') + $app.requires) { $expected[$program] = $true }
    }
    foreach ($app in $apps) {
        $program = $app.program
        foreach ($file in @("$program.exe", "$program.version", "$program.exe.torro-package.json")) {
            if ((Test-Path (Join-Path $root $file)) -ne $expected.ContainsKey($program)) { throw "Wrong shared-file lifetime: $file" }
        }
        if ($expected.ContainsKey($program)) { Check-Version $program }
    }
}
$data = Join-Path $env:LOCALAPPDATA 'torrotasks/installer-smoke-preserve.txt'
New-Item (Split-Path $data) -ItemType Directory -Force | Out-Null
Set-Content $data 'personal data'
$custom = Join-Path $env:TEMP ('torro-custom-' + [guid]::NewGuid())
$process = Start-Process (Setup-For $Newer 'torroworkplace') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=$custom") -Wait -PassThru
if ($process.ExitCode -eq 0 -or (Test-Path $custom) -or (Test-Path $root)) { throw 'Custom destination was accepted or changed files' }
$process = Start-Process (Setup-For $Newer 'torroworkplace') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCANCEL') -Wait -PassThru
if ($process.ExitCode -eq 0 -or (Test-Path $root)) { throw 'Disabling safety cancellation was accepted' }
# Both dependency removal orders; every app is installed, reinstalled and upgraded in each pass.
foreach ($reverse in @($false, $true)) {
    foreach ($app in $apps) { Run-Installer (Setup-For $Older $app.program) }
    Check-Payload @($apps | ForEach-Object { $_.id })
    $beforeLocked = @(Get-ChildItem $root -Recurse -File | Get-FileHash | Sort-Object Path | ForEach-Object { "$($_.Path):$($_.Hash)" })
    $lock = [IO.File]::Open((Join-Path $root 'torroworkplace.exe'), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    $unsafeInf = Join-Path $env:TEMP ('torro-nocancel-' + [guid]::NewGuid() + '.inf')
    try {
        $process = Start-Process (Setup-For $Newer 'torrocalendar') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCLOSEAPPLICATIONS') -Wait -PassThru
        if ($process.ExitCode -eq 0) { throw 'Upgrade with unresolved shared-service lock was accepted' }
        Set-Content -LiteralPath $unsafeInf -Value @('[Setup]', 'NoCancel=yes') -Encoding ASCII
        $process = Start-Process (Setup-For $Newer 'torrocalendar') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCLOSEAPPLICATIONS', "/LOADINF=`"$unsafeInf`"") -Wait -PassThru
        if ($process.ExitCode -eq 0) { throw 'INF disabled native safety cancellation during a locked upgrade' }
    } finally {
        $lock.Dispose()
        Remove-Item -LiteralPath $unsafeInf -Force -ErrorAction SilentlyContinue
    }
    $afterLocked = @(Get-ChildItem $root -Recurse -File | Get-FileHash | Sort-Object Path | ForEach-Object { "$($_.Path):$($_.Hash)" })
    if (Compare-Object $beforeLocked $afterLocked) { throw 'Locked upgrade changed payload or uninstall metadata files' }
    foreach ($app in $apps) { Run-Installer (Setup-For $Newer $app.program) }
    foreach ($app in $apps) { Run-Installer (Setup-For $Newer $app.program) }
    $remaining = @($apps | ForEach-Object { $_.id })
    Check-Payload $remaining
    # A stable shared service must reject an older RC before modifying any files.
    $before = (Get-FileHash (Join-Path $root 'torroworkplace.exe')).Hash
    $process = Start-Process (Setup-For $Older 'torroworkplace') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -PassThru
    if ($process.ExitCode -eq 0) { throw 'Downgrade incorrectly accepted' }
    if ($before -ne (Get-FileHash (Join-Path $root 'torroworkplace.exe')).Hash) { throw 'Downgrade changed service' }
    $order = @($apps)
    if ($reverse) { [array]::Reverse($order) }
    foreach ($app in $order) {
        $uninstaller = Join-Path $root "uninstall/$($app.id)/unins000.exe"
        Run-Installer $uninstaller
        $remaining = @($remaining | Where-Object { $_ -ne $app.id })
        Check-Payload $remaining
        if (-not (Test-Path $data)) { throw 'Personal data removed' }
    }
}
Write-Output 'PASS: fixed directory, unresolved-lock abort, unsafe INF rejection, all apps load with matching versions, repeat, upgrade, downgrade refusal, both removal orders and shared-file lifetime'
