# Install retro-wall's live wallpaper for the user running it, on Windows 10 and 11.
#
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1 [RETRO-WALL OPTIONS...]
#
# Run it as yourself (no administrator) from an unpacked release: retro-wall.exe, themes\ and windows\
# side by side. It:
#
#   1. stops a running retro-wall of this install
#   2. saves the desktop's wallpaper settings as they are (the first time only)
#   3. copies retro-wall.exe, the themes and the uninstaller to %LOCALAPPDATA%\retro-wall
#   4. registers the scheduled task \retro-wall\retro-wall: retro-wall with the options given, at logon
#      and kept running (the logon trigger repeats every 5 minutes; a repeat while it runs does nothing)
#   5. starts it
#
# Re-running it is how you update, or change the options. The log is
# %LOCALAPPDATA%\retro-wall\retro-wall.log. windows\uninstall.ps1 removes it all.
# Windows PowerShell 5.1 or later.

$ErrorActionPreference = 'Stop'
$options = $args
$here = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $env:LOCALAPPDATA 'retro-wall'
$exe = Join-Path $dest 'retro-wall.exe'
$saved = Join-Path $dest 'desktop-before'
$taskPath = '\retro-wall\'
$taskName = 'retro-wall'

function Say($text) { Write-Host "retro-wall: $text" }

# A program's run to its end, hidden; its exit code.
function Run($program, [string[]]$arguments) {
    $quoted = $arguments | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }
    $p = Start-Process -FilePath $program -ArgumentList $quoted -Wait -PassThru -WindowStyle Hidden
    $p.ExitCode
}

# Stop the task's retro-wall, and any other of this install.
function Stop-Wall {
    if (Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction SilentlyContinue) {
        Stop-ScheduledTask -TaskPath $taskPath -TaskName $taskName
    }
    $running = Get-Process -Name 'retro-wall' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
    if ($running) {
        $running | Stop-Process -Force
        $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
        Say 'stopped retro-wall'
    }
}

$source = Join-Path $here 'retro-wall.exe'
if (-not (Test-Path $source)) { throw "no retro-wall.exe beside windows\ (looked in $here)" }
if (-not (Test-Path (Join-Path $here 'themes'))) { throw "no themes\ beside windows\ (looked in $here)" }

Stop-Wall
New-Item -ItemType Directory -Force -Path $dest | Out-Null
if (-not (Test-Path $saved)) {
    $code = Run $source @('--desktop', 'save', $saved)
    if ($code -ne 0 -or -not (Test-Path $saved)) { throw "the wallpaper settings were not saved (exit $code): nothing installed" }
    Say "saved the wallpaper settings to $saved"
}

Copy-Item -Force $source $exe
$themes = Join-Path $dest 'themes'
if (Test-Path $themes) { Remove-Item -Recurse -Force $themes }
Copy-Item -Recurse (Join-Path $here 'themes') $themes
$scripts = Join-Path $dest 'windows'
New-Item -ItemType Directory -Force -Path $scripts | Out-Null
Copy-Item -Force (Join-Path $PSScriptRoot 'uninstall.ps1') $scripts
Say "installed in $dest"

$user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$quoted = $options | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }
$action = if ($quoted) {
    New-ScheduledTaskAction -Execute $exe -Argument ($quoted -join ' ') -WorkingDirectory $dest
} else {
    New-ScheduledTaskAction -Execute $exe -WorkingDirectory $dest
}
# Task Scheduler's restart on failure ignores exit codes: a logon trigger repeating every 5 minutes,
# and IgnoreNew while it runs, brings a dead retro-wall back. A logon trigger repeats only from a logon,
# so a second trigger repeats from now, for the session the install is run in.
$every = (New-ScheduledTaskTrigger -Once -At 00:00 -RepetitionInterval (New-TimeSpan -Minutes 5)).Repetition
$logon = New-ScheduledTaskTrigger -AtLogOn -User $user
$logon.Delay = 'PT5S'   # Explorer's desktop first
$logon.Repetition = $every
$now = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(5)
$now.Repetition = $every
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit 0 -Priority 6 `
    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
Register-ScheduledTask -TaskPath $taskPath -TaskName $taskName -Action $action -Trigger @($logon, $now) `
    -Principal $principal -Settings $settings -Force | Out-Null
Say "task $taskPath$taskName registered$(if ($quoted) { ': retro-wall ' + ($quoted -join ' ') })"

Start-ScheduledTask -TaskPath $taskPath -TaskName $taskName
Say 'started'
