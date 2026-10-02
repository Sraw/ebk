#!/bin/sh
# Usage: fuzz/run.sh <raw|index|roundtrip|epub|jpeg|lepton|lepton_header|jpeg_encode> <seconds> [workers]
# Builds one target with coverage instrumentation and runs it; corpora and findings are kept under fuzz/corpus/<target>
# and fuzz/artifacts/<target>. Exit status 0 means no crash, timeout or memory limit was hit.
set -e
here=$(cd "$(dirname "$0")" && pwd)
target=$1; seconds=$2; workers=${3:-4}
flags="--cfg fuzzing -Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 -Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table -Cllvm-args=-sanitizer-coverage-trace-compares -Ccodegen-units=1"
RUSTFLAGS="$flags" cargo build --quiet --release --manifest-path "$here/Cargo.toml" --target x86_64-unknown-linux-gnu --bin "$target"
mkdir -p "$here/corpus/$target" "$here/artifacts/$target"
# inputs that crashed an earlier version must pass before anything new is looked for
if [ -d "$here/regressions/$target" ]; then
    "$here/target/x86_64-unknown-linux-gnu/release/$target" "$here"/regressions/"$target"/* > /dev/null 2>&1 || { echo "a regression input of $target fails"; exit 1; }
fi
cd "$here/artifacts/$target"
exec "$here/target/x86_64-unknown-linux-gnu/release/$target" "$here/corpus/$target" \
    -artifact_prefix="$here/artifacts/$target/" -max_len=65536 -timeout=10 -rss_limit_mb=3072 -malloc_limit_mb=1024 \
    -max_total_time="$seconds" -jobs="$workers" -workers="$workers" -print_final_stats=1
