//! Reading an EBK book on a computer: the book is written as an EPUB file into a cache folder of the user and
//! handed to the program the system opens EPUB files with. A double click on an .ebk file does this once the
//! file type belongs to this program: on Windows the program takes it when it is started by a double click,
//! on Linux `ebk associate` does it, on macOS the app EBK.app does it.

use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};

use crate::batch::{chinese, wait};

/// What the cache may hold besides the book being opened.
const CACHE_BYTES: u64 = 1 << 30;
/// Marks the last time a book of the cache was opened; the least recently opened go first.
const USED: &str = ".used";

/// Nothing but .ebk files: what a double click on one, or several dropped on the program, gives it.
pub fn wanted(args: &[OsString]) -> bool {
    !args.is_empty() && args.iter().all(|arg| is_ebk(Path::new(arg)))
}

fn is_ebk(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ebk"))
}

pub fn run(files: &[PathBuf]) -> ExitCode {
    let mut failed = false;
    for file in files {
        let name = file.file_name().unwrap_or(file.as_os_str()).to_string_lossy();
        match open_book(file) {
            Ok(epub) => println!("{name} → {}", epub.display()),
            Err(e) => {
                let text = if chinese() { format!("打不开 {name}：{e:#}") } else { format!("Cannot open {name}: {e:#}") };
                eprintln!("{text}");
                notify(&text);
                failed = true;
            }
        }
    }
    if failed {
        wait(if chinese() { "按回车键关闭…" } else { "Press Enter to close…" });
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn open_book(file: &Path) -> Result<PathBuf> {
    let epub = cached(file, &cache_dir()?)?;
    launch(&epub)?;
    Ok(epub)
}

/// The folder of the cache: the user's cache folder of the system, or `EBK_CACHE_DIR`.
fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("EBK_CACHE_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(dir.into());
    }
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let dir = if cfg!(windows) {
        var("LOCALAPPDATA").map(|dir| dir.join("EBK"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Caches/EBK"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|home| home.join(".cache"))).map(|dir| dir.join("ebk"))
    };
    dir.context("there is no folder for the cache (LOCALAPPDATA, HOME or EBK_CACHE_DIR)")
}

/// The book as an EPUB file in the cache: one folder per book, named after the path, size and time of the .ebk
/// file, so that the same book opens as the same file (and the reading program finds its place in it again).
pub fn cached(file: &Path, cache: &Path) -> Result<PathBuf> {
    let meta = std::fs::metadata(file).with_context(|| format!("cannot open {}", file.display()))?;
    let full = std::fs::canonicalize(file).with_context(|| format!("cannot open {}", file.display()))?;
    let since = meta.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    let mut key = full.as_os_str().as_encoded_bytes().to_vec();
    key.extend_from_slice(&meta.len().to_le_bytes());
    key.extend_from_slice(&since.to_le_bytes());
    let folder = cache.join(format!("{:016x}", fnv1a(&key)));
    // the name of the book, which some programs show; not too long for the paths of Windows
    let stem: String = file.file_stem().unwrap_or_default().to_string_lossy().chars().take(80).collect();
    let epub = folder.join(format!("{}.epub", stem.trim()));

    if !epub.is_file() {
        std::fs::create_dir_all(&folder).with_context(|| format!("cannot create {}", folder.display()))?;
        tidy(cache, meta.len().saturating_mul(2), &folder);
        let written = crate::to_epub(file, &epub, false, crate::DEFAULT_MAX_OUTPUT, ebk::MAX_MEMBER_LEN);
        // started twice at once, the other start may have written it
        if let Err(e) = written {
            if !epub.is_file() {
                let _ = std::fs::remove_dir(&folder);
                return Err(e);
            }
        }
    }
    let _ = File::create(folder.join(USED));
    tidy(cache, 0, &folder);
    Ok(epub)
}

/// FNV-1a, 64 bits: a name that stays the same from one version of the program to the next.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// Removes the least recently opened books until `room` more bytes fit under the limit; never `keep`. Files
/// that cannot be removed (a reading program has them open, on Windows) stay. What a stopped run left is removed
/// once it is older than ten minutes.
fn tidy(cache: &Path, room: u64, keep: &Path) {
    let now = SystemTime::now();
    let old = |t: SystemTime| now.duration_since(t).unwrap_or_default() > Duration::from_secs(600);
    let mut books = Vec::new();
    let mut total = 0u64;
    for entry in std::fs::read_dir(cache).into_iter().flatten().flatten() {
        let folder = entry.path();
        // only the folders made here: a name of 16 hexadecimal digits
        let ours = entry.file_name().to_str().is_some_and(|name| name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit()));
        if folder == keep || !ours || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let (mut size, mut epub, mut used) = (0u64, false, None);
        for file in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let Ok(meta) = file.metadata() else { continue };
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == "tmp") && meta.modified().is_ok_and(old) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            epub |= path.extension().is_some_and(|ext| ext == "epub");
            size += meta.len();
            if file.file_name() == USED || used.is_none() {
                used = meta.modified().ok().or(used);
            }
        }
        let used = used.unwrap_or(SystemTime::UNIX_EPOCH);
        if !epub && old(used) {
            let _ = std::fs::remove_dir_all(&folder);
            continue;
        }
        total += size;
        books.push((used, size, folder));
    }
    books.sort();
    for (_, size, folder) in books {
        if total.saturating_add(room) <= CACHE_BYTES {
            break;
        }
        if std::fs::remove_dir_all(&folder).is_ok() {
            total -= size;
        }
    }
}

/// Hands the EPUB file to the program the system opens such files with (`EBK_OPENER` names another, for tests).
fn launch(epub: &Path) -> Result<()> {
    let no_reader = || if chinese() {
        "这台电脑上没有能打开 EPUB 的程序。装一个阅读器（例如 Calibre、Thorium Reader、SumatraPDF）再试一次。"
    } else {
        "there is no program for EPUB files on this computer. Install a reader (Calibre, Thorium Reader, SumatraPDF, …) and try again."
    };
    let run = |program: &OsString| -> Result<()> {
        let status = std::process::Command::new(program).arg(epub).status().with_context(|| format!("cannot start {}", program.to_string_lossy()))?;
        if !status.success() {
            bail!("{} ({}: {status})", no_reader(), program.to_string_lossy());
        }
        Ok(())
    };
    if let Some(opener) = std::env::var_os("EBK_OPENER").filter(|value| !value.is_empty()) {
        return run(&opener);
    }
    #[cfg(windows)]
    {
        windows::shell_open(epub).ok_or_else(|| anyhow::anyhow!(no_reader()))
    }
    #[cfg(target_os = "macos")]
    {
        run(&"open".into())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        run(&"xdg-open".into())
    }
}

/// Without a terminal (a double click on Linux) a message on the screen as well, when the desktop can show one.
fn notify(text: &str) {
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        use std::io::IsTerminal;
        if !std::io::stderr().is_terminal() {
            let _ = std::process::Command::new("notify-send").args(["EBK", text]).status();
        }
    }
    let _ = text;
}

/// `ebk associate`: a double click on an .ebk file opens it with this program; with `remove`, no longer.
pub fn associate(remove: bool) -> Result<String> {
    let exe = std::env::current_exe().context("cannot find where this program is")?;
    #[cfg(windows)]
    {
        if remove {
            windows::unregister()?;
            return Ok(if chinese() { ".ebk 文件不再由这个程序打开。".into() } else { ".ebk files are no longer opened by this program.".into() });
        }
        windows::register(&exe)?;
        Ok(if chinese() { format!("双击 .ebk 文件时由 {} 打开。", exe.display()) } else { format!("A double click on an .ebk file opens it with {}.", exe.display()) })
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (exe, remove);
        bail!("on macOS the app EBK.app opens .ebk files: put it into Applications and double-click a book once")
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        linux::associate(&exe, remove)
    }
}

/// On Windows, started by a double click: the .ebk file type is given to this program unless it already has
/// it - from the same place - or the user gave it to another. Gives a line for the window when it did something.
#[cfg(windows)]
pub fn associate_if_needed() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    if windows::registered(&exe) {
        return None;
    }
    windows::register(&exe).ok()?;
    Some(if chinese() {
        "以后双击 .ebk 文件，就会用这台电脑上的 EPUB 阅读器打开它（不想这样：ebk associate --remove）。".into()
    } else {
        "From now on a double click on an .ebk file opens it in this computer's EPUB reader (to undo: ebk associate --remove).".into()
    })
}

#[cfg(windows)]
mod windows {
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use anyhow::{bail, Result};

    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const REG_SZ: u32 = 1;
    const RRF_RT_REG_SZ: u32 = 2;
    const PROG_ID: &str = "EBK.Book";

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(window: *mut c_void, verb: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> isize;
        fn SHChangeNotify(event: i32, flags: u32, one: *const c_void, two: *const c_void);
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, model: u32) -> i32;
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn RegSetKeyValueW(key: isize, subkey: *const u16, name: *const u16, kind: u32, data: *const c_void, len: u32) -> i32;
        fn RegGetValueW(key: isize, subkey: *const u16, name: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, len: *mut u32) -> i32;
        fn RegDeleteTreeW(key: isize, subkey: *const u16) -> i32;
    }

    fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
        text.as_ref().encode_wide().chain([0]).collect()
    }

    /// Opens the file with the program Windows has for it; when there is none, Windows asks which to use.
    pub fn shell_open(path: &Path) -> Option<()> {
        let file = wide(path);
        // shell extensions may need COM, in an apartment of one thread
        unsafe { CoInitializeEx(null_mut(), 0x2 | 0x4) };
        let code = unsafe { ShellExecuteW(null_mut(), wide("open").as_ptr(), file.as_ptr(), null(), null(), 1) };
        // 31: no program is associated with the file type
        if code > 32 || (code == 31 && unsafe { ShellExecuteW(null_mut(), wide("openas").as_ptr(), file.as_ptr(), null(), null(), 1) } > 32) {
            return Some(());
        }
        None
    }

    fn command(exe: &Path) -> String {
        format!("\"{}\" \"%1\"", exe.display())
    }

    fn set(subkey: &str, value: &str) -> Result<()> {
        let data = wide(value);
        let code = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, wide(subkey).as_ptr(), null(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
        if code != 0 {
            bail!("cannot write HKEY_CURRENT_USER\\{subkey} (error {code})");
        }
        Ok(())
    }

    fn get(subkey: &str) -> Option<String> {
        let mut data = vec![0u16; 2048];
        let mut len = (data.len() * 2) as u32;
        let code = unsafe { RegGetValueW(HKEY_CURRENT_USER, wide(subkey).as_ptr(), null(), RRF_RT_REG_SZ, null_mut(), data.as_mut_ptr().cast(), &mut len) };
        (code == 0).then(|| String::from_utf16_lossy(&data[..(len as usize / 2).saturating_sub(1)]))
    }

    /// Whether .ebk files go to this program (in this place), or the user has chosen another for them.
    pub fn registered(exe: &Path) -> bool {
        let chosen = get(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.ebk\UserChoice");
        chosen.is_some() || get(&format!(r"Software\Classes\{PROG_ID}\shell\open\command")).is_some_and(|c| c == command(exe))
    }

    pub fn register(exe: &Path) -> Result<()> {
        let name = if super::chinese() { "EBK 电子书" } else { "EBK book" };
        set(&format!(r"Software\Classes\{PROG_ID}"), name)?;
        set(&format!(r"Software\Classes\{PROG_ID}\shell\open\command"), &command(exe))?;
        set(r"Software\Classes\.ebk", PROG_ID)?;
        changed();
        Ok(())
    }

    pub fn unregister() -> Result<()> {
        if get(r"Software\Classes\.ebk").is_some_and(|id| id == PROG_ID) {
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(r"Software\Classes\.ebk").as_ptr()) };
        }
        let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(format!(r"Software\Classes\{PROG_ID}")).as_ptr()) };
        // 2: it was not there
        if code != 0 && code != 2 {
            bail!("cannot remove HKEY_CURRENT_USER\\Software\\Classes\\{PROG_ID} (error {code})");
        }
        changed();
        Ok(())
    }

    /// Tells Explorer, which keeps the file types in memory.
    fn changed() {
        unsafe { SHChangeNotify(0x0800_0000, 0, null(), null()) };
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod linux {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use anyhow::{bail, Context, Result};

    use super::chinese;

    const MIME: &str = "application/x-ebk";

    fn data_home() -> Result<PathBuf> {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|home| home.join(".local/share"))).context("HOME is not set")
    }

    /// An argument of `Exec` in a desktop entry: quoted, then escaped once more as a string of the file.
    fn exec_argument(path: &Path) -> Result<String> {
        let Some(text) = path.to_str().filter(|text| !text.contains(['\n', '\r'])) else {
            bail!("the path of this program cannot be written into a desktop entry: {}", path.display());
        };
        let mut quoted = String::from("\"");
        for c in text.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        Ok(quoted.replace('\\', "\\\\").replace('%', "%%"))
    }

    /// Runs a tool of the desktop if it is there; whether it ran and succeeded.
    fn tool(program: &str, args: &[&std::ffi::OsStr]) -> bool {
        Command::new(program).args(args).output().is_ok_and(|out| out.status.success())
    }

    pub fn associate(exe: &Path, remove: bool) -> Result<String> {
        let data = data_home()?;
        let (mime_dir, apps) = (data.join("mime"), data.join("applications"));
        let (package, desktop) = (mime_dir.join("packages/ebk.xml"), apps.join("ebk.desktop"));
        if remove {
            for file in [&package, &desktop] {
                if let Err(e) = std::fs::remove_file(file) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(e).with_context(|| format!("cannot remove {}", file.display()));
                    }
                }
            }
            tool("update-mime-database", &[mime_dir.as_os_str()]);
            tool("update-desktop-database", &[apps.as_os_str()]);
            return Ok(if chinese() { ".ebk 文件不再由这个程序打开。".into() } else { ".ebk files are no longer opened by this program.".into() });
        }
        let exec = exec_argument(exe)?;
        std::fs::create_dir_all(package.parent().unwrap()).with_context(|| format!("cannot create {}", mime_dir.display()))?;
        std::fs::create_dir_all(&apps).with_context(|| format!("cannot create {}", apps.display()))?;
        std::fs::write(&package, format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n  <mime-type type=\"{MIME}\">\n    <comment>EBK book</comment>\n    <comment xml:lang=\"zh_CN\">EBK 电子书</comment>\n    <glob pattern=\"*.ebk\"/>\n  </mime-type>\n</mime-info>\n"
        )).with_context(|| format!("cannot write {}", package.display()))?;
        std::fs::write(&desktop, format!(
            "[Desktop Entry]\nType=Application\nName=EBK\nName[zh_CN]=EBK 电子书\nComment=Opens EBK books in the EPUB reader of this computer\nComment[zh_CN]=用这台电脑上的 EPUB 阅读器打开 EBK 电子书\nExec={exec} open %F\nMimeType={MIME};\nTerminal=false\nNoDisplay=true\n"
        )).with_context(|| format!("cannot write {}", desktop.display()))?;
        let known = tool("update-mime-database", &[mime_dir.as_os_str()]);
        tool("update-desktop-database", &[apps.as_os_str()]);
        let default = tool("xdg-mime", &["default".as_ref(), "ebk.desktop".as_ref(), MIME.as_ref()]);
        let mut text = if chinese() {
            format!("双击 .ebk 文件时由 {} 打开（{}，{}）。", exe.display(), desktop.display(), package.display())
        } else {
            format!("A double click on an .ebk file opens it with {} ({}, {}).", exe.display(), desktop.display(), package.display())
        };
        if !known || !default {
            text += if chinese() {
                "\n没有找到 update-mime-database 或 xdg-mime：文件管理器可能要重新登录后才认得 .ebk 文件。"
            } else {
                "\nupdate-mime-database or xdg-mime is missing: the file manager may know .ebk files only after logging in again."
            };
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_book_opens_as_the_same_file_and_the_cache_keeps_to_its_limit() {
        let dir = std::env::temp_dir().join(format!("ebk-open-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        // books the cache had before: two over the limit together, the newer one opened more recently
        for (name, used) in [("aaaaaaaaaaaaaaaa", 3000), ("bbbbbbbbbbbbbbbb", 1000)] {
            let folder = cache.join(name);
            std::fs::create_dir(&folder).unwrap();
            let file = File::create(folder.join("old.epub")).unwrap();
            file.set_len(CACHE_BYTES / 2 + 1).unwrap();
            let marker = File::create(folder.join(USED)).unwrap();
            marker.set_modified(SystemTime::now() - Duration::from_secs(used)).unwrap();
        }
        // what a stopped run left: a folder without a book, and a temporary file
        std::fs::create_dir(cache.join("cccccccccccccccc")).unwrap();
        File::create(cache.join("cccccccccccccccc/.ebk-1-0.tmp")).unwrap().set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
        // and what is not the cache's: left alone, however old or large
        std::fs::create_dir(cache.join("photos")).unwrap();
        File::create(cache.join("photos/big")).unwrap().set_len(CACHE_BYTES * 2).unwrap();

        tidy(&cache, 0, &cache.join("none"));
        let mut left: Vec<_> = std::fs::read_dir(&cache).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        // the least recently opened went; the other fits
        assert_eq!(left, ["bbbbbbbbbbbbbbbb", "photos"]);
        tidy(&cache, CACHE_BYTES, &cache.join("bbbbbbbbbbbbbbbb"));
        assert!(cache.join("bbbbbbbbbbbbbbbb").exists(), "the book being opened stays");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_do_not_change() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
