//! Usage: why <file.jpg>... — what the fork of lepton_jpeg in third_party/ does with each JPEG under the settings
//! the ebk crate uses: one line per file with the outcome, tab separated — `stored` (with the Lepton length),
//! `refused` (with the codec's error), `changed` (decodes, but not to the same bytes) or `unreadable` (the codec
//! does not read its own output under the reader's limits). With WHY_KEEP=<dir> the Lepton files are written there.
use std::io::Cursor;

use fork::{decode_lepton, encode_lepton, EnabledFeatures, SingleThreadPool};

fn main() {
    for path in std::env::args().skip(1) {
        let jpeg = std::fs::read(&path).expect("cannot read the file");
        let write = EnabledFeatures { max_partitions: 1, max_processor_threads: 1, ..EnabledFeatures::compat_lepton_vector_write() };
        let mut packed = Cursor::new(Vec::new());
        let outcome = match encode_lepton(&mut Cursor::new(&jpeg), &mut packed, &write, &SingleThreadPool::default()) {
            Err(e) => format!("refused\t{:?}: {}", e.exit_code(), e.message()),
            Ok(_) => {
                let packed = packed.into_inner();
                let read = EnabledFeatures { max_jpeg_file_size: jpeg.len() as u32, max_jpeg_pixels: 1 << 26, ..write };
                let mut back = Vec::new();
                match decode_lepton(&mut Cursor::new(&packed), &mut back, &read, &SingleThreadPool::default()) {
                    Err(e) => format!("unreadable\t{:?}: {}", e.exit_code(), e.message()),
                    Ok(_) if back != jpeg => {
                        let at = back.iter().zip(&jpeg).position(|(a, b)| a != b).unwrap_or(back.len().min(jpeg.len()));
                        format!("changed\t{} bytes; {} bytes come back, the first difference is at {at}", packed.len(), back.len())
                    }
                    Ok(_) => format!("stored\t{}", packed.len()),
                }
            }
        };
        if let Ok(dir) = std::env::var("WHY_KEEP") {
            // the Lepton file, for looking at what the encoder wrote
            let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
            let mut kept = Cursor::new(Vec::new());
            if encode_lepton(&mut Cursor::new(&jpeg), &mut kept, &write, &SingleThreadPool::default()).is_ok() {
                std::fs::write(std::path::Path::new(&dir).join(name + ".lep"), kept.into_inner()).unwrap();
            }
        }
        println!("{path}\t{}\t{}", jpeg.len(), outcome.replace('\n', " "));
    }
}
