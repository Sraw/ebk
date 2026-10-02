//! The per-book code page, layout 1 (spec section 6): the 64 most frequent non-ASCII characters of
//! a book take one byte, the next 8064 two bytes, any other four.

use crate::format::MAX_CHARSET;

/// Characters with a one-byte code.
const SHORT: usize = 64;
const ESCAPE: u8 = 0xFF;

/// The characters of a book's code page; the position of a character is its rank.
#[derive(Debug)]
pub(crate) struct Table {
    chars: Vec<char>,
    /// The same characters in code point order, to tell whether a character has a rank.
    sorted: Vec<char>,
}

pub(crate) enum DecodeError {
    Memory,
    Data(&'static str),
}

impl Table {
    /// `chars` holds no ASCII character, no character twice, and at most `MAX_CHARSET` characters.
    pub fn new(chars: Vec<char>, sorted: Vec<char>) -> Table {
        debug_assert!(chars.len() <= MAX_CHARSET && chars.len() == sorted.len());
        Table { chars, sorted }
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// Turns the stored bytes of a member back into its UTF-8 text, which must be `raw_len` bytes long.
    pub fn decode(&self, stored: &[u8], raw_len: u64) -> Result<Vec<u8>, DecodeError> {
        const CUT: DecodeError = DecodeError::Data("coded text ends inside a character");
        const TRAIL: DecodeError = DecodeError::Data("coded text has a second byte below 0x80");
        const LENGTH: DecodeError = DecodeError::Data("decoded text does not have the declared length");
        // the index was checked for raw_len <= 4 * stored length, so this is in proportion to data that exists
        let raw_len = usize::try_from(raw_len).map_err(|_| DecodeError::Memory)?;
        let mut out = Vec::new();
        out.try_reserve_exact(raw_len).map_err(|_| DecodeError::Memory)?;
        let mut rest = stored;
        while let Some((&b, tail)) = rest.split_first() {
            // text is mostly runs of ASCII (markup) and runs of coded characters
            if b < 0x80 {
                let run = rest.iter().position(|&b| b >= 0x80).unwrap_or(rest.len());
                if run > raw_len - out.len() {
                    return Err(LENGTH);
                }
                out.extend_from_slice(&rest[..run]);
                rest = &rest[run..];
                continue;
            }
            let c = if b < 0xC0 {
                rest = tail;
                self.chars.get(usize::from(b - 0x80)).copied()
            } else if b < ESCAPE {
                let Some((&t, tail)) = tail.split_first() else { return Err(CUT) };
                if t < 0x80 {
                    return Err(TRAIL);
                }
                rest = tail;
                self.chars.get(SHORT + usize::from(b - 0xC0) * 128 + usize::from(t - 0x80)).copied()
            } else {
                let Some((&[x, y, z], tail)) = tail.split_first_chunk::<3>() else { return Err(CUT) };
                if x < 0x80 || y < 0x80 || z < 0x80 {
                    return Err(TRAIL);
                }
                rest = tail;
                let code = u32::from(x - 0x80) << 14 | u32::from(y - 0x80) << 7 | u32::from(z - 0x80);
                let Some(c) = char::from_u32(code).filter(|&c| c > '\u{7f}') else {
                    return Err(DecodeError::Data("coded text has a four-byte form that is not a non-ASCII character"));
                };
                if self.sorted.binary_search(&c).is_ok() {
                    return Err(DecodeError::Data("coded text spells out a character that has a code"));
                }
                Some(c)
            };
            let Some(c) = c else { return Err(DecodeError::Data("coded text uses a code beyond the character table")) };
            // never past the reserved length: the buffer does not grow
            if c.len_utf8() > raw_len - out.len() {
                return Err(LENGTH);
            }
            out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        }
        if out.len() != raw_len {
            return Err(LENGTH);
        }
        Ok(out)
    }
}

/// The character table for a book: its non-ASCII characters, most frequent first, equally frequent
/// ones in code point order; at most `MAX_CHARSET` of them.
#[cfg(feature = "write")]
pub(crate) fn rank<'a>(texts: impl Iterator<Item = &'a str>) -> Vec<char> {
    let mut count = vec![0u32; 0x11_0000];
    for text in texts {
        for c in text.chars().filter(|&c| c > '\u{7f}') {
            let n = &mut count[c as usize];
            *n = n.saturating_add(1);
        }
    }
    let mut chars: Vec<char> = (0x80..0x11_0000).filter(|&c| count[c as usize] > 0).filter_map(char::from_u32).collect();
    chars.sort_by_key(|&c| (std::cmp::Reverse(count[c as usize]), c));
    chars.truncate(MAX_CHARSET);
    chars
}

/// Encodes texts with a character table.
#[cfg(feature = "write")]
pub(crate) struct Encoder {
    /// Rank of each character, or `NONE`.
    rank: Vec<u16>,
}

#[cfg(feature = "write")]
impl Encoder {
    const NONE: u16 = u16::MAX;

    pub fn new(chars: &[char]) -> Encoder {
        let mut rank = vec![Self::NONE; 0x11_0000];
        for (r, &c) in chars.iter().enumerate() {
            rank[c as usize] = r as u16; // at most MAX_CHARSET
        }
        Encoder { rank }
    }

    pub fn encode(&self, text: &str, out: &mut Vec<u8>) {
        for c in text.chars() {
            let code = c as u32;
            if code < 0x80 {
                out.push(code as u8);
                continue;
            }
            match usize::from(self.rank[code as usize]) {
                r if r < SHORT => out.push(0x80 + r as u8),
                r if r < MAX_CHARSET => out.extend_from_slice(&[0xC0 + ((r - SHORT) / 128) as u8, 0x80 + ((r - SHORT) % 128) as u8]),
                _ => out.extend_from_slice(&[ESCAPE, 0x80 | (code >> 14) as u8, 0x80 | (code >> 7 & 0x7F) as u8, 0x80 | (code & 0x7F) as u8]),
            }
        }
    }
}

#[cfg(all(test, feature = "write"))]
mod tests {
    use super::*;

    fn table(chars: &[char]) -> Table {
        let mut sorted = chars.to_vec();
        sorted.sort_unstable();
        Table::new(chars.to_vec(), sorted)
    }

    fn roundtrip(text: &str) -> Vec<u8> {
        let chars = rank([text].into_iter());
        let mut coded = Vec::new();
        Encoder::new(&chars).encode(text, &mut coded);
        let decoded = table(&chars).decode(&coded, text.len() as u64);
        assert!(decoded.is_ok_and(|d| d == text.as_bytes()), "{text:?}");
        coded
    }

    #[test]
    fn each_form_has_the_bytes_the_specification_gives() {
        // ranks: 的 0 (three times), 一 1 (twice), then by code point
        assert_eq!(rank(["a的的的一一é😀"].into_iter()), ['的', '一', 'é', '😀']);
        assert_eq!(roundtrip("a的的的一一é😀"), [b'a', 0x80, 0x80, 0x80, 0x81, 0x81, 0x82, 0x83]);

        // 70 characters: ranks 64 and up take two bytes
        let many: String = (0..70).map(|i| char::from_u32(0x4E00 + i).unwrap()).collect();
        let coded = roundtrip(&many);
        assert_eq!(coded.len(), 64 + 6 * 2);
        assert_eq!(coded[64..68], [0xC0, 0x80, 0xC0, 0x81]);

        // the last rank, 8127, is FE FF; characters past the table are spelled out
        let full: String = (0..MAX_CHARSET as u32 + 2).map(|i| char::from_u32(0x1_0000 + i).unwrap()).collect();
        let coded = roundtrip(&full);
        assert_eq!(coded.len(), 64 + (MAX_CHARSET - 64) * 2 + 2 * 4);
        assert_eq!(coded[coded.len() - 10..coded.len() - 8], [0xFE, 0xFF]);
        let last = 0x1_0000 + MAX_CHARSET as u32 + 1;
        assert_eq!(coded[coded.len() - 4..], [0xFF, 0x80 | (last >> 14) as u8, 0x80 | (last >> 7 & 0x7F) as u8, 0x80 | (last & 0x7F) as u8]);
        roundtrip("");
        roundtrip("plain ASCII\n");
        roundtrip("\u{80}\u{7ff}\u{800}\u{ffff}\u{10000}\u{10ffff}");
    }

    #[test]
    fn bad_coded_text_is_an_error() {
        let t = table(&['的', '一']);
        let bad = |stored: &[u8], raw_len: u64| matches!(t.decode(stored, raw_len), Err(DecodeError::Data(_)));
        assert!(t.decode(&[0x80, b'a', 0x81], 7).is_ok_and(|d| d == "的a一".as_bytes()));
        assert!(bad(&[0x80, b'a', 0x81], 6), "longer than declared");
        assert!(bad(&[0x80, b'a', 0x81], 8), "shorter than declared");
        assert!(bad(&[0x80, b'a', b'b'], 4), "an ASCII run longer than declared");
        assert!(bad(&[0x82], 3), "rank beyond the table");
        assert!(bad(&[0xC0, 0x80], 3), "two-byte rank beyond the table");
        assert!(bad(&[0xC0], 3), "ends inside a two-byte form");
        assert!(bad(&[0xC0, 0x7F], 3), "second byte below 0x80");
        assert!(bad(&[0xFF, 0x80, 0x81], 2), "ends inside a four-byte form");
        assert!(bad(&[0xFF, 0x80, 0x81, 0x7F], 2), "fourth byte below 0x80");
        assert!(t.decode(&[0xFF, 0x80, 0x81, 0xE9], 2).is_ok_and(|d| d == "é".as_bytes()));
        assert!(bad(&[0xFF, 0x80, 0x80, 0xE9], 1), "ASCII in the four-byte form");
        assert!(bad(&[0xFF, 0x83, 0xB0, 0x80], 3), "a surrogate");
        assert!(bad(&[0xFF, 0xC4, 0x80, 0x80], 4), "beyond U+10FFFF");
        let de = '的' as u32;
        assert!(bad(&[0xFF, 0x80 | (de >> 14) as u8, 0x80 | (de >> 7 & 0x7F) as u8, 0x80 | (de & 0x7F) as u8], 3), "a character that has a code");
        assert!(bad(&[0xFF, 0x80, 0x81, 0xE9], 1), "a four-byte form longer than declared");
    }
}
