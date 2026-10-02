//! Reader and writer for the EBK e-book container (`spec/ebk-format-1.0.md`).
//!
//! An EBK file holds the members of an EPUB under their original paths. Text members are
//! concatenated into a stream that is cut into independently compressed blocks; other members
//! are stored one by one. Every member read back is checked against its length and CRC-32.
//!
//! Features: `write` (the writer), `jpeg` (storage mode 4, JPEG files recompressed with Lepton). Both are on by
//! default. A reader built without `jpeg` reports such members as unsupported and does not conform to the
//! specification, which asks readers for all five storage modes.

mod codepage;
mod error;
mod format;
mod index;
#[cfg(feature = "jpeg")]
mod jpeg;
mod reader;
#[cfg(feature = "write")]
mod writer;

pub use error::{Error, Result};
pub use format::{DEFAULT_BLOCK_SIZE, DEFAULT_PIXEL_LIMIT, MAX_BLOCK_SIZE, MAX_JPEG_LEN, MAX_MEMBER_LEN, MIN_BLOCK_LEN, MIN_BLOCK_SIZE, VERSION};
pub use index::{check_path, Block, Member, Mode};
#[cfg(any(unix, windows))]
pub use reader::FileSource;
pub use reader::{Reader, Source};
#[cfg(feature = "write")]
pub use writer::{CodePage, Options, Pack, Summary, Writer};
