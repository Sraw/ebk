//! The input is a whole file.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| ebk_fuzz::open_and_read(data, false));
