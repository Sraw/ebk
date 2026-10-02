mod batch;
mod epub;
mod zipout;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ebk::{CodePage, FileSource, Mode, Options, Pack, Reader, Source, Writer};

use epub::Archive;

#[derive(Parser)]
#[command(name = "ebk", version, about = "Convert EPUB to the EBK container and inspect EBK files")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert an EPUB to EBK
    Convert {
        input: PathBuf,
        /// Output file (default: the input with the extension .ebk)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Target size of a text block in bytes, 65536 to 16777216
        #[arg(long, default_value_t = ebk::DEFAULT_BLOCK_SIZE)]
        block_size: u64,
        /// brotli quality, 0-11
        #[arg(long, default_value_t = 11)]
        quality: u32,
        /// Compression threads (default: all cores)
        #[arg(long)]
        threads: Option<usize>,
        /// Refuse an EPUB whose files add up to more than this many bytes
        #[arg(long, default_value_t = DEFAULT_MAX_INPUT)]
        max_input: u64,
        /// Recode text with a code page made for the book: auto (when the file gets smaller), never, always
        #[arg(long, default_value = "auto", value_parser = code_page)]
        code_page: CodePage,
        /// Leave JPEG files as they are instead of recompressing them (they read back the same either way)
        #[arg(long)]
        keep_jpeg: bool,
    },
    /// Show the layout of an EBK file
    Info {
        file: PathBuf,
        /// List every member
        #[arg(long)]
        members: bool,
    },
    /// Write every member of an EBK file under a new or empty directory
    Extract {
        file: PathBuf,
        dir: PathBuf,
        /// Write nothing if the members add up to more than this many bytes
        #[arg(long, default_value_t = DEFAULT_MAX_OUTPUT)]
        max_output: u64,
        /// Write nothing if a member is larger than this many bytes (each member is held in memory)
        #[arg(long, default_value_t = ebk::MAX_MEMBER_LEN)]
        max_member: u64,
    },
    /// Write the members of an EBK file as an EPUB file (the same files; not the ZIP file the book came from)
    Epub {
        file: PathBuf,
        /// Output file, which must not exist (default: the input with the extension .epub)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Do not compress: faster to write, for an EPUB that is read once and thrown away
        #[arg(long)]
        store: bool,
        /// Write nothing if the members add up to more than this many bytes
        #[arg(long, default_value_t = DEFAULT_MAX_OUTPUT)]
        max_output: u64,
        /// Write nothing if a member is larger than this many bytes (each member is held in memory)
        #[arg(long, default_value_t = ebk::MAX_MEMBER_LEN)]
        max_member: u64,
    },
    /// Read every member and check it; with --epub, also compare with the EPUB it was made from
    Verify {
        file: PathBuf,
        #[arg(long)]
        epub: Option<PathBuf>,
        /// Stop once the members read add up to more than this many bytes; also the most the EPUB may unpack to
        #[arg(long, default_value_t = DEFAULT_MAX_OUTPUT)]
        max_output: u64,
        /// Report members larger than this many bytes as too large (each member is held in memory)
        #[arg(long, default_value_t = ebk::MAX_MEMBER_LEN)]
        max_member: u64,
    },
}

/// The program `ebk`: what `main` does.
pub fn run() -> ExitCode {
    // The JPEG codec panics on some damaged files. The reader catches that and reports the member as damaged;
    // the message of the panic itself would only look like a crash of this program.
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !info.location().is_some_and(|at| at.file().contains("lepton_jpeg")) {
            report(info);
        }
    }));
    // started by a double click or with files dropped on it: convert them, no command needed
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if batch::wanted(&args) {
        return batch::run(&args);
    }
    let result = match Cli::parse().command {
        Command::Convert { input, output, block_size, quality, threads, max_input, code_page, keep_jpeg } => {
            convert(&input, output, Options { block_size, quality, code_page, ..Options::default() }, threads, max_input, keep_jpeg).map(|book| {
                let code_page = book.charset_len.map_or(String::new(), |n| format!(", code page of {n} characters"));
                println!("{}: {} -> {} bytes ({:.4}), {} members, {} text blocks{code_page}",
                         book.output.display(), book.epub_len, book.file_len, book.file_len as f64 / book.epub_len as f64, book.members, book.blocks);
            })
        }
        Command::Epub { file, output, store, max_output, max_member } => {
            let output = output.unwrap_or_else(|| file.with_extension("epub"));
            to_epub(&file, &output, store, max_output, max_member).map(|files| println!("{}: {files} files", output.display()))
        }
        Command::Info { file, members } => info(&file, members),
        Command::Extract { file, dir, max_output, max_member } => extract(&file, &dir, max_output, max_member),
        Command::Verify { file, epub, max_output, max_member } => verify(&file, epub.as_deref(), max_output, max_member),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ebk: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// A few bytes of brotli expand to 16 MiB, so a small file can declare any amount of text.
pub const DEFAULT_MAX_OUTPUT: u64 = 16 << 30;
/// A few kilobytes of deflate expand to gigabytes, and the converter holds the whole book in memory.
const DEFAULT_MAX_INPUT: u64 = 4 << 30;

/// Formats that are already compressed; everything else that is not text is tried with brotli.
const STORED_AS_IS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "avif", "jxl", "woff", "woff2", "mp3", "m4a", "mp4", "m4v", "ogg", "oga", "ogv", "opus", "webm"];

fn code_page(arg: &str) -> Result<CodePage, String> {
    match arg {
        "auto" => Ok(CodePage::Auto),
        "never" => Ok(CodePage::Never),
        "always" => Ok(CodePage::Always),
        _ => Err("one of auto, never, always".into()),
    }
}

/// What `convert` wrote.
struct Converted {
    output: PathBuf,
    epub_len: u64,
    file_len: u64,
    members: usize,
    blocks: usize,
    charset_len: Option<usize>,
}

fn convert(input: &Path, output: Option<PathBuf>, mut opts: Options, threads: Option<usize>, max_input: u64, keep_jpeg: bool) -> Result<Converted> {
    let output = output.unwrap_or_else(|| input.with_extension("ebk"));
    if std::fs::canonicalize(&output).is_ok_and(|out| std::fs::canonicalize(input).is_ok_and(|inp| inp == out)) {
        bail!("the output would replace the input; name another file with -o");
    }
    let mut entries = epub::read(input, max_input)?;
    if !entries.iter().any(|e| e.path == "META-INF/container.xml") {
        bail!("{} is not an EPUB: it has no META-INF/container.xml", input.display());
    }
    // what the writer refuses here is a fault of the EPUB, not of an EBK file
    let refused = |e: ebk::Error| match e {
        ebk::Error::Invalid(why) => anyhow::anyhow!("this EPUB cannot be converted: {why}"),
        e => e.into(),
    };
    if let Some(threads) = threads {
        opts.threads = threads;
    }
    let mut writer = Writer::new(opts).map_err(|e| match e {
        ebk::Error::Invalid(why) => anyhow::anyhow!("{why} (--block-size takes 65536 to 16777216)"),
        e => e.into(),
    })?;
    let (order, head) = epub::reading_order(&entries);
    for (n, &i) in order.iter().enumerate() {
        if n == head {
            writer.end_head();
        }
        // the writer takes the data over: the book is not held twice
        let data = std::mem::take(&mut entries[i].data);
        let (path, deflated) = (&entries[i].path, entries[i].deflated);
        if std::str::from_utf8(&data).is_ok() {
            writer.add_text(path, &data).map_err(refused)?;
        } else {
            let ext = path.rsplit_once('.').map_or(String::new(), |(_, ext)| ext.to_ascii_lowercase());
            // A JPEG is known by its first bytes, whatever it is called. For it and for the other formats that are
            // compressed already, brotli is tried only where the ZIP file's deflate found something to save:
            // that costs nothing to know, and it keeps the EBK file from coming out larger than the EPUB.
            let compressed_format = STORED_AS_IS.contains(&ext.as_str());
            let pack = match (data.starts_with(&[0xFF, 0xD8, 0xFF]) && !keep_jpeg, deflated) {
                (true, true) => Pack::JpegOrBrotli,
                (true, false) => Pack::Jpeg,
                (false, false) if compressed_format || data.starts_with(&[0xFF, 0xD8, 0xFF]) => Pack::AsIs,
                (false, _) => Pack::Brotli,
            };
            writer.add_resource(path, data, pack).map_err(refused)?;
        }
    }
    drop(entries);
    let mut out = Vec::new();
    let summary = writer.finish(&mut out).map_err(refused)?;

    // before anything reaches the disk, read every member back and compare it with the EPUB, read a second time
    let mut archive = Archive::open(input, max_input)?;
    let mut reader = Reader::open(&out[..]).context("the file just written does not open")?;
    reader.set_member_limit(u64::MAX);
    if reader.member_count() != archive.len() {
        bail!("the output has {} members, the EPUB {}", reader.member_count(), archive.len());
    }
    let in_epub: HashMap<String, usize> = (0..archive.len()).map(|i| (archive.path(i).to_owned(), i)).collect();
    // in member order, so that each text block is decoded once
    for i in 0..reader.member_count() {
        let path = reader.member(i).unwrap().path.to_owned();
        let Some(&original) = in_epub.get(&path) else { bail!("{path:?} is not in the EPUB") };
        if reader.read(i)? != archive.read(original)? {
            bail!("{path:?} does not read back identically");
        }
    }

    write_new(&output, &out)?;
    let epub_len = std::fs::metadata(input)?.len();
    Ok(Converted { output, epub_len, file_len: summary.file_len, members: archive.len(), blocks: reader.blocks().len(), charset_len: summary.charset_len })
}

/// Writes `data` under a temporary name next to `path` and renames it, so that `path` is never half written.
fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let cannot = || format!("cannot write {}", path.display());
    let (temp, mut file) = temporary_next_to(path)?;
    // from here on the temporary file is ours to remove
    let written = file.write_all(data).and_then(|()| file.sync_all()).and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written.with_context(cannot)
}

/// A new file next to `path`, to be renamed to it. A short name of its own: derived from the output's name it
/// could be too long for the file system. A file of that name left by a run that was stopped is passed over.
fn temporary_next_to(path: &Path) -> Result<(PathBuf, File)> {
    let mut n = 0;
    loop {
        let temp = path.with_file_name(format!(".ebk-{}-{n}.tmp", std::process::id()));
        match File::options().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => n += 1,
            Err(e) => return Err(e).with_context(|| format!("cannot create {}", temp.display())),
        }
    }
}

/// Whether there is anything under this name, a link that leads nowhere included.
fn taken(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn open(file: &Path) -> Result<Reader<FileSource>> {
    let source = FileSource::new(File::open(file).with_context(|| format!("cannot open {}", file.display()))?)?;
    Ok(Reader::open(source)?)
}

fn info(file: &Path, list: bool) -> Result<()> {
    let reader = open(file)?;
    let (major, minor) = reader.version();
    let blocks: u64 = reader.blocks().iter().map(|b| b.packed_len).sum();
    let largest = reader.blocks().iter().map(|b| b.raw_len).max().unwrap_or(0);
    let in_stream = reader.members().filter(|m| m.mode.in_stream()).count();
    let resources: u64 = reader.members().filter(|m| !m.mode.in_stream()).map(|m| m.stored_len).sum();
    println!("format version   {major}.{minor}");
    println!("file size        {}", FileSource::new(File::open(file)?)?.len());
    println!("members          {} ({} in the text stream, {} resources)", reader.member_count(), in_stream, reader.member_count() - in_stream);
    let jpeg: Vec<_> = reader.members().filter(|m| m.mode == Mode::Jpeg).collect();
    if !jpeg.is_empty() {
        let (raw, stored): (u64, u64) = jpeg.iter().fold((0, 0), |(raw, stored), m| (raw + m.raw_len, stored + m.stored_len));
        println!("recompressed     {} JPEG files, {raw} -> {stored} bytes", jpeg.len());
    }
    let coded = reader.members().filter(|m| m.mode == Mode::StreamCoded).count();
    if coded > 0 {
        let table = reader.charset_len().map_or("a layout this version does not know".to_owned(), |n| format!("{n} characters"));
        println!("code page        {table}, used by {coded} members");
    }
    println!("text stream      {} bytes in {} blocks (largest {}) -> {} bytes", reader.stream_len(), reader.blocks().len(), largest, blocks);
    println!("resources        {resources} bytes");
    println!("index            {} bytes", reader.index_len());
    if list {
        println!("\nmode    original      stored  crc32     path");
        for m in reader.members() {
            // escaped: a path may hold characters that change what a terminal shows
            println!("{:4} {:11} {:11}  {:08x}  {}", m.mode.to_u8(), m.raw_len, m.stored_len, m.crc32, m.path.escape_debug());
        }
    }
    Ok(())
}

/// Why a member path cannot be created safely on every file system, Windows included (spec section 8).
fn unsafe_on_disk(path: &str) -> Option<&'static str> {
    const DEVICES: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
    for segment in path.split('/') {
        if segment.contains([':', '*', '?', '"', '<', '>', '|']) {
            return Some("a character that Windows does not allow in file names");
        }
        if segment.ends_with(['.', ' ']) {
            return Some("a name that ends in a dot or a space");
        }
        // the limit of the common file systems; Windows counts UTF-16 units, which are never more than the bytes
        if segment.len() > 255 {
            return Some("a name longer than 255 bytes");
        }
        let stem = segment.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
        // COM1 to COM9 and LPT1 to LPT9, and the same with a superscript 1, 2 or 3
        let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "¹" | "²" | "³") || (n.len() == 1 && n.as_bytes()[0].is_ascii_digit()));
        if DEVICES.contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
            return Some("a Windows device name");
        }
    }
    None
}

fn extract(file: &Path, dir: &Path, max_output: u64, max_member: u64) -> Result<()> {
    let mut reader = open(file)?;
    reader.set_member_limit(max_member);
    // everything the index can tell is checked before the first file is created
    let mut total = 0u64;
    for (i, m) in reader.members().enumerate() {
        let path = m.path.escape_debug();
        if let Some(why) = unsafe_on_disk(m.path) {
            bail!("\"{path}\" has {why}; nothing was written");
        }
        if !reader.readable(i) {
            bail!("\"{path}\" is stored in a way this version cannot read (storage mode {}); nothing was written", m.mode.to_u8());
        }
        if m.raw_len > max_member {
            bail!("\"{path}\" is larger than {max_member} bytes (raise --max-member to go on); nothing was written");
        }
        total = total.saturating_add(m.raw_len);
    }
    if total > max_output {
        bail!("the members add up to more than {max_output} bytes (raise --max-output to go on); nothing was written");
    }
    if dir.exists() && std::fs::read_dir(dir)?.next().is_some() {
        bail!("{} is not empty", dir.display());
    }
    // in member order, so that each text block is decoded once
    for i in 0..reader.member_count() {
        // member paths were validated when the file was opened: relative, no '..', no backslash
        let target = dir.join(reader.member(i).unwrap().path);
        let written = reader.read(i).map_err(anyhow::Error::from).and_then(|data| {
            std::fs::create_dir_all(target.parent().unwrap_or(dir))?;
            // create_new: two paths that the file system considers the same name must not overwrite each other
            // escaped: the member's path is part of the name
            let shown = target.to_string_lossy().escape_debug().to_string();
            let mut out = File::options().write(true).create_new(true).open(&target).with_context(|| format!("cannot create \"{shown}\""))?;
            out.write_all(&data).with_context(|| format!("cannot write \"{shown}\""))
        });
        written.with_context(|| format!("stopped after writing {i} of {} members to {}", reader.member_count(), dir.display()))?;
    }
    println!("{} members written to {}", reader.member_count(), dir.display());
    Ok(())
}

/// The members as an EPUB file: `mimetype` first and not compressed, as EPUB asks, then the rest in member order.
/// Gives the number of files written. `output` must not exist.
pub fn to_epub(file: &Path, output: &Path, store: bool, max_output: u64, max_member: u64) -> Result<usize> {
    // never replaced: next to the EBK file there may be the EPUB it was made from
    if taken(output) {
        bail!("{} exists already; name another file with -o", output.display());
    }
    let mut reader = open(file)?;
    reader.set_member_limit(max_member);
    // EPUB files without the file `mimetype` exist, and are converted: the EPUB written here gets one
    let mimetype = reader.find("mimetype");
    if mimetype.is_none() && reader.find("META-INF/container.xml").is_none() {
        bail!("{} has neither \"mimetype\" nor \"META-INF/container.xml\": it was not made from an EPUB", file.display());
    }
    let mut total = 0u64;
    for (i, m) in reader.members().enumerate() {
        let path = m.path.escape_debug();
        if !reader.readable(i) {
            bail!("\"{path}\" is stored in a way this version cannot read (storage mode {}); nothing was written", m.mode.to_u8());
        }
        if m.raw_len > max_member {
            bail!("\"{path}\" is larger than {max_member} bytes (raise --max-member to go on); nothing was written");
        }
        total = total.saturating_add(m.raw_len);
    }
    if total > max_output {
        bail!("the members add up to more than {max_output} bytes (raise --max-output to go on); nothing was written");
    }

    // under a temporary name, so that the output is never there half written
    let (temp, out) = temporary_next_to(output)?;
    let written = (|| -> Result<()> {
        let mut zip = zipout::ZipWriter::new(BufWriter::new(out));
        match mimetype {
            Some(i) => zip.add("mimetype", &reader.read(i)?, false)?,
            None => zip.add("mimetype", b"application/epub+zip", false)?,
        }
        // in member order, so that each text block is decoded once
        for i in (0..reader.member_count()).filter(|&i| Some(i) != mimetype) {
            let path = reader.member(i).unwrap().path.to_owned();
            zip.add(&path, &reader.read(i)?, !store)?;
        }
        let file = zip.finish()?.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        // not `rename`, which replaces: a file that appeared meanwhile stays
        std::fs::hard_link(&temp, output).or_else(|_| if taken(output) { Err(std::io::ErrorKind::AlreadyExists.into()) } else { std::fs::rename(&temp, output) })?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    written.with_context(|| format!("cannot write {}", output.display()))?;
    Ok(reader.member_count() + usize::from(mimetype.is_none()))
}

fn verify(file: &Path, epub: Option<&Path>, max_output: u64, max_member: u64) -> Result<()> {
    let mut reader = open(file)?;
    reader.set_member_limit(max_member);
    let mut archive = match epub {
        Some(path) => Some(Archive::open(path, max_output)?),
        None => None,
    };
    let in_epub: HashMap<String, usize> = archive.iter().flat_map(|a| (0..a.len()).map(|i| (a.path(i).to_owned(), i))).collect();
    let (mut problems, mut total) = (0, 0u64);
    for i in 0..reader.member_count() {
        let m = reader.member(i).unwrap();
        let path = m.path.to_owned();
        // members this version cannot read are reported below; they cost nothing to skip
        if reader.readable(i) {
            total = total.saturating_add(m.raw_len);
            if total > max_output {
                bail!("the members read so far add up to more than {max_output} bytes (raise --max-output to go on)");
            }
        }
        let problem = match (reader.read(i), &mut archive) {
            (Err(e), _) => Some(e.to_string()),
            (Ok(_), None) => None,
            (Ok(data), Some(archive)) => match in_epub.get(&path) {
                None => Some(format!("{}: not in the EPUB", path.escape_debug())),
                Some(&original) => (archive.read(original)? != data).then(|| format!("{}: differs from the EPUB", path.escape_debug())),
            },
        };
        if let Some(problem) = problem {
            eprintln!("{problem}");
            problems += 1;
        }
    }
    for path in in_epub.keys().filter(|p| reader.find(p).is_none()) {
        eprintln!("{}: in the EPUB but not in the EBK file", path.escape_debug());
        problems += 1;
    }
    if problems > 0 {
        bail!("{problems} problems in {}", file.display());
    }
    println!("{}: {} members ok{}", file.display(), reader.member_count(), if epub.is_some() { ", identical to the EPUB" } else { "" });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn write_new_leaves_other_files_alone() {
        let dir = std::env::temp_dir().join(format!("ebk-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (out, temp) = (dir.join("x".repeat(250) + ".ebk"), dir.join(format!(".ebk-{}-0.tmp", std::process::id())));
        // a file with the temporary name is someone else's (or left by a run that was stopped): it stays as
        // it is, and the write takes another name
        std::fs::write(&temp, b"theirs").unwrap();
        super::write_new(&out, b"first").unwrap();
        assert_eq!(std::fs::read(&temp).unwrap(), b"theirs");
        assert_eq!(std::fs::read(&out).unwrap(), b"first");
        std::fs::remove_file(&temp).unwrap();
        // an output name close to the longest a file system takes, replacing an older file
        super::write_new(&out, b"new").unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    use super::unsafe_on_disk;

    #[test]
    fn names_windows_would_misread_are_refused() {
        for path in ["C:/x", "C:x", "a/b?.txt", "a/x*", "a/\"q\"", "a<b", "a>b", "a|b", "dir./x", "x ", "a/NUL", "a/nul.txt", "Com1", "lpt9.a.b", "aux /x", "CON", "COM¹", "lpt².txt", "a/CONIN$", "conout$.log"] {
            assert!(unsafe_on_disk(path).is_some(), "{path:?}");
        }
        for path in ["OEBPS/c1.xhtml", "a/.hidden", "a b/c", "CONSOLE", "COM10", "com", "x/NULL.txt", "图/封面.jpg", "a..b", "COM⁴", "CONIN", "a$"] {
            assert!(unsafe_on_disk(path).is_none(), "{path:?}");
        }
    }
}
