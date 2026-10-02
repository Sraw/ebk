//! Writes a ZIP file of the plain kind: no 64-bit extension, no data descriptors, names in UTF-8.

use std::io::Write;

use anyhow::{bail, Result};
use flate2::{write::DeflateEncoder, Compression};

struct Entry {
    name: Vec<u8>,
    method: u16,
    crc32: u32,
    packed_len: u32,
    raw_len: u32,
    offset: u32,
}

pub struct ZipWriter<W: Write> {
    out: W,
    at: u64,
    entries: Vec<Entry>,
}

/// 1 January 1980, the earliest date the format has: the members of an EBK file have no dates.
const DOS_DATE: u16 = 0x0021;
/// Names are UTF-8.
const FLAGS: u16 = 0x0800;

impl<W: Write> ZipWriter<W> {
    pub fn new(out: W) -> Self {
        ZipWriter { out, at: 0, entries: Vec::new() }
    }

    /// Adds a file. With `deflate` it is compressed when that makes it smaller.
    pub fn add(&mut self, name: &str, data: &[u8], deflate: bool) -> Result<()> {
        let too_large = || anyhow::anyhow!("the book is larger than a ZIP file without the 64-bit extension holds");
        let raw_len = u32::try_from(data.len()).map_err(|_| too_large())?;
        let offset = u32::try_from(self.at).map_err(|_| too_large())?;
        if self.entries.len() == usize::from(u16::MAX) || name.len() > usize::from(u16::MAX) {
            bail!("the book has more files than a ZIP file without the 64-bit extension holds");
        }
        let mut packed = Vec::new();
        if deflate {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data)?;
            packed = encoder.finish()?;
        }
        let (method, body) = if deflate && packed.len() < data.len() { (8, &packed[..]) } else { (0, data) };
        let entry = Entry { name: name.as_bytes().to_vec(), method, crc32: crc32fast::hash(data), packed_len: body.len() as u32, raw_len, offset };

        let mut header = Vec::with_capacity(30 + entry.name.len());
        header.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        header.extend_from_slice(&20u16.to_le_bytes());
        entry.common_fields(&mut header);
        header.extend_from_slice(&0u16.to_le_bytes()); // no extra field
        header.extend_from_slice(&entry.name);
        self.out.write_all(&header)?;
        self.out.write_all(body)?;
        self.at += (header.len() + body.len()) as u64;
        self.entries.push(entry);
        Ok(())
    }

    /// Writes the central directory and gives the writer back.
    pub fn finish(mut self) -> Result<W> {
        let start = u32::try_from(self.at).map_err(|_| anyhow::anyhow!("the book is larger than a ZIP file without the 64-bit extension holds"))?;
        let mut directory = Vec::new();
        for entry in &self.entries {
            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes()); // made by
            directory.extend_from_slice(&20u16.to_le_bytes()); // needed to extract
            entry.common_fields(&mut directory);
            directory.extend_from_slice(&[0; 12]); // no extra field, no comment, disk 0, no attributes
            directory.extend_from_slice(&entry.offset.to_le_bytes());
            directory.extend_from_slice(&entry.name);
        }
        let count = self.entries.len() as u16;
        let directory_len = u32::try_from(directory.len()).map_err(|_| anyhow::anyhow!("the directory of the ZIP file is too large"))?;
        directory.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        directory.extend_from_slice(&[0; 4]); // this disk, the disk of the directory
        directory.extend_from_slice(&count.to_le_bytes());
        directory.extend_from_slice(&count.to_le_bytes());
        directory.extend_from_slice(&directory_len.to_le_bytes());
        directory.extend_from_slice(&start.to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes()); // no comment
        self.out.write_all(&directory)?;
        Ok(self.out)
    }
}

impl Entry {
    /// What the local header and the directory entry have in common, from the flags to the length of the name.
    fn common_fields(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&FLAGS.to_le_bytes());
        out.extend_from_slice(&self.method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&self.crc32.to_le_bytes());
        out.extend_from_slice(&self.packed_len.to_le_bytes());
        out.extend_from_slice(&self.raw_len.to_le_bytes());
        out.extend_from_slice(&(self.name.len() as u16).to_le_bytes());
    }
}
