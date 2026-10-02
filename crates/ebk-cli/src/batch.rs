//! What the program does when it is started without a command: by a double click, or with files dropped on it.
//! Every EPUB in the folder of the program - or every file and folder it was given - is converted, each to an
//! .ebk file next to it. Nothing is deleted or replaced.

use std::ffi::OsString;
use std::fs::File;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ebk::Options;

const COMMANDS: &[&str] = &["convert", "info", "extract", "verify", "epub", "help"];
const LOG_NAME: &str = "ebk-convert.log";

/// No arguments, or nothing but existing files and folders (and no name of a command in front).
pub fn wanted(args: &[OsString]) -> bool {
    match args.first() {
        None => true,
        Some(first) => !COMMANDS.iter().any(|c| first == c) && args.iter().all(|a| Path::new(a).exists()),
    }
}

pub fn run(args: &[OsString]) -> ExitCode {
    let chinese = chinese();
    let say = |zh: &str, en: &str| if chinese { zh.to_owned() } else { en.to_owned() };
    let (folder, inputs, mut others) = inputs(args);
    // without a terminal (a double click in a file manager on Linux) the lines go to a file in the folder as
    // well; added to what is in it: a file of that name may be someone's
    let log = || File::options().append(true).create(true).open(folder.join(LOG_NAME)).ok();
    let mut out = Report { log: (!std::io::stdout().is_terminal()).then(log).flatten() };
    out.line(&format!("EBK {}", env!("CARGO_PKG_VERSION")));
    others.sort();
    for other in &others {
        out.line(&format!("{}: {}", other.display(), say("不是 .epub 文件，跳过", "not an .epub file, skipped")));
    }
    if inputs.is_empty() {
        out.line(&say(
            &format!("在 {} 里没有找到 .epub 文件。\n把这个程序放进有 EPUB 的文件夹再运行，或者把 EPUB 文件（或文件夹）拖到它上面。\n命令行用法见 ebk help。", folder.display()),
            &format!("No .epub files in {}.\nPut this program in a folder with EPUB files and run it, or drop EPUB files (or a folder) on it.\nFor the command line see: ebk help.", folder.display()),
        ));
        wait(&say("按回车键关闭…", "Press Enter to close…"));
        return ExitCode::FAILURE;
    }

    let (mut done, mut skipped, mut failed) = (0usize, 0usize, Vec::new());
    let (mut from, mut to) = (0u64, 0u64);
    for (n, input) in inputs.iter().enumerate() {
        let name = input.file_name().unwrap_or(input.as_os_str()).to_string_lossy().into_owned();
        let head = format!("[{}/{}] {name}", n + 1, inputs.len());
        if input.with_extension("ebk").symlink_metadata().is_ok() {
            out.line(&format!("{head}: {}", say("已有同名的 .ebk，跳过", "the .ebk file is there already, skipped")));
            skipped += 1;
            continue;
        }
        // the name first: a large book takes a while
        out.start(&format!("{head} … "));
        match crate::convert(input, None, Options::default(), None, crate::DEFAULT_MAX_INPUT, false) {
            Ok(book) => {
                out.line(&format!("{} → {} ({:.0}%)", size(book.epub_len), size(book.file_len), 100.0 * book.file_len as f64 / book.epub_len as f64));
                (done, from, to) = (done + 1, from + book.epub_len, to + book.file_len);
            }
            Err(e) => {
                out.line(&format!("{}: {e:#}", say("失败", "failed")));
                failed.push(name);
            }
        }
    }

    out.line("");
    if done > 0 {
        out.line(&say(
            &format!("转换了 {done} 本：{} → {}（{:.0}%）。原来的 EPUB 没有改动。", size(from), size(to), 100.0 * to as f64 / from as f64),
            &format!("{done} converted: {} → {} ({:.0}%). The EPUB files are untouched.", size(from), size(to), 100.0 * to as f64 / from as f64),
        ));
    }
    if skipped > 0 {
        out.line(&say(&format!("跳过 {skipped} 本（已有 .ebk）。"), &format!("{skipped} skipped (converted earlier).")));
    }
    if !failed.is_empty() {
        out.line(&say(&format!("失败 {} 本：{}", failed.len(), failed.join("、")), &format!("{} failed: {}", failed.len(), failed.join(", "))));
    }
    wait(&say("按回车键关闭…", "Press Enter to close…"));
    if failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Lines for the person who started the program: on the terminal, and in the log file when there is one.
struct Report {
    log: Option<File>,
}

impl Report {
    fn line(&mut self, text: &str) {
        println!("{text}");
        if let Some(log) = &mut self.log {
            let _ = writeln!(log, "{text}");
        }
    }

    /// The beginning of a line that `line` ends.
    fn start(&mut self, text: &str) {
        print!("{text}");
        let _ = std::io::stdout().flush();
        if let Some(log) = &mut self.log {
            let _ = write!(log, "{text}");
        }
    }
}

/// The folder that is worked in, the EPUB files to convert in the order of their names, and what else was given.
fn inputs(args: &[OsString]) -> (PathBuf, Vec<PathBuf>, Vec<PathBuf>) {
    let is_epub = |path: &Path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("epub"));
    let in_folder = |folder: &Path| -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(folder).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| is_epub(path)).collect();
        found.sort();
        found
    };
    if args.is_empty() {
        // the folder of the program, not the current one: after a double click that is somewhere else on macOS
        let folder = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_owned)).unwrap_or_else(|| PathBuf::from("."));
        let found = in_folder(&folder);
        return (folder, found, Vec::new());
    }
    let (mut found, mut others) = (Vec::new(), Vec::new());
    for arg in args.iter().map(PathBuf::from) {
        if arg.is_dir() {
            found.extend(in_folder(&arg));
        } else if is_epub(&arg) {
            found.push(arg);
        } else {
            others.push(arg);
        }
    }
    let first = PathBuf::from(&args[0]);
    let folder = if first.is_dir() { first } else { first.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_owned) };
    (folder, found, others)
}

fn size(bytes: u64) -> String {
    if bytes >= 1 << 20 { format!("{:.1} MB", bytes as f64 / f64::from(1 << 20)) } else { format!("{:.0} KB", (bytes as f64 / 1024.0).max(1.0)) }
}

/// Whether to speak Chinese: the language of the user interface on Windows, of the locale elsewhere.
fn chinese() -> bool {
    #[cfg(windows)]
    {
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        // the low ten bits are the primary language; 4 is Chinese
        (unsafe { GetUserDefaultUILanguage() }) & 0x3ff == 0x04
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty())).is_some_and(|locale| locale.starts_with("zh"))
    }
}

/// On Windows a double click opens a console window that closes when the program ends: wait for a key then.
/// Elsewhere, and in a console that was there before, there is nothing to wait for.
fn wait(prompt: &str) {
    #[cfg(windows)]
    {
        extern "system" {
            fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        }
        let mut ids = [0u32; 2];
        // one process attached to the console: it was made for this program
        if unsafe { GetConsoleProcessList(ids.as_mut_ptr(), 2) } == 1 {
            println!("\n{prompt}");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
    }
    #[cfg(not(windows))]
    let _ = prompt;
}
