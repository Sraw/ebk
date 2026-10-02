//! Usage: unlepton <file.lep>... — writes each decoded JPEG next to its input as <file.lep>.jpg.
//! Exit status 1 when a file does not decode; its name is printed.
use std::io::Cursor;

use lepton_jpeg::{decode_lepton, EnabledFeatures, SingleThreadPool};

fn main() {
    let mut failed = false;
    for path in std::env::args().skip(1) {
        let lep = std::fs::read(&path).expect("cannot read the input");
        let mut jpeg = Vec::new();
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_lepton(&mut Cursor::new(&lep), &mut jpeg, &EnabledFeatures::compat_lepton_vector_read(), &SingleThreadPool::default()).is_ok()
        }));
        if decoded.unwrap_or(false) {
            std::fs::write(format!("{path}.jpg"), &jpeg).expect("cannot write the output");
        } else {
            println!("{path}");
            failed = true;
        }
    }
    std::process::exit(failed as i32);
}
