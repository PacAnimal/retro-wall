//! What the setup program does to Windows: install, update and uninstall the wall for the user who runs
//! it (no administrator), keep its settings and restart it. The window (`gui`) and the command line
//! (`main`) both do it through here.
//!
//! An install is `%LOCALAPPDATA%\retro-wall`: the wall's program, its themes and this program (which
//! is the settings and the uninstaller); the scheduled task `\retro-wall\retro-wall`; an entry in
//! Settings, Installed apps; a Start menu shortcut to the settings. The wall's settings are in
//! `%APPDATA%\retro` (`retro_wall::setup::settings`).

use retro_wall::config;
use retro_wall::platform::windows::wide;
use retro_wall::setup::{payload, settings::Settings, task};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW,
    RegGetValueW, RegSetValueExW,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject,
};

/// Its name in Installed apps and the Start menu.
pub const PRODUCT: &str = "retro-wall";
/// This program's file name in the install.
pub const SETUP_EXE: &str = "retro-wall-setup.exe";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\retro-wall";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// The user's profile folders are set: without them every path here would be relative to wherever the
/// program was started, and an uninstall could remove the wrong folder.
pub fn need_profile() -> Result<(), String> {
    for v in ["LOCALAPPDATA", "APPDATA"] {
        if !std::env::var_os(v).map(PathBuf::from).is_some_and(|p| p.is_absolute()) {
            return Err(format!("%{v}% is not set: run this as yourself, from your own desktop"));
        }
    }
    Ok(())
}

/// Where the install is.
pub fn install_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("retro-wall")
}

/// The installed wall.
pub fn wall_exe() -> PathBuf {
    install_dir().join("retro-wall.exe")
}

/// The wall's settings directory.
pub fn settings_dir() -> PathBuf {
    config::config_dir()
}

/// What was there before the first install: the desktop's own wallpaper settings.
fn saved_desktop() -> PathBuf {
    install_dir().join("desktop-before")
}

fn start_menu_link() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join(r"Microsoft\Windows\Start Menu\Programs\retro-wall settings.lnk")
}

/// A step's report, for the window's status line and the command line's output.
pub type Say<'a> = &'a mut dyn FnMut(&str);

/// The version installed, from Installed apps; None if it is not installed.
pub fn installed_version() -> Option<String> {
    let key = wide(UNINSTALL_KEY);
    let name = wide("DisplayVersion");
    let mut buf = [0u16; 128];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: the key and value names are NUL-terminated; buf and len describe a buffer we own.
    let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len) };
    (rc == ERROR_SUCCESS && wall_exe().is_file()).then(|| retro_wall::platform::windows::unwide(&buf))
}

/// Whether this program is the installed one (the settings), not a download.
pub fn running_installed() -> bool {
    let me = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
    let there = install_dir().join(SETUP_EXE).canonicalize().ok();
    me.is_some() && me == there
}

/// The files to install: this program's payload, or, from an unpacked release or a build, the wall's
/// program and themes beside it.
fn files() -> Result<Vec<payload::File>, String> {
    let me = std::env::current_exe().map_err(|e| format!("this program's path: {e}"))?;
    let bytes = std::fs::read(&me).map_err(|e| format!("{}: {e}", me.display()))?;
    if let Some(files) = payload::read(&bytes) {
        return files;
    }
    let here = me.parent().unwrap_or(Path::new("."));
    let exe = here.join("retro-wall.exe");
    let themes = here.join("themes");
    if !exe.is_file() || !themes.is_dir() {
        if me.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("setup")) {
            return Err("this download is incomplete: download retro-wall's setup program again".into());
        }
        return Err(format!(
            "nothing to install: this program carries no payload, and there is no retro-wall.exe and themes\\ beside it in {}",
            here.display()
        ));
    }
    let mut out = vec![("retro-wall.exe".to_string(), std::fs::read(&exe).map_err(|e| format!("{}: {e}", exe.display()))?)];
    out.extend(payload::collect(&themes, "themes")?);
    Ok(out)
}

/// What this program carries, unpacked into `dir`: for a first run's previews, before it installs.
pub fn unpack_to(dir: &Path) -> Result<(), String> {
    for (path, data) in files()? {
        let to = dir.join(path.replace('/', "\\"));
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&to, data).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}

/// A program run to its end with no window; its exit code and its output.
fn run(program: &Path, args: &[&str]) -> Result<(i32, String), String> {
    let out = Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| format!("{}: {e}", program.display()))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    Ok((out.status.code().unwrap_or(-1), text.trim().to_string()))
}

fn schtasks(args: &[&str]) -> Result<(i32, String), String> {
    run(Path::new("schtasks.exe"), args)
}

/// Stop every running wall of this install: the task's, and any started by hand.
pub fn stop_wall(say: Say) {
    let _ = schtasks(&["/End", "/TN", task::NAME]);
    let target = wall_exe().canonicalize().ok();
    let mut stopped = 0;
    for pid in processes() {
        // SAFETY: OpenProcess with a pid from the snapshot; the handle is closed on every path.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
            if h.is_null() {
                continue;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let path = (QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0)
                .then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
            if path.and_then(|p| p.canonicalize().ok()).is_some_and(|p| Some(p) == target) && TerminateProcess(h, 0) != 0 {
                WaitForSingleObject(h, 10_000);
                stopped += 1;
            }
            CloseHandle(h);
        }
    }
    if stopped > 0 {
        say("stopped retro-wall");
    }
}

/// The ids of the processes named retro-wall.exe.
fn processes() -> Vec<u32> {
    let mut out = vec![];
    // SAFETY: a process snapshot walked with a PROCESSENTRY32W of the right size, then closed.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut e) != 0;
        while more {
            let name = retro_wall::platform::windows::unwide(&e.szExeFile);
            if name.eq_ignore_ascii_case("retro-wall.exe") {
                out.push(e.th32ProcessID);
            }
            more = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    out
}

/// Whether the wall of this install is running.
pub fn wall_running() -> bool {
    let target = wall_exe().canonicalize().ok();
    processes().into_iter().any(|pid| {
        // SAFETY: as in stop_wall, query only.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
            CloseHandle(h);
            ok && PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])).canonicalize().ok() == target
        }
    })
}

/// Start the wall through its task, as at logon.
pub fn start_wall(say: Say) -> Result<(), String> {
    match schtasks(&["/Run", "/TN", task::NAME])? {
        (0, _) => {
            say("retro-wall started");
            Ok(())
        }
        (code, out) => Err(format!("the task would not start (schtasks {code}): {out}")),
    }
}

/// Install, or update: everything but the user's settings, which are kept. `first` is settings to
/// write (a first install's, or the window's Install); without, a new install gets the defaults, or the
/// options an install by windows\install.ps1 gave its task. The wall is started; if an update fails
/// half way, the wall is started again as it was.
pub fn install(first: Option<&Settings>, say: Say) -> Result<(), String> {
    need_profile()?;
    let files = files()?;
    let was = installed_version().is_some() || wall_exe().is_file();
    let old_options = old_task_options();
    stop_wall(say);
    let r = put_in_place(&files, first, old_options, say);
    if r.is_err() && was {
        let _ = start_wall(&mut |_| {});
    }
    r?;
    start_wall(say)
}

fn put_in_place(files: &[payload::File], first: Option<&Settings>, old_options: Option<Vec<String>>, say: Say) -> Result<(), String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // the wall's program first, from the payload, to save the desktop's own wallpaper settings with
    let (_, exe) = files.iter().find(|(p, _)| p == "retro-wall.exe").ok_or("the payload has no retro-wall.exe")?;
    std::fs::write(wall_exe(), exe).map_err(|e| format!("{}: {e}", wall_exe().display()))?;
    if !saved_desktop().is_file() {
        let saved = saved_desktop();
        match run(&wall_exe(), &["--desktop", "save", &saved.to_string_lossy()])? {
            (0, _) if saved.is_file() => say("saved the wallpaper settings"),
            (code, out) => {
                let _ = std::fs::remove_file(wall_exe());
                return Err(format!("the wallpaper settings could not be saved (exit {code}: {out}): nothing installed"));
            }
        }
    }
    // the themes, whole: an update's set replaces the last
    let themes = dir.join("themes");
    if themes.exists() {
        std::fs::remove_dir_all(&themes).map_err(|e| format!("{}: {e}", themes.display()))?;
    }
    for (path, data) in files {
        if path == "retro-wall.exe" {
            continue;
        }
        let to = dir.join(path.replace('/', "\\"));
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&to, data).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    // what an install by windows\install.ps1 left: its uninstaller
    let _ = std::fs::remove_dir_all(dir.join("windows"));
    // this program, as the settings and the uninstaller (not when it is the installed one already). The
    // installed one may be running (its window open): a running program can be renamed, not replaced
    if !running_installed() {
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        let to = dir.join(SETUP_EXE);
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with("retro-wall-setup.old-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
        if to.exists() && std::fs::remove_file(&to).is_err() {
            let aside = dir.join(format!("retro-wall-setup.old-{}.exe", std::process::id()));
            std::fs::rename(&to, &aside).map_err(|e| format!("{}: {e}: close retro-wall's settings and try again", to.display()))?;
        }
        std::fs::copy(&me, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    say(&format!("installed in {}", dir.display()));
    // the settings: those given; else, for a new install, the old task's options or the defaults
    let sdir = settings_dir();
    let have = retro_wall::setup::settings::conf_args(&std::fs::read_to_string(sdir.join("retro-wall.conf")).unwrap_or_default());
    let fresh = !sdir.join("retro-wall.conf").is_file() || have.is_empty() && first.is_none() && old_options.is_some();
    match (first, fresh) {
        (Some(s), _) => {
            fitted(s).write(&sdir, &layout)?;
            say("settings written");
        }
        (None, true) => {
            let s = old_options.map(|o| Settings::from_args(&o)).unwrap_or_default();
            s.write(&sdir, &layout)?;
            say("settings written");
        }
        (None, false) => {}
    }
    register_task()?;
    say("task registered");
    register_app(&dir, files)?;
    if let Err(e) = link() {
        say(&format!("no Start menu shortcut ({e}): open retro-wall-setup.exe in {} for the settings", dir.display()));
    }
    Ok(())
}

/// The options the task of an install by windows\install.ps1 started the wall with; None if there is no
/// such task, or it had none.
fn old_task_options() -> Option<Vec<String>> {
    let (code, xml) = schtasks(&["/Query", "/TN", task::NAME, "/XML"]).ok()?;
    if code != 0 {
        return None;
    }
    let a = xml.find("<Arguments>")? + "<Arguments>".len();
    let b = a + xml[a..].find("</Arguments>")?;
    let args = xml[a..b].replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
    let words = retro_wall::setup::settings::words(&args);
    (!words.is_empty()).then_some(words)
}

/// The user, as Task Scheduler names them: DOMAIN\\name (an Entra ID account's too).
fn user() -> String {
    use windows_sys::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    // SAFETY: a buffer we own, its length in characters.
    if unsafe { GetUserNameExW(NameSamCompatible, buf.as_mut_ptr(), &mut len) } {
        return String::from_utf16_lossy(&buf[..len as usize]);
    }
    format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default())
}

fn register_task() -> Result<(), String> {
    let user = user();
    let now = local_now_plus_minutes(1);
    let xml = task::xml(&wall_exe().to_string_lossy(), &install_dir().to_string_lossy(), &user, &now);
    let file = std::env::temp_dir().join(format!("retro-wall-task-{}.xml", std::process::id()));
    std::fs::write(&file, task::utf16(&xml)).map_err(|e| format!("{}: {e}", file.display()))?;
    let r = schtasks(&["/Create", "/TN", task::NAME, "/XML", &file.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&file);
    match r? {
        (0, _) => Ok(()),
        (code, out) => Err(format!("the task could not be registered (schtasks {code}): {out}")),
    }
}

/// The local time `minutes` from now, as Task Scheduler writes it.
fn local_now_plus_minutes(minutes: u64) -> String {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime};
    // SAFETY: plain structs filled by the system.
    unsafe {
        let mut st: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut st);
        let mut ft: FILETIME = std::mem::zeroed();
        SystemTimeToFileTime(&st, &mut ft);
        let t = ((ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64) + minutes * 60 * 10_000_000;
        let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
        FileTimeToSystemTime(&ft, &mut st);
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
    }
}

/// The entry in Settings, Installed apps (per user: no administrator).
fn register_app(dir: &Path, files: &[payload::File]) -> Result<(), String> {
    let setup = dir.join(SETUP_EXE);
    let q = |p: &Path| format!("\"{}\"", p.display());
    let size_kb = (files.iter().map(|(_, d)| d.len() as u64).sum::<u64>() + std::fs::metadata(&setup).map_or(0, |m| m.len())) / 1024;
    let strings = [
        ("DisplayName", PRODUCT.to_string()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
        ("Publisher", "PacAnimal".to_string()),
        ("URLInfoAbout", "https://github.com/PacAnimal/retro-wall".to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("DisplayIcon", format!("{},0", setup.display())),
        ("UninstallString", format!("{} --uninstall", q(&setup))),
        ("QuietUninstallString", format!("{} --uninstall --quiet", q(&setup))),
        ("ModifyPath", q(&setup)),
    ];
    let dwords = [("NoRepair", 1u32), ("EstimatedSize", size_kb.min(u32::MAX as u64) as u32)];
    let key = wide(UNINSTALL_KEY);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: a key under HKCU created and closed here; values written from buffers we own.
    unsafe {
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE | KEY_READ,
            std::ptr::null(),
            &mut h,
            std::ptr::null_mut(),
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("Installed apps entry: error {rc}"));
        }
        let mut failed = None;
        for (name, v) in strings {
            let (n, v) = (wide(name), wide(&v));
            if RegSetValueExW(h, n.as_ptr(), 0, REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) != ERROR_SUCCESS {
                failed = Some(name);
            }
        }
        for (name, v) in dwords {
            let n = wide(name);
            if RegSetValueExW(h, n.as_ptr(), 0, REG_DWORD, (&v as *const u32).cast(), 4) != ERROR_SUCCESS {
                failed = Some(name);
            }
        }
        RegCloseKey(h);
        if let Some(name) = failed {
            return Err(format!("Installed apps entry: {name} could not be written"));
        }
    }
    Ok(())
}

/// The Start menu's shortcut to the settings.
fn link() -> Result<(), String> {
    let lnk = start_menu_link();
    let target = install_dir().join(SETUP_EXE);
    // WScript.Shell writes a .lnk (windows-sys has no COM interfaces). The paths go in as environment
    // variables: never as code, and -Command would join extra arguments into the script
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:RW_LNK); $s.TargetPath = $env:RW_TARGET; $s.WorkingDirectory = $env:RW_DIR; $s.Description = 'retro-wall: themes, screens, instruments, pausing'; $s.Save()";
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("RW_LNK", &lnk)
        .env("RW_TARGET", &target)
        .env("RW_DIR", install_dir())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !out.status.success() || !lnk.is_file() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

/// Uninstall: stop the wall, put the desktop's own wallpaper back, remove the task, the entry, the
/// shortcut and the install; the settings too unless `keep_settings`. This program, if it runs from the
/// install, is removed by a helper a moment after it exits.
pub fn uninstall(keep_settings: bool, say: Say) -> Result<(), String> {
    need_profile()?;
    let dir = install_dir();
    stop_wall(say);
    if schtasks(&["/Query", "/TN", task::NAME]).is_ok_and(|r| r.0 == 0) {
        match schtasks(&["/Delete", "/TN", task::NAME, "/F"])? {
            (0, _) => say("task removed"),
            (code, out) => return Err(format!("the task could not be removed (schtasks {code}): {out}")),
        }
    }
    // the task's folder, if empty: schtasks cannot, the scheduler's own COM object can
    let _ = run(
        Path::new("powershell.exe"),
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "try { $s = New-Object -ComObject Schedule.Service; $s.Connect(); $s.GetFolder('\\').DeleteFolder('retro-wall', 0) } catch {}",
        ],
    );
    let saved = saved_desktop();
    if saved.is_file() && wall_exe().is_file() {
        match run(&wall_exe(), &["--desktop", "restore", &saved.to_string_lossy()])? {
            (0, _) => say("wallpaper settings put back"),
            (code, out) => return Err(format!("the wallpaper settings were not put back (exit {code}: {out}): {} kept", saved.display())),
        }
    }
    let key = wide(UNINSTALL_KEY);
    // SAFETY: a NUL-terminated key path under HKCU.
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
    let _ = std::fs::remove_file(start_menu_link());
    if !keep_settings {
        // what it wrote: the user's own themes, sources and lines stay
        retro_wall::setup::settings::forget(&settings_dir())?;
        say("settings removed");
    }
    if dir.exists() {
        let _ = std::env::set_current_dir(std::env::temp_dir());
        if running_installed() {
            // everything but this program, which a helper removes with the folder once it has exited
            for e in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
                if e.file_name() == SETUP_EXE {
                    continue;
                }
                let p = e.path();
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
            spawn_remover(&dir);
        } else {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        say(&format!("removed {}", dir.display()));
    }
    Ok(())
}

/// The helper that removes the install once this program, which runs from it, has exited: this
/// program without its payload, copied to the temporary folder and started there with
/// `--remove-after`. A program, not a script: nothing to quote, no console needed. At the next sign-in,
/// a RunOnce entry takes what is left, the helper's copy too.
fn spawn_remover(dir: &Path) {
    let helper = std::env::temp_dir().join(format!("retro-wall-remove-{}.exe", std::process::id()));
    let copied = std::env::current_exe().ok().and_then(|me| std::fs::read(me).ok()).is_some_and(|b| std::fs::write(&helper, payload::strip(&b)).is_ok());
    run_once_removal(dir, &helper);
    if copied {
        let _ = Command::new(&helper)
            .args(["--remove-after", &std::process::id().to_string(), &dir.to_string_lossy()])
            .current_dir(std::env::temp_dir())
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn();
    }
}

/// The helper's work (`--remove-after PID DIR`): wait for that process to exit (a minute at most: a
/// message left open longer leaves the folder to the RunOnce entry), then remove the folder, tried for
/// 30 seconds (a file can stay locked a moment after its program has gone), each failure in
/// %TEMP%\retro-wall-uninstall.log.
pub fn remove_after(pid: u32, dir: &Path) {
    use std::io::Write;
    // SAFETY: a handle to wait on, closed after.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 60_000);
            CloseHandle(h);
        }
    }
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("retro-wall-uninstall.log")).ok();
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        match std::fs::remove_dir_all(dir) {
            Ok(()) => break,
            Err(e) if !dir.exists() => {
                let _ = e;
                break;
            }
            Err(e) => {
                if let Some(l) = log.as_mut() {
                    let _ = writeln!(l, "{}: {e}", dir.display());
                }
            }
        }
    }
    if let Some(l) = log.as_mut() {
        let _ = writeln!(l, "{}: {}", dir.display(), if dir.exists() { "still there" } else { "removed" });
    }
}

/// At the next sign-in, remove `dir` and the helper's copy (what an uninstall could not while this
/// program ran from the install). The folder only if it has no wall's program in it: an install made
/// again before then stays. A last resort, for when the helper could not: Windows skips a RunOnce
/// command over 260 characters (a very long profile path), and expands a `%` in a path.
fn run_once_removal(dir: &Path, helper: &Path) {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\RunOnce");
    let name = wide("retro-wall-uninstall");
    // only if not installed again since: the folder without the wall's program
    let value = wide(format!(
        "cmd.exe /c (if not exist \"{}\" rmdir /s /q \"{}\") & del /f /q \"{}\"",
        dir.join("retro-wall.exe").display(),
        dir.display(),
        helper.display()
    ));
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: HKCU's RunOnce key, opened and closed here; a NUL-terminated value.
    unsafe {
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut h,
            std::ptr::null_mut(),
        ) == ERROR_SUCCESS
        {
            RegSetValueExW(h, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32);
            RegCloseKey(h);
        }
    }
}

/// A theme's instruments as it has them (the default layout for a theme with none), with the battery's
/// meter: what the settings leave hidden instruments out of.
pub fn layout(theme: &str) -> Vec<config::Group> {
    // by path: a downloaded copy of this program does not have the install's themes beside it
    let spec = [install_dir().join("themes").join(theme), settings_dir().join("themes").join(theme)]
        .into_iter()
        .map(|d| d.join("theme.toml"))
        .find(|p| p.is_file())
        .map_or_else(|| theme.to_string(), |p| p.to_string_lossy().into_owned());
    let groups =
        config::load(&spec, &config::Config::default(), retro_wall::elements::kind_info, false).ok().map(|l| l.plain.groups.clone()).unwrap_or_default();
    if groups.is_empty() { config::default_layout() } else { groups }
}

/// Whether a theme has blocky's world, whose interface and hand the settings can change.
fn has_world(theme: &str) -> bool {
    [install_dir().join("themes").join(theme), settings_dir().join("themes").join(theme)]
        .iter()
        .find_map(|d| std::fs::read_to_string(d.join("theme.toml")).ok())
        .is_some_and(|t| t.contains("type = \"blocky\""))
}

/// The settings as they can be kept: blocky's extras only for a theme that has its world (a theme of
/// the setup program's own extending another would be one the wall refuses).
pub fn fitted(s: &Settings) -> Settings {
    let mut s = s.clone();
    if !has_world(s.theme.as_deref().unwrap_or("phosphor")) {
        (s.ui, s.hand) = (None, None);
    }
    s
}

/// Keep the settings and restart the wall with them.
pub fn apply(s: &Settings, say: Say) -> Result<(), String> {
    fitted(s).write(&settings_dir(), &layout)?;
    say("settings saved");
    stop_wall(say);
    start_wall(say)
}
