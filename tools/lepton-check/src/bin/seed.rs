//! Usage: seed <out_dir> <file.jpg>... — for each JPEG the encoder of the fork takes, writes an input of the `jpeg`
//! fuzz target: the length of the JPEG (four bytes, little-endian) and the Lepton file. Files the encoder refuses
//! are skipped. Used by fuzz/jpeg_seeds.py.
use std::io::Cursor;

use fork::{encode_lepton, EnabledFeatures, SingleThreadPool};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("output directory");
    let (mut taken, mut refused) = (0, 0);
    for path in args {
        let jpeg = std::fs::read(&path).unwrap();
        let mut lepton = Cursor::new(Vec::new());
        match encode_lepton(&mut Cursor::new(&jpeg), &mut lepton, &EnabledFeatures::compat_lepton_vector_write(), &SingleThreadPool::default()) {
            Ok(_) => {
                let name = std::path::Path::new(&path).file_stem().unwrap().to_string_lossy().into_owned();
                let seed = [&(jpeg.len() as u32).to_le_bytes()[..], &lepton.into_inner()].concat();
                std::fs::write(format!("{out}/seed-{name}"), seed).unwrap();
                taken += 1;
            }
            Err(_) => refused += 1,
        }
    }
    println!("{taken} seeds written, {refused} files refused by the encoder");
}
