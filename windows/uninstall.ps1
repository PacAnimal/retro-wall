# Remove retro-wall's live wallpaper for the user running it, on Windows.
#
#   powershell -ExecutionPolicy Bypass -File windows\uninstall.ps1
#
# Run it as yourself. Stops retro-wall, removes the scheduled task \retro-wall\retro-wall, puts the
# wallpaper settings back as they were before the first install (your wallpaper), and removes
# %LOCALAPPDATA%\retro-wall: the program, the themes, the pictures it drew, its cache and its log.
# Your options in %APPDATA%\retro, if you made any, stay.
# Windows PowerShell 5.1 or later.

$ErrorActionPreference = 'Stop'
$dest = Join-Path $env:LOCALAPPDATA 'retro-wall'
$exe = Join-Path $dest 'retro-wall.exe'
$saved = Join-Path $dest 'desktop-before'
$taskPath = '\retro-wall\'
$taskName = 'retro-wall'

function Say($text) { Write-Host "retro-wall: $text" }

Set-Location $env:TEMP   # not inside what is removed
if (Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction SilentlyContinue) {
    Stop-ScheduledTask -TaskPath $taskPath -TaskName $taskName
    Unregister-ScheduledTask -TaskPath $taskPath -TaskName $taskName -Confirm:$false
    Say "task $taskPath$taskName removed"
}
try {
    $root = (New-Object -ComObject Schedule.Service)
    $root.Connect()
    $root.GetFolder('\').DeleteFolder($taskPath.Trim('\'), 0)
} catch { }   # not there, or not empty

$running = Get-Process -Name 'retro-wall' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
if ($running) {
    $running | Stop-Process -Force
    $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
    Say 'stopped retro-wall'
}

if ((Test-Path $saved) -and (Test-Path $exe)) {
    $p = Start-Process -FilePath $exe -ArgumentList @('--desktop', 'restore', "`"$saved`"") -Wait -PassThru -WindowStyle Hidden
    if ($p.ExitCode -ne 0) { throw "the wallpaper settings were not put back (exit $($p.ExitCode)): $saved kept" }
    Say 'wallpaper settings put back'
}

if (Test-Path $dest) {
    Remove-Item -Recurse -Force $dest
    Say "removed $dest"
}
