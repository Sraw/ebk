//! The input is the declared length of a JPEG (four bytes) and the bytes stored for it: a Lepton file, or what is left of one.
//!
//! The codec is this repository's own copy (third_party/lepton_jpeg), so everything counts as a failure here:
//! a panic inside it (the reader would catch it natively, but in WebAssembly it is a trap), arithmetic that
//! overflows (the build checks it), running out of memory, not finishing.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Some((len, stored)) = data.split_first_chunk::<4>() {
        ebk_fuzz::open_and_read(&ebk_fuzz::one_jpeg(u32::from_le_bytes(*len), stored), true);
    }
});
