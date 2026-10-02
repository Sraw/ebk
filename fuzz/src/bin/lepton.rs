//! The input is a Lepton file, given to the codec of third_party/lepton_jpeg directly, with the settings the reader
//! uses but without the reader's own checks in front (declared length, ratio of the lengths), so that damaged
//! files reach further into the decoder. A panic, an overflow, running out of memory or time is a failure.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| ebk_fuzz::decode_lepton_file(data));
