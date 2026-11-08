# Destructive installer checks: run as administrator in a fresh, disposable Windows x64 VM only.
param([Parameter(Mandatory=$true)][string]$Older, [Parameter(Mandatory=$true)][string]$Newer)
$ErrorActionPreference = 'Stop'
if ($env:TORRO_INSTALLER_SMOKE -ne '1') { throw 'Set TORRO_INSTALLER_SMOKE=1 only in a disposable test VM' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator required' }
$root = Join-Path $env:ProgramFiles 'TorroWorkplace'
if (Test-Path $root) { throw 'Existing installation; refusing destructive tests' }
$apps = (Get-Content (Join-Path $PSScriptRoot '../apps.json') -Raw | ConvertFrom-Json).apps
function Run-Installer($File) {
    $process = Start-Process $File -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed: $File ($($process.ExitCode))" }
}
function Setup-For($Folder, $Program) {
    $files = @(Get-ChildItem $Folder -Filter "$Program-*-x86_64-pc-windows-msvc-setup.exe")
    if ($files.Count -ne 1) { throw "Expected exactly one Setup for $Program in $Folder" }
    return $files[0].FullName
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
    }
}
$data = Join-Path $env:LOCALAPPDATA 'torrotasks/installer-smoke-preserve.txt'
New-Item (Split-Path $data) -ItemType Directory -Force | Out-Null
Set-Content $data 'personal data'
# Both dependency removal orders; every app is installed, reinstalled and upgraded in each pass.
foreach ($reverse in @($false, $true)) {
    foreach ($app in $apps) { Run-Installer (Setup-For $Older $app.program) }
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
Write-Output 'PASS: all apps, repeat, upgrade, downgrade refusal, both removal orders and shared-file lifetime'
