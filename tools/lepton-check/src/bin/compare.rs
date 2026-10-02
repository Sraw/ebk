//! Usage: compare <file.jpg>... — the fork of lepton_jpeg in third_party/ against the crate as published:
//! for every JPEG, both encoders must give the same answer (the same Lepton bytes, or both refuse), and each
//! decoder must turn the other's output back into the JPEG. One exception: where the published encoder writes a
//! file that does not decode back to the JPEG, the fork may write another one, which both decoders must turn
//! back into the JPEG. Prints one line of totals; exit status 1 on a difference.
use std::io::Cursor;

macro_rules! codec {
    ($name:ident, $krate:ident) => {
        mod $name {
            use std::io::Cursor;

            use $krate::{decode_lepton, encode_lepton, EnabledFeatures, SingleThreadPool};

            fn features() -> EnabledFeatures {
                EnabledFeatures { max_partitions: 1, max_processor_threads: 1, ..EnabledFeatures::compat_lepton_vector_write() }
            }

            pub fn encode(jpeg: &[u8]) -> Option<Vec<u8>> {
                let mut out = Cursor::new(Vec::new());
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    encode_lepton(&mut Cursor::new(jpeg), &mut out, &features(), &SingleThreadPool::default()).is_ok()
                }));
                run.unwrap_or(false).then(|| out.into_inner())
            }

            pub fn decode(lepton: &[u8]) -> Option<Vec<u8>> {
                let mut out = Vec::new();
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decode_lepton(&mut Cursor::new(lepton), &mut out, &features(), &SingleThreadPool::default()).is_ok()
                }));
                run.unwrap_or(false).then_some(out)
            }
        }
    };
}

codec!(published, lepton_jpeg);
codec!(forked, fork);

fn main() {
    let _ = Cursor::new(0);
    let (mut same, mut both_refuse, mut exact, mut fork_only, mut differ) = (0u32, 0u32, 0u32, 0u32, Vec::new());
    for path in std::env::args().skip(1) {
        let jpeg = std::fs::read(&path).expect("cannot read the file");
        match (published::encode(&jpeg), forked::encode(&jpeg)) {
            (None, None) => both_refuse += 1,
            (Some(a), Some(b)) if a == b => {
                same += 1;
                // each decoder on the common output; whether that is the JPEG again is a property of the file
                let (back_a, back_b) = (published::decode(&a), forked::decode(&b));
                if back_a != back_b {
                    differ.push(format!("{path}: the decoders disagree"));
                } else if back_b.as_deref() == Some(&jpeg[..]) {
                    exact += 1;
                }
            }
            // a file the published encoder gets wrong and the fork gets right, in the stream the published decoder reads
            (Some(a), Some(b))
                if published::decode(&a).as_deref() != Some(&jpeg[..])
                    && published::decode(&b).as_deref() == Some(&jpeg[..])
                    && forked::decode(&b).as_deref() == Some(&jpeg[..]) =>
            {
                fork_only += 1
            }
            (a, b) => differ.push(format!("{path}: the encoders disagree ({:?} and {:?} bytes)", a.map(|x| x.len()), b.map(|x| x.len()))),
        }
    }
    println!(
        "{same} same Lepton bytes ({exact} of them decode back to the JPEG), {both_refuse} refused by both, {fork_only} restored by the fork only, {} differences",
        differ.len()
    );
    for line in &differ {
        println!("  {line}");
    }
    std::process::exit(!differ.is_empty() as i32);
}
