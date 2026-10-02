#!/usr/bin/env python3
"""Do the two readers agree? Every .ebk file of a directory (for example the work directory of
tools/ebk_vectors.py, which is full of files that break one rule each) is given to the Rust reader
(`ebk verify`) and to the Python reader of tools/ebk_check.py; a file must be "every member reads
back" for both or for neither.

Usage: .venv/bin/python tools/ebk_differential.py <ebk_binary> <dir>
The Python reader has no limit on its output, so it runs in a process of its own with 3 GiB of
address space and two minutes; the Rust reader gets a limit of 256 MiB of output. A file on which
either reader runs into its limit (the decompression bombs) is counted apart.
"""
import os, resource, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
READ = "import sys; sys.path.insert(0, sys.argv[1]); import ebk_check; ebk_check.read_ebk(open(sys.argv[2], 'rb').read())"


def python_verdict(path):
    limit = lambda: resource.setrlimit(resource.RLIMIT_AS, (3 << 30, 3 << 30))
    try:
        run = subprocess.run([sys.executable, "-c", READ, HERE, path], capture_output=True, text=True, timeout=120, preexec_fn=limit)
    except subprocess.TimeoutExpired:
        return "limit"
    if run.returncode == 0:
        return "reads"
    return "limit" if "MemoryError" in run.stderr or run.returncode < 0 else "refuses"


def rust_verdict(binary, path):
    run = subprocess.run([binary, "verify", path, "--max-output", str(1 << 28)], capture_output=True, text=True)
    if run.returncode == 0:
        return "reads"
    return "limit" if "raise --max-output" in run.stderr else "refuses"


def main():
    binary, directory = sys.argv[1], sys.argv[2]
    same, different, limited = 0, [], []
    for name in sorted(os.listdir(directory)):
        path = os.path.join(directory, name)
        if not name.endswith(".ebk") or not os.path.isfile(path):
            continue
        rust, python = rust_verdict(binary, path), python_verdict(path)
        if "limit" in (rust, python):
            limited.append(name)
        elif rust == python:
            same += 1
        else:
            different.append(f"{name}: rust {rust}, python {python}")
    print(f"{same} files with the same verdict, {len(different)} with different verdicts, {len(limited)} over a reader's limit ({', '.join(limited)})")
    for line in different:
        print("  " + line)
    return 1 if different else 0


if __name__ == "__main__":
    sys.exit(main())
