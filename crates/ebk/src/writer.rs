//! Writing an EBK file. The whole book is held in memory until `finish`.

use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::codepage::{rank, Encoder};
use crate::error::{invalid, Result};
use crate::format::*;
use crate::index::{check_path, encode, sort_paths, Block, Member, Mode};

/// Whether text is recoded with a code page made for the book (spec section 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodePage {
    /// The file is made both ways and the smaller one is kept.
    Auto,
    Never,
    /// Every member that can use the code page does, whatever it does to the size.
    Always,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Target size of a text block before compression. Blocks end at member boundaries where they can,
    /// so most are a little smaller.
    pub block_size: u64,
    /// brotli quality, 0-11.
    pub quality: u32,
    /// Blocks and resources compressed at the same time.
    pub threads: usize,
    pub code_page: CodePage,
}

impl Default for Options {
    fn default() -> Self {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        Options { block_size: DEFAULT_BLOCK_SIZE, quality: 11, threads, code_page: CodePage::Auto }
    }
}

/// Sizes of the parts of a finished file.
#[derive(Clone, Debug)]
pub struct Summary {
    pub file_len: u64,
    pub stream_len: u64,
    pub blocks_len: u64,
    pub resources_len: u64,
    pub index_len: u64,
    /// Characters in the book's code page; `None` when the file has no code page.
    pub charset_len: Option<usize>,
}

/// One way of storing the text stream: as it is, or recoded.
struct Layout<'a> {
    stream: std::borrow::Cow<'a, [u8]>,
    head_len: usize,
    chars: Option<Vec<char>>,
    /// Mode and stored length of each member of the stream, in order.
    stored: Vec<(Mode, u64)>,
}

/// What the writer may do to a resource. Whatever it tries, the member reads back byte for byte,
/// and it is stored as it is when the attempt does not make it smaller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pack {
    /// For formats that are compressed already.
    AsIs,
    Brotli,
    /// For JPEG files: recompressed with Lepton when the result restores to the same bytes.
    Jpeg,
    /// As `Jpeg`, and `Brotli` when that gives nothing.
    JpegOrBrotli,
}

enum Payload {
    InStream,
    Resource { data: Vec<u8>, pack: Pack },
}

/// Something to compress.
enum Job<'a> {
    Brotli(&'a [u8]),
    Jpeg { data: &'a [u8], or_brotli: bool },
}

struct Record {
    mode: Mode,
    path: String,
    raw_len: u64,
    stored_len: u64,
    crc32: u32,
}

pub struct Writer {
    opts: Options,
    stream: Vec<u8>,
    /// Length of the stream when `end_head` was called.
    head_len: usize,
    members: Vec<(Record, Payload)>,
    paths: HashSet<String>,
}

impl Writer {
    pub fn new(opts: Options) -> Result<Self> {
        if !(MIN_BLOCK_SIZE..=MAX_BLOCK_SIZE).contains(&opts.block_size) {
            return invalid("block size is out of range");
        }
        Ok(Writer { opts, stream: Vec::new(), head_len: 0, members: Vec::new(), paths: HashSet::new() })
    }

    /// Appends a member to the text stream. Members are read back cheapest in the order they were added.
    pub fn add_text(&mut self, path: &str, data: &[u8]) -> Result<()> {
        self.add(path, data, Mode::Stream)?;
        self.stream.extend_from_slice(data);
        self.members.last_mut().unwrap().1 = Payload::InStream;
        Ok(())
    }

    /// Adds a member stored on its own.
    pub fn add_resource(&mut self, path: &str, data: Vec<u8>, pack: Pack) -> Result<()> {
        self.add(path, &data, Mode::Raw)?;
        self.members.last_mut().unwrap().1 = Payload::Resource { data, pack };
        Ok(())
    }

    /// Marks the text added so far as what a reader needs to open the book (package document, navigation,
    /// style sheets). In a book of more than one block it gets blocks of its own.
    pub fn end_head(&mut self) {
        self.head_len = self.stream.len();
    }

    fn add(&mut self, path: &str, data: &[u8], mode: Mode) -> Result<()> {
        if let Err(why) = check_path(path) {
            return invalid(format!("{why}: {path:?}"));
        }
        if self.members.len() as u64 >= MAX_MEMBERS {
            return invalid("too many members");
        }
        if !self.paths.insert(path.to_owned()) {
            return invalid(format!("path appears twice: {path:?}"));
        }
        let (len, crc32) = (data.len() as u64, crc32fast::hash(data));
        self.members.push((Record { mode, path: path.to_owned(), raw_len: len, stored_len: len, crc32 }, Payload::InStream));
        Ok(())
    }

    /// The text stream recoded with a code page made from it, if any member has text to recode.
    fn recoded(&self) -> Option<Layout<'static>> {
        // members that are UTF-8 and not plain ASCII; ASCII would come out as it went in
        let mut texts = Vec::new();
        let mut at = 0;
        for (m, _) in self.members.iter().filter(|(m, _)| m.mode.in_stream()) {
            let data = &self.stream[at..at + m.raw_len as usize];
            at += data.len();
            texts.push(std::str::from_utf8(data).ok().filter(|text| !text.is_ascii()));
        }
        if texts.iter().all(Option::is_none) {
            return None;
        }
        let chars = rank(texts.iter().flatten().copied());
        let encoder = Encoder::new(&chars);
        let (mut stream, mut stored, mut head_len, mut at) = (Vec::with_capacity(self.stream.len()), Vec::with_capacity(texts.len()), 0, 0);
        for ((m, _), text) in self.members.iter().filter(|(m, _)| m.mode.in_stream()).zip(texts) {
            if at == self.head_len {
                head_len = stream.len();
            }
            let start = stream.len();
            match text {
                Some(text) => encoder.encode(text, &mut stream),
                None => stream.extend_from_slice(&self.stream[at..at + m.raw_len as usize]),
            }
            stored.push((if text.is_some() { Mode::StreamCoded } else { Mode::Stream }, (stream.len() - start) as u64));
            at += m.raw_len as usize;
        }
        if at == self.head_len {
            head_len = stream.len();
        }
        Some(Layout { stream: stream.into(), head_len, chars: Some(chars), stored })
    }

    pub fn finish<W: Write>(mut self, out: &mut W) -> Result<Summary> {
        let quality = self.opts.quality;
        sort_paths(self.members.len(), |i| self.members[i].0.path.as_str())?;

        let recoded = if self.opts.code_page == CodePage::Never { None } else { self.recoded() };
        let mut layouts = Vec::new();
        if recoded.is_none() || self.opts.code_page != CodePage::Always {
            let stored = self.members.iter().filter(|(m, _)| m.mode.in_stream()).map(|(m, _)| (Mode::Stream, m.raw_len)).collect();
            layouts.push(Layout { stream: (&self.stream[..]).into(), head_len: self.head_len, chars: None, stored });
        }
        layouts.extend(recoded);

        // every layout is cut into blocks and compressed, together with the resources
        let (mut jobs, mut block_counts): (Vec<Job>, Vec<usize>) = (Vec::new(), Vec::new());
        for layout in &layouts {
            let ends = layout.stored.iter().scan(0, |end, (_, len)| {
                *end += *len as usize;
                Some(*end)
            });
            let lens = plan_blocks(layout.stream.len(), ends, layout.head_len, self.opts.block_size as usize, MIN_BLOCK_LEN as usize);
            if lens.len() as u64 > MAX_BLOCKS {
                return invalid("too many text blocks");
            }
            block_counts.push(lens.len());
            let mut rest = &layout.stream[..];
            for len in lens {
                let (chunk, tail) = rest.split_at(len);
                jobs.push(Job::Brotli(chunk));
                rest = tail;
            }
        }
        let text_jobs = jobs.len();
        let mut job_member = Vec::new();
        let raw_lens: Vec<u64> = jobs.iter().map(|job| match job {
            Job::Brotli(chunk) | Job::Jpeg { data: chunk, .. } => chunk.len() as u64,
        }).collect();
        for (i, (_, payload)) in self.members.iter().enumerate() {
            match payload {
                Payload::Resource { data, pack: Pack::Brotli } => jobs.push(Job::Brotli(data)),
                Payload::Resource { data, pack: Pack::Jpeg } => jobs.push(Job::Jpeg { data, or_brotli: false }),
                Payload::Resource { data, pack: Pack::JpegOrBrotli } => jobs.push(Job::Jpeg { data, or_brotli: true }),
                _ => continue,
            }
            job_member.push(i);
        }
        // each job gives the storage mode and the bytes to store, or nothing when the member stays as it is
        let brotli = |data: &[u8]| compress(data, quality).map(|packed| Some((Mode::Brotli, packed)));
        let mut packed = in_parallel(&jobs, self.opts.threads, |job| match job {
            Job::Brotli(data) => brotli(data),
            Job::Jpeg { data, or_brotli } => {
                #[cfg(feature = "jpeg")]
                if let Some(packed) = crate::jpeg::pack(data) {
                    return Ok(Some((Mode::Jpeg, packed)));
                }
                if *or_brotli { brotli(data) } else { Ok(None) }
            }
        }).into_iter().collect::<Result<Vec<_>>>()?;
        let packed_resources = packed.split_off(text_jobs);
        drop(jobs);
        // text blocks always come back
        let packed: Vec<Vec<u8>> = packed.into_iter().flatten().map(|(_, block)| block).collect();

        for (i, smaller) in job_member.into_iter().zip(packed_resources) {
            let (member, payload) = &mut self.members[i];
            if let Some((mode, smaller)) = smaller.filter(|(_, s)| (s.len() as u64) < member.raw_len) {
                member.mode = mode;
                member.stored_len = smaller.len() as u64;
                *payload = Payload::Resource { data: smaller, pack: Pack::AsIs };
            }
        }

        // the index of each layout; the layout that gives the smaller file is kept, the plain one when they tie
        let mut best: Option<(u64, Vec<Vec<u8>>, Vec<u8>, Vec<u8>, usize)> = None;
        let (mut packed, mut raw_lens) = (packed.into_iter(), raw_lens.into_iter());
        for (n, (layout, count)) in layouts.iter().zip(block_counts).enumerate() {
            let blocks: Vec<Vec<u8>> = packed.by_ref().take(count).collect();
            let mut table = Vec::with_capacity(count);
            for (block, raw_len) in blocks.iter().zip(raw_lens.by_ref()) {
                let packed_len = block.len() as u64;
                if packed_len > max_packed_len(raw_len) {
                    return invalid("a text block grew more than the format allows when compressed");
                }
                table.push(Block { raw_len, packed_len });
            }
            let mut stored = layout.stored.iter();
            let raw_index = encode(&table, layout.chars.as_deref(), self.members.iter().map(|(m, _)| {
                let (mode, stored_len) = if m.mode.in_stream() { *stored.next().unwrap() } else { (m.mode, m.stored_len) };
                Member { path: &m.path, mode, raw_len: m.raw_len, stored_len, crc32: m.crc32 }
            }));
            let packed_index = compress(&raw_index, 11)?;
            let len = blocks.iter().map(|b| b.len() as u64).sum::<u64>() + packed_index.len() as u64;
            if best.as_ref().is_none_or(|(best_len, ..)| len < *best_len) {
                best = Some((len, blocks, raw_index, packed_index, n));
            }
        }
        let (_, blocks, raw_index, packed_index, chosen) = best.expect("there is always a layout");
        let layout = &layouts[chosen];
        let (stream_len, charset_len) = (layout.stream.len() as u64, layout.chars.as_ref().map(Vec::len));
        let (raw_index_len, index_len) = (raw_index.len() as u64, packed_index.len() as u64);
        if raw_index_len > MAX_INDEX_LEN || index_len > MAX_INDEX_LEN || index_len > max_packed_len(raw_index_len) {
            return invalid("index is too large");
        }

        let mut header = [0u8; HEADER_LEN as usize];
        header[..8].copy_from_slice(&MAGIC);
        (header[8], header[9]) = VERSION;
        out.write_all(&header)?;
        let (mut blocks_len, mut resources_len) = (0u64, 0u64);
        for block in &blocks {
            out.write_all(block)?;
            blocks_len += block.len() as u64;
        }
        for (_, payload) in &self.members {
            if let Payload::Resource { data, .. } = payload {
                out.write_all(data)?;
                resources_len += data.len() as u64;
            }
        }
        out.write_all(&packed_index)?;
        out.write_all(&(index_len as u32).to_le_bytes())?;
        out.write_all(&(raw_index_len as u32).to_le_bytes())?;
        out.write_all(&crc32fast::hash(&raw_index).to_le_bytes())?;
        out.write_all(&END_MAGIC)?;
        Ok(Summary { file_len: HEADER_LEN + blocks_len + resources_len + index_len + FOOTER_LEN, stream_len, blocks_len, resources_len, index_len, charset_len })
    }
}

/// Lengths of the text blocks for a stream of `total` bytes whose members end at `ends` (ascending).
/// A stream that fits in `target` is one block. Otherwise the first `head` bytes are cut off, and
/// a block ends before the first member that would take it past `target`; a member longer than
/// `target` is cut every `target` bytes. A cut that would leave a block shorter than `min` is not
/// made, so only the last block can be shorter: a head shorter than `min` is cut off together with
/// the members that bring it to `min`, and elsewhere the block runs on to `target` bytes.
fn plan_blocks(total: usize, ends: impl Iterator<Item = usize>, head: usize, target: usize, min: usize) -> Vec<usize> {
    let mut lens = Vec::new();
    if total <= target {
        lens.extend((total > 0).then_some(total));
        return lens;
    }
    let mut start = 0;
    let mut cut = |start: &mut usize, at: usize| {
        if at > *start {
            lens.push(at - *start);
            *start = at;
        }
    };
    let mut member_start = 0;
    let mut head_pending = head > 0;
    for member_end in ends {
        if head_pending && member_start >= head {
            if member_start - start >= min {
                cut(&mut start, member_start);
                head_pending = false;
            } else if start > 0 {
                head_pending = false; // a long member was cut just before; one more cut gains nothing
            }
        }
        if member_end - start > target {
            if member_start - start >= min {
                cut(&mut start, member_start);
            }
            while member_end - start > target {
                let at = start + target;
                cut(&mut start, at);
            }
        }
        member_start = member_end;
    }
    cut(&mut start, total);
    lens
}

/// One complete brotli stream, with the smallest window that covers the input.
fn compress(data: &[u8], quality: u32) -> Result<Vec<u8>> {
    let run = |quality: u32| -> Result<Vec<u8>> {
        let mut params = brotli::enc::BrotliEncoderParams::default();
        params.quality = quality.min(11) as i32;
        params.lgwin = (16..24).find(|&bits| (1usize << bits) - 16 >= data.len()).unwrap_or(24);
        params.size_hint = data.len();
        let mut out = Vec::new();
        brotli::BrotliCompress(&mut &data[..], &mut out, &params)?;
        Ok(out)
    };
    let out = run(quality)?;
    // at qualities 0 and 1 this encoder grows incompressible data by more than the format allows
    if quality < 2 && out.len() as u64 > max_packed_len(data.len() as u64) {
        return run(2);
    }
    Ok(out)
}

/// The results of `run` on each job, in order; several jobs at a time.
fn in_parallel<T: Sync, R: Send>(jobs: &[T], threads: usize, run: impl Fn(&T) -> R + Sync) -> Vec<R> {
    // WebAssembly has no threads to spawn
    if threads <= 1 || jobs.len() <= 1 || cfg!(target_family = "wasm") {
        return jobs.iter().map(run).collect();
    }
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads.min(jobs.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(i) else { break };
                let result = run(job);
                results.lock().unwrap()[i] = Some(result);
            });
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.expect("every job ran")).collect()
}

#[cfg(test)]
mod tests {
    use super::plan_blocks;

    fn plan_min(total: usize, ends: &[usize], head: usize, target: usize, min: usize) -> Vec<usize> {
        let lens = plan_blocks(total, ends.iter().copied(), head, target, min);
        assert_eq!(lens.iter().sum::<usize>(), total, "{ends:?} head {head}");
        assert!(lens.iter().all(|&len| (1..=target).contains(&len)), "{lens:?} from {ends:?} head {head}");
        assert!(lens.iter().rev().skip(1).all(|&len| len >= min), "{lens:?} from {ends:?} head {head}");
        lens
    }

    fn plan(total: usize, ends: &[usize], head: usize, target: usize) -> Vec<usize> {
        plan_min(total, ends, head, target, 1)
    }

    #[test]
    fn blocks_end_at_member_boundaries() {
        assert_eq!(plan(0, &[], 0, 100), []);
        assert_eq!(plan(0, &[0, 0], 0, 100), []);
        assert_eq!(plan(100, &[10, 100], 10, 100), [100]); // fits: the head is not split off
        assert_eq!(plan(250, &[10, 20, 90, 150, 250], 20, 100), [20, 70, 60, 100]);
        assert_eq!(plan(250, &[10, 20, 90, 150, 250], 0, 100), [90, 60, 100]);
        // a member longer than the target is cut; its tail shares a block with what follows
        assert_eq!(plan(360, &[30, 290, 300, 360], 0, 100), [30, 100, 100, 70, 60]);
        // a head longer than the target, an empty member at the head boundary, no member after the head
        assert_eq!(plan(250, &[120, 250, 250], 250, 100), [100, 20, 100, 30]);
        assert_eq!(plan(150, &[60, 60, 150], 60, 100), [60, 90]);
    }

    #[test]
    fn no_block_but_the_last_is_shorter_than_the_minimum() {
        // a head too short to stand alone, and the short tail of a long member before another long one
        assert_eq!(plan_min(250, &[10, 20, 90, 150, 250], 20, 100, 30), [90, 60, 100]);
        assert_eq!(plan_min(250, &[10, 20, 35, 90, 150, 250], 20, 100, 30), [35, 55, 60, 100]);
        assert_eq!(plan_min(420, &[205, 420], 0, 100, 30), [100, 100, 100, 100, 20]);
        assert_eq!(plan_min(445, &[230, 445], 0, 100, 30), [100, 100, 30, 100, 100, 15]);

        let mut state = 0x9E3779B9u32;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as usize % n
        };
        for _ in 0..20000 {
            let (target, min) = (40 + next(100), 1 + next(40));
            let mut ends = Vec::new();
            let mut total = 0;
            for _ in 0..next(12) {
                total += [0, 1, next(10), next(60), next(400)][next(5)];
                ends.push(total);
            }
            let head = if ends.is_empty() { 0 } else { ends[next(ends.len())] };
            plan_min(total, &ends, head, target, min);
        }
    }
}
