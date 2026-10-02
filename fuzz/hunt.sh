#!/bin/sh
# Usage: fuzz/hunt.sh <target> <seconds> [workers]
# Like run.sh, but does not stop at the first failure: it keeps fuzzing and collects every failing input, then
# says where each one fails. For working through a target that has many defects; run.sh is the pass/fail check.
set -e
here=$(cd "$(dirname "$0")" && pwd)
target=$1; seconds=$2; workers=${3:-4}
flags="--cfg fuzzing -Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 -Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table -Cllvm-args=-sanitizer-coverage-trace-compares -Ccodegen-units=1"
RUSTFLAGS="$flags" cargo build --quiet --release --manifest-path "$here/Cargo.toml" --target x86_64-unknown-linux-gnu --bin "$target"
bin="$here/target/x86_64-unknown-linux-gnu/release/$target"
found="$here/artifacts/$target-hunt"
rm -rf "$found"; mkdir -p "$here/corpus/$target" "$found"
cd "$found"
"$bin" "$here/corpus/$target" -artifact_prefix="$found/" -max_len=65536 -timeout=10 -rss_limit_mb=3072 -malloc_limit_mb=1024 \
    -max_total_time="$seconds" -fork="$workers" -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1 > "$found/fuzz.log" 2>&1 || true
# where each input fails: the place of the panic, or the kind of failure
for input in "$found"/crash-* "$found"/timeout-* "$found"/oom-*; do
    [ -f "$input" ] || continue
    where=$(timeout 30 "$bin" "$input" 2>&1 | grep -m1 -o "panicked at [^:]*:[0-9]*\|memory allocation of [0-9]* bytes failed\|out-of-memory\|timeout" || echo "other")
    echo "$where	$(basename "$input")"
done | sort > "$found/where.tsv"
echo "$(ls "$found" | grep -c '^crash-\|^timeout-\|^oom-') failing inputs:"
cut -f1 "$found/where.tsv" | sort | uniq -c | sort -rn
