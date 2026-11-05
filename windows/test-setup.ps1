# Test retro-wall-setup.exe on a Windows 10 or 11 machine that does not have retro-wall: install, change
# the settings, update, uninstall, and its window (through Win32 messages), each checked by what it left
# behind (files, the task, Installed apps, the Start menu, the wall's own log, the wallpaper put back).
# Screenshots of the desktop and of every page of the window go beside the results.
#
#   powershell -ExecutionPolicy Bypass -File windows\test-setup.ps1 -Setup retro-wall-setup.exe -Out results
#   ... -UiOnly -Tag 150      the window's pages only, named for the display's scale (after a re-logon at it)
#
# Run it as the user, in their desktop session, not as an administrator: that is how the setup program
# is run. It removes retro-wall and its settings at the end. results\results.txt: PASS or FAIL, a line
# each; DONE last. The exit code is the number of failures.

param(
    [Parameter(Mandatory)][string]$Setup,
    [Parameter(Mandatory)][string]$Out,
    [switch]$UiOnly,
    [string]$Tag = '100'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Setup = (Resolve-Path $Setup).Path
$results = Join-Path $Out 'results.txt'
if (-not $UiOnly) { Set-Content -Path $results -Value @() }
$script:failures = 0
$install = Join-Path $env:LOCALAPPDATA 'retro-wall'
$settings = Join-Path $env:APPDATA 'retro'
$installed = Join-Path $install 'retro-wall-setup.exe'
$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\retro-wall settings.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\retro-wall'

function Note($line) { Add-Content -Path $results -Value $line; Write-Output $line }
function Check($name, [scriptblock]$test) {
    try {
        $r = & $test
        if ($r -is [string] -and $r) { throw $r }
        if ($r -eq $false) { throw 'false' }
        Note "PASS $name"
    } catch {
        $script:failures++
        Note "FAIL ${name}: $($_.Exception.Message)"
    }
}

# A program run to its end; its exit code and output (a GUI program's too, through Start-Process).
function Run($exe, [string[]]$arguments) {
    $o = Join-Path $env:TEMP "rw-out-$PID.txt"
    $e = Join-Path $env:TEMP "rw-err-$PID.txt"
    $quoted = $arguments | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }
    $p = if ($quoted) { Start-Process -FilePath $exe -ArgumentList $quoted -Wait -PassThru -NoNewWindow -RedirectStandardOutput $o -RedirectStandardError $e }
         else { Start-Process -FilePath $exe -Wait -PassThru -NoNewWindow -RedirectStandardOutput $o -RedirectStandardError $e }
    [pscustomobject]@{ Code = $p.ExitCode; Text = ((Get-Content $o -Raw) + (Get-Content $e -Raw)) }
}

function Shot($name) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

function Get-Settings {
    $r = Run $installed @('--get')
    $h = @{}
    foreach ($l in ($r.Text -split "`r?`n")) { if ($l -match '^([a-z-]+)=(.*)$') { $h[$Matches[1]] = $Matches[2] } }
    $h
}

function Wall-Process { Get-Process -Name 'retro-wall' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $install 'retro-wall.exe') } }

# The wall's log says the theme it draws, a line a monitor: "DISPLAY1: Winter at 1920x1080+0+0, ..."
# animating, "DISPLAY1: Winter, OpenGL ...: the still picture, not animating" without a graphics driver
# (a virtual machine's). A theme it could not load ("theme: ...: the default theme ... instead") fails.
function Wait-Theme($shown, $seconds = 60) {
    $log = Join-Path $install 'retro-wall.log'
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $log) {
            $text = Get-Content $log -Raw
            if ($text -match 'instead') { return "the wall refused its theme: $(($text -split "`n" | Select-String 'instead') -join ' | ')" }
            if ($text -match ": $([regex]::Escape($shown))( at |, )") { return $true }
        }
        Start-Sleep 2
    }
    "the log never named '$shown': $(if (Test-Path $log) { (Get-Content $log -Tail 5) -join ' | ' } else { 'no log' })"
}

# The task starts the wall a moment after it is asked to.
function Wait-Wall($seconds = 30) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) { if (Wall-Process) { return $true }; Start-Sleep 1 }
    $false
}

# A window test leaves no window behind, passing or failing: the next would find the setup program's
# window already open (it allows one at a time).
function Close-Setup { Get-Process -Name 'retro-wall-setup' -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }

function Wallpaper { (Get-ItemProperty 'HKCU:\Control Panel\Desktop').WallPaper }

# --- the window, through Win32 ----------------------------------------------------------------------
# Driven as a user's clicks reach it: each control found by its fixed id (GetDlgItem), buttons clicked
# (BM_CLICK, posted: one may open a message box), a list's item chosen with the notification a click
# sends (the pages too: the navigation is a list). Windows PowerShell's UI Automation client does
# not see a program's current (version 6) controls whole, so it is not used.
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class W32 {
    public delegate bool Enum(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Enum f, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, Enum f, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, string l);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    public static string Text(IntPtr h) { var s = new StringBuilder(2048); GetWindowText(h, s, s.Capacity); return s.ToString(); }
    public static IntPtr Find(string title) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { if (IsWindowVisible(h) && Text(h) == title) { found = h; return false; } return true; }, IntPtr.Zero);
        return found;
    }
    public static string AllText(IntPtr p) {
        var sb = new StringBuilder();
        EnumChildWindows(p, (h, l) => { sb.Append(Text(h)).Append(' '); return true; }, IntPtr.Zero);
        return sb.ToString();
    }
}
'@
# the screenshots in real pixels at any scale, not the scaled-down screen of a program unaware of it
[W32]::SetProcessDPIAware() | Out-Null
$BM_CLICK = 0x00F5; $BM_GETCHECK = 0x00F0; $WM_COMMAND = 0x0111; $WM_SETTEXT = 0x000C
$LB_FINDSTRINGEXACT = 0x01A2; $LB_SETCURSEL = 0x0186; $LB_GETCOUNT = 0x018B; $LBN_SELCHANGE = 1

function Find-Window($title, $seconds = 30) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $w = [W32]::Find($title)
        if ($w -ne [IntPtr]::Zero) { return $w }
        Start-Sleep -Milliseconds 500
    }
    throw "no window '$title'"
}
function Control($window, [int]$id, $seconds = 20) {
    $deadline = (Get-Date).AddSeconds($seconds)
    do {
        $c = [W32]::GetDlgItem($window, $id)
        if ($c -ne [IntPtr]::Zero) { return $c }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "no control $id"
}
function Invoke($window, [int]$id) { [W32]::PostMessage((Control $window $id), $BM_CLICK, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 400 }
function Toggle($window, [int]$id) { Invoke $window $id }
function Checked($window, [int]$id) { [int][W32]::SendMessage((Control $window $id), $BM_GETCHECK, [IntPtr]::Zero, [IntPtr]::Zero) -eq 1 }
function Pick($window, [int]$id, $item) {
    $c = Control $window $id
    $i = [int][W32]::SendMessage($c, $LB_FINDSTRINGEXACT, [IntPtr](-1), $item)
    if ($i -lt 0) { throw "no item '$item' in $id" }
    [W32]::SendMessage($c, $LB_SETCURSEL, [IntPtr]$i, [IntPtr]::Zero) | Out-Null
    [W32]::SendMessage([W32]::GetParent($c), $WM_COMMAND, [IntPtr](($LBN_SELCHANGE -shl 16) -bor $id), $c) | Out-Null
    Start-Sleep -Milliseconds 400
}
function Set-Text($window, [int]$id, $text) { [W32]::SendMessage((Control $window $id), $WM_SETTEXT, [IntPtr]::Zero, $text) | Out-Null }
function Tab($window, $name) {
    $c = Control $window 100
    $i = $pages.IndexOf($name)
    [W32]::SendMessage($c, $LB_SETCURSEL, [IntPtr]$i, [IntPtr]::Zero) | Out-Null
    [W32]::SendMessage([W32]::GetParent($c), $WM_COMMAND, [IntPtr](($LBN_SELCHANGE -shl 16) -bor 100), $c) | Out-Null
    Start-Sleep -Milliseconds 700
}
function Status($window) { [W32]::Text((Control $window 301)) }
function Wait-Status($window, $text, $seconds = 90) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $s = Status $window
        if ($s -like "*$text*") { return $true }
        Start-Sleep 1
    }
    "the status line said '$(Status $window)', not '$text'"
}
# The window is ready when it lists its themes and no longer says it is getting ready.
function Wait-Ready($window, $seconds = 90) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if ([int][W32]::SendMessage((Control $window 201), $LB_GETCOUNT, [IntPtr]::Zero, [IntPtr]::Zero) -gt 0 -and (Status $window) -notlike '*ready*') { return }
        Start-Sleep 1
    }
    throw "the window was not ready in $seconds s: '$(Status $window)'"
}
# A message box: its text, and one of its buttons pressed (OK 1, Cancel 2, Yes 6, No 7).
function Answer($title, [int]$button, $seconds = 30) {
    $box = Find-Window $title $seconds
    $text = [W32]::AllText($box)
    # as a click sends it: BN_CLICKED (0) with the button's own window; a box with OK alone may give
    # its button the id of Cancel
    $c = [W32]::GetDlgItem($box, $button)
    if ($c -eq [IntPtr]::Zero -and $button -eq 1) { $button = 2; $c = [W32]::GetDlgItem($box, 2) }
    [W32]::PostMessage($box, $WM_COMMAND, [IntPtr]$button, $c) | Out-Null
    Start-Sleep -Milliseconds 700
    $text
}
$pages = 'Theme', 'Screens', 'Instruments', 'Pausing', 'Advanced'

if ($UiOnly) {
    # the settings window at this scale: every page drawn
    Check "the window opens at $Tag% and shows every page" {
        if (-not (Test-Path (Join-Path $install 'retro-wall.exe'))) { $r = Run $Setup @('--install'); if ($r.Code) { return "install for the UI pass: $($r.Text)" } }
        $p = Start-Process -FilePath $installed -PassThru
        $w = Find-Window 'retro-wall settings'
        Wait-Ready $w
        Start-Sleep 3
        foreach ($page in $pages) { Tab $w $page; Shot "settings-$Tag-$page" }
        Invoke $w 304
        Start-Sleep 2
        if (-not $p.HasExited) { $p | Stop-Process -Force }
    }
    Note 'DONE'
    exit $script:failures
}

# --- the command line ------------------------------------------------------------------------------
$wallpaperBefore = Wallpaper
Note "INFO the desktop's wallpaper before: $wallpaperBefore"

Check 'nothing installed to begin with' {
    $r = Run $Setup @('--status')
    if ($r.Text -notmatch 'installed=no') { "status: $($r.Text)" }
}
Check 'an unknown key is refused, nothing done' {
    $r = Run $Setup @('--install', 'colour=blue')
    if ($r.Code -ne 2) { "exit $($r.Code): $($r.Text)" } elseif (Test-Path $install) { 'it installed anyway' }
}
# an install by windows\install.ps1 kept its options in the task: an update over it takes them up
schtasks.exe /Create /TN '\retro-wall\retro-wall' /TR 'C:\Windows\System32\cmd.exe --theme winter --battery' /SC ONCE /ST 23:59 /F | Out-Null
Check 'an install over an old one takes up the options its task had' {
    $r = Run $Setup @('--install')
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    $s = Get-Settings
    if ($s.theme -ne 'winter' -or $s.battery -ne 'on') { return "theme $($s.theme), battery $($s.battery)" }
    Wait-Theme 'Winter'
}
Check 'first settings, given on the command line' {
    $r = Run $installed @('--set', 'theme=blocky', 'hand=on', 'battery=on', 'rdp=3389')
    if ($r.Code) { "exit $($r.Code): $($r.Text)" }
}
Check 'the program, the themes, the setup program and the saved wallpaper are in place' {
    foreach ($f in 'retro-wall.exe', 'retro-wall-setup.exe', 'desktop-before', 'themes\blocky\theme.toml', 'themes\phosphor\theme.toml') {
        if (-not (Test-Path (Join-Path $install $f))) { return "missing $f" }
    }
}
Check 'the task starts the wall with no options, at logon, kept running' {
    $x = [xml](schtasks.exe /Query /TN '\retro-wall\retro-wall' /XML | Out-String)
    if (-not $x.Task.Triggers.LogonTrigger) { return 'no logon trigger' }
    if ($x.Task.Actions.Exec.Arguments) { return "arguments: $($x.Task.Actions.Exec.Arguments)" }
    if ($x.Task.Settings.MultipleInstancesPolicy -ne 'IgnoreNew') { return 'not IgnoreNew' }
    # least privilege is the default: Task Scheduler leaves it out of the XML
    if ($x.Task.Principals.Principal.RunLevel -and $x.Task.Principals.Principal.RunLevel -ne 'LeastPrivilege') { return "run level $($x.Task.Principals.Principal.RunLevel)" }
}
Check 'it is in Installed apps, with an uninstaller' {
    $k = Get-ItemProperty $uninstallKey
    if ($k.DisplayName -ne 'retro-wall') { return "name $($k.DisplayName)" }
    if ($k.UninstallString -notmatch 'retro-wall-setup\.exe" --uninstall$') { return "uninstall: $($k.UninstallString)" }
    if (-not $k.DisplayVersion) { return 'no version' }
}
Check 'the Start menu has the settings' { Test-Path $lnk }
Check 'the wall runs, drawing blocky' { if (-not (Wait-Wall)) { 'not running' } else { Wait-Theme 'Blocky' } }
Check 'the settings are in files, the extras through a theme of their own' {
    $conf = Get-Content (Join-Path $settings 'retro-wall.conf') -Raw
    if ($conf -notmatch '(?m)^windows\s+--theme setup-blocky .*--battery') { return "conf: $conf" }
    $t = Get-Content (Join-Path $settings 'themes\setup-blocky\theme.toml') -Raw
    if ($t -notmatch 'extends = "blocky"' -or $t -notmatch 'hand = true') { return "theme: $t" }
    $s = Get-Settings
    if ($s.theme -ne 'blocky' -or $s.hand -ne 'on' -or $s.battery -ne 'on' -or $s.rdp -ne '3389') { return "get: $(($s.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' ')" }
}
Start-Sleep 8
Shot 'installed-blocky'

$before = (Wall-Process).Id
Check 'changing the settings restarts the wall with them' {
    $r = Run $installed @('--set', 'theme=winter', 'hide=swap,deck', 'title=none')
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    $now = (Wall-Process).Id
    if (-not $now -or $now -eq $before) { return "the wall was not restarted ($before -> $now)" }
    Wait-Theme 'Winter'
}
Check 'the instruments hidden and the hostname nowhere, in retro.toml' {
    $t = Get-Content (Join-Path $settings 'retro.toml') -Raw
    if ($t -notmatch '\[monitors\]' -or $t -notmatch 'title = "none"') { return "monitors: $t" }
    if ($t -notmatch '\[\[group\]\]' -or $t -match '"swap"' -or $t -match '"deck"' -or $t -notmatch '"bat"') { return "groups: $t" }
    if (Test-Path (Join-Path $settings 'themes\setup-blocky')) { return "blocky's own theme left behind" }
}
Check 'the user''s own lines in retro.toml are kept' {
    Add-Content (Join-Path $settings 'retro.toml') "`n# my own note`n[source.users]`ncommand = `"echo 3`"`nevery = 30.0`n"
    $r = Run $installed @('--set', 'graph=none')
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    $t = Get-Content (Join-Path $settings 'retro.toml') -Raw
    if ($t -notmatch '# my own note' -or $t -notmatch '\[source\.users\]' -or $t -notmatch 'graph = "none"') { "retro.toml: $t" }
}
Check 'a wrong value is refused, and nothing changes' {
    $r = Run $installed @('--set', 'rdp=lots')
    if ($r.Code -ne 2) { return "exit $($r.Code)" }
    if ((Get-Settings).rdp -ne '3389') { 'changed anyway' }
}
Start-Sleep 6
Shot 'changed-winter'

Check 'an update while the settings window is open' {
    $p = Start-Process -FilePath $installed -PassThru
    Find-Window 'retro-wall settings' | Out-Null
    $r = Run $Setup @('--install')
    $p | Stop-Process -Force
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    if (-not (Wait-Wall)) { return 'the wall is not running' }
    Wait-Theme 'Winter'
}
Check 'an update keeps the settings' {
    $r = Run $Setup @('--install')
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    if ((Get-Settings).theme -ne 'winter') { return 'the theme was not kept' }
    Wait-Theme 'Winter'
}

function Uninstall-Log { $l = Join-Path $env:TEMP 'retro-wall-uninstall.log'; if (Test-Path $l) { (Get-Content $l -Tail 4) -join ' | ' } else { 'no helper log' } }
Check 'uninstall, keeping the settings' {
    $r = Run $installed @('--uninstall', '--quiet', '--keep-settings')
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    Start-Sleep 8
    if (Test-Path $install) { return "left: $((Get-ChildItem $install -Recurse | Select-Object -First 5) -join ', '); helper: $(Uninstall-Log)" }
    cmd.exe /c 'schtasks.exe /Query /TN \retro-wall\retro-wall >nul 2>&1'
    if ($LASTEXITCODE -eq 0) { return 'the task is still there' }
    if (Test-Path $uninstallKey) { return 'still in Installed apps' }
    if (Test-Path $lnk) { return 'the shortcut is still there' }
    if (Wall-Process) { return 'the wall still runs' }
    if (-not (Test-Path (Join-Path $settings 'retro-wall.conf'))) { return 'the settings went too' }
}
Check 'the desktop''s own wallpaper is back' { if ((Wallpaper) -ne $wallpaperBefore) { "now '$(Wallpaper)', was '$wallpaperBefore'" } }

Check 'installing again takes up the settings kept' {
    $r = Run $Setup @('--install')
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    Wait-Theme 'Winter'
}
Check 'the uninstall''s removal at the next sign-in leaves an install made since' {
    $k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'
    $cmd = (Get-ItemProperty $k -ErrorAction SilentlyContinue).'retro-wall-uninstall'
    if (-not $cmd) { return 'no RunOnce entry from the uninstall' }
    # as the sign-in would run it, now
    Start-Process -FilePath cmd.exe -ArgumentList ($cmd -replace '^cmd\.exe /c ', '/c ') -Wait -WindowStyle Hidden
    Remove-ItemProperty $k -Name 'retro-wall-uninstall'
    if (-not (Test-Path (Join-Path $install 'retro-wall.exe'))) { return "the new install went: $cmd" }
}
Check 'uninstall with its settings: what the setup program wrote goes, the user''s own lines stay' {
    $r = Run $installed @('--uninstall', '--quiet')
    if ($r.Code) { return "exit $($r.Code): $($r.Text)" }
    Start-Sleep 8
    if (Test-Path $install) { return "the install is still there; helper: $(Uninstall-Log)" }
    if (Test-Path (Join-Path $settings 'retro-wall.conf')) { return 'retro-wall.conf is still there' }
    if (Test-Path (Join-Path $settings 'themes\setup-*')) { return 'its own themes are still there' }
    $t = Get-Content (Join-Path $settings 'retro.toml') -Raw
    if ($t -notmatch '# my own note' -or $t -match '\[monitors\]' -or $t -match '\[\[group\]\]') { return "retro.toml: $t" }
    if ((Wallpaper) -ne $wallpaperBefore) { return "wallpaper now '$(Wallpaper)'" }
    Remove-Item -Recurse -Force $settings   # a clean start for the window's tests
}

# --- the window ------------------------------------------------------------------------------------
Close-Setup
Check 'a first run opens the install window, and Install installs with what was chosen' {
    $p = Start-Process -FilePath $Setup -PassThru
    $w = Find-Window 'Install retro-wall'
    Wait-Ready $w
    foreach ($page in $pages) { Tab $w $page; Shot "install-$Tag-$page" }
    Tab $w 'Theme'
    Pick $w 201 'Aurora'
    Tab $w 'Instruments'
    Toggle $w 223   # swap off
    Shot 'install-choices'
    Invoke $w 302
    $done = Wait-Status $w 'Installed'
    Shot 'install-done'
    if ($done -ne $true) { return $done }
    if ((Get-Settings).theme -ne 'aurora') { return "theme $((Get-Settings).theme)" }
    if ((Get-Settings).hide -ne 'swap') { return "hidden $((Get-Settings).hide)" }
    $t = Wait-Theme 'Aurora'
    if ([W32]::Find('retro-wall settings') -eq [IntPtr]::Zero) { return 'the window did not stay open as the settings' }
    Shot 'install-then-settings'
    Invoke $w 304
    Start-Sleep 2
    if (-not $p.HasExited) { $p | Stop-Process -Force }
    $t
}
Close-Setup
Check 'the settings window: Apply keeps a change and restarts the wall' {
    $p = Start-Process -FilePath $installed -PassThru
    $w = Find-Window 'retro-wall settings'
    Wait-Ready $w
    Tab $w 'Theme'
    Pick $w 201 'Neon rain'
    Tab $w 'Pausing'
    Toggle $w 233   # pause while the screen is off
    Invoke $w 303
    $done = Wait-Status $w 'Saved'
    Shot 'settings-applied'
    if ($done -ne $true) { return $done }
    $s = Get-Settings
    if ($s.theme -ne 'neon_rain' -or $s.'screen-off' -ne 'on') { return "theme $($s.theme), screen-off $($s.'screen-off')" }
    foreach ($page in $pages) { Tab $w $page; Shot "settings-$Tag-$page" }
    Invoke $w 304
    Start-Sleep 2
    if (-not $p.HasExited) { $p | Stop-Process -Force }
    Wait-Theme 'Neon rain'
}
Close-Setup
Check 'a wrong port is refused in the window, with a message, and nothing is kept' {
    $p = Start-Process -FilePath $installed -PassThru
    $w = Find-Window 'retro-wall settings'
    Wait-Ready $w
    $before = (Get-Settings).rdp
    Tab $w 'Pausing'
    if (-not (Checked $w 234)) { Toggle $w 234 }
    Set-Text $w 235 '0'
    Invoke $w 303
    Shot 'settings-refused'
    $text = Answer 'retro-wall' 1 10
    Start-Sleep 1
    $p | Stop-Process -Force
    if ($text -notmatch 'port') { return "the message: $text" }
    if ((Get-Settings).rdp -ne $before) { return "kept anyway: rdp $((Get-Settings).rdp), was $before" }
}
Close-Setup
Check 'Uninstall in the window removes it all, the program itself once it has closed' {
    $p = Start-Process -FilePath $installed -PassThru
    $w = Find-Window 'retro-wall settings'
    Wait-Ready $w
    Tab $w 'Advanced'
    Invoke $w 246
    Answer 'Uninstall retro-wall' 1 | Out-Null    # Remove retro-wall? OK
    Answer 'Uninstall retro-wall' 7 | Out-Null    # Keep your settings? No
    $box = Find-Window 'retro-wall' 60
    Shot 'uninstalled-message'
    Start-Sleep 5   # the message read for a while: the program is still running, its folder must wait
    Answer 'retro-wall' 1 | Out-Null
    Start-Sleep 2
    Shot 'uninstalled-after-ok'
    $after = "box $([W32]::Find('retro-wall') -ne [IntPtr]::Zero), window $([W32]::Find('retro-wall settings') -ne [IntPtr]::Zero)"
    $p | Wait-Process -Timeout 20 -ErrorAction SilentlyContinue
    Start-Sleep 6
    if (Test-Path $install) {
        $running = (Get-CimInstance Win32_Process -Filter "Name LIKE 'retro-wall%'" | ForEach-Object { "$($_.ProcessId) $($_.CommandLine)" }) -join ' || '
        return "left: $((Get-ChildItem $install -Recurse | Select-Object -First 5) -join ', '); setup exited: $($p.HasExited) ($after); running: $running; helper: $(Uninstall-Log)"
    }
    if (Test-Path (Join-Path $settings 'retro-wall.conf')) { return 'the settings are still there' }
}
Close-Setup
Check 'the wallpaper is back at the very end' { if ((Wallpaper) -ne $wallpaperBefore) { "now '$(Wallpaper)'" } }

Note "INFO failures: $script:failures"
Note 'DONE'
exit $script:failures
