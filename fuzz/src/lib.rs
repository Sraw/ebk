//! What the fuzz targets share.

use ebk::{Error, Mode, Reader};

/// Members larger than this are refused by the reader under test.
const MEMBER_LIMIT: u64 = 32 << 20;
/// Reading stops once this much has come out: a small input can declare gigabytes.
const TOTAL_LIMIT: u64 = 128 << 20;

/// Opens a file and reads its members. Any outcome is fine except a panic.
/// Recompressed JPEG files are read only with `jpeg`: their decoder is a codebase of its own (target `jpeg`).
pub fn open_and_read(file: &[u8], jpeg: bool) {
    let mut reader = match Reader::open(file) {
        Ok(reader) => reader,
        Err(Error::Invalid(_) | Error::TooLarge(_)) => return,
        Err(e) => panic!("open: unexpected error kind {e:?}"),
    };
    reader.set_member_limit(MEMBER_LIMIT);
    reader.set_pixel_limit(1 << 22);
    let count = reader.member_count();
    assert!(reader.member(count).is_none());
    let mut total = 0u64;
    for i in 0..count {
        let m = reader.member(i).unwrap();
        if m.mode == Mode::Jpeg && !jpeg {
            continue;
        }
        let (path, raw_len, crc32, readable) = (m.path.to_owned(), m.raw_len, m.crc32, reader.readable(i));
        assert_eq!(reader.find(&path), Some(i), "{path:?} is not found under its own name");
        if readable {
            total = total.saturating_add(raw_len.min(MEMBER_LIMIT));
            if total > TOTAL_LIMIT {
                break;
            }
        }
        match reader.read(i) {
            Ok(data) => {
                assert!(readable && data.len() as u64 == raw_len && crc32fast::hash(&data) == crc32, "{path:?} read back wrong");
                // a second read gives the same bytes
                if i % 7 == 0 {
                    assert!(reader.read(i).is_ok_and(|again| again == data));
                }
            }
            Err(Error::Corrupt(_) | Error::TooLarge(_)) => assert!(readable),
            Err(Error::Unsupported(_)) => assert!(!readable),
            Err(e) => panic!("read: unexpected error kind {e:?}"),
        }
    }
    assert!(matches!(reader.read(count), Err(Error::NoSuchMember(_))));
}

/// A file with the given data area and index: the index is compressed and the footer filled in.
pub fn wrap(body: &[u8], index: &[u8]) -> Vec<u8> {
    let mut params = brotli::enc::BrotliEncoderParams::default();
    params.quality = 1;
    params.lgwin = 22;
    let mut packed = Vec::new();
    brotli::BrotliCompress(&mut &index[..], &mut packed, &params).unwrap();
    let mut file = b"\x89EBK\r\n\x1a\n".to_vec();
    file.extend_from_slice(&[ebk::VERSION.0, ebk::VERSION.1, 0, 0, 0, 0, 0, 0]);
    file.extend_from_slice(body);
    file.extend_from_slice(&packed);
    file.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    file.extend_from_slice(&(index.len() as u32).to_le_bytes());
    file.extend_from_slice(&crc32fast::hash(index).to_le_bytes());
    file.extend_from_slice(b"EBK\x1a");
    file
}

/// The input of the `index` target: two bytes with the length of the data area, the data area, the index as it is before compression.
pub fn split(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let (len, rest) = data.split_first_chunk::<2>()?;
    let len = u16::from_le_bytes(*len) as usize;
    (len <= rest.len()).then(|| rest.split_at(len))
}

/// A file with one member: a recompressed JPEG of `raw_len` bytes, stored as `stored`.
pub fn one_jpeg(raw_len: u32, stored: &[u8]) -> Vec<u8> {
    fn varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push(v as u8 | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }
    let mut members = vec![1, 4, 1, b'j'];
    varint(&mut members, u64::from(raw_len));
    varint(&mut members, stored.len() as u64);
    members.extend_from_slice(&[0; 4]); // any checksum: what matters is what the decoder does
    let mut index = vec![1, 1, 0, 3];
    varint(&mut index, members.len() as u64);
    index.extend_from_slice(&members);
    wrap(stored, &index)
}

/// Takes what the Lepton decoder writes, up to a megabyte.
struct Sink(usize);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        if self.0 > 1 << 20 {
            return Err(std::io::Error::other("more than the file may be"));
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A Lepton file given to the codec of third_party/lepton_jpeg directly, with the settings the reader uses but
/// without the reader's own checks in front (declared length, ratio of the lengths).
pub fn decode_lepton_file(file: &[u8]) {
    use lepton_jpeg::{decode_lepton, EnabledFeatures, SingleThreadPool};
    let features = EnabledFeatures { max_jpeg_file_size: 1 << 20, max_jpeg_pixels: 1 << 20, max_processor_threads: 1, ..EnabledFeatures::compat_lepton_vector_write() };
    let _ = decode_lepton(&mut std::io::Cursor::new(file), &mut Sink(0), &features, &SingleThreadPool::default());
}
