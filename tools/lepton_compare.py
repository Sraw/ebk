#!/usr/bin/env python3
"""The fork of lepton_jpeg in third_party/ must write and read the same coded stream as the published crate.
Every JPEG file of every EPUB in the given directories goes through both (tools/lepton-check, binary `compare`),
and so does every *.jpg file that lies in such a directory itself.

Usage: .venv/bin/python tools/lepton_compare.py <corpus_dir>...
Run it after any change to third_party/lepton_jpeg. Exit status 0 when there is no difference.
"""
import os, subprocess, sys, tempfile, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))


def main():
    crate = os.path.join(HERE, "lepton-check")
    subprocess.run(["cargo", "build", "--quiet", "--release", "--bin", "compare"], cwd=crate, check=True)
    binary = os.path.join(crate, "target", "release", "compare")
    totals, failed, files = [0, 0, 0, 0, 0], False, 0
    for directory in sys.argv[1:]:
        loose = sorted(os.path.join(directory, n) for n in os.listdir(directory) if n.endswith(".jpg"))
        if loose:
            files += len(loose)
            run = subprocess.run([binary] + loose, capture_output=True, text=True)
            numbers = [int(w) for w in run.stdout.splitlines()[0].replace("(", " ").split() if w.isdigit()] if run.stdout else []
            if run.returncode or len(numbers) != 5:
                failed = True
                print(f"{directory}: {run.stdout.strip() or run.stderr.strip()[-300:]}")
            else:
                totals = [a + b for a, b in zip(totals, numbers)]
        for name in sorted(os.listdir(directory)):
            if not name.endswith(".epub"):
                continue
            with tempfile.TemporaryDirectory() as tmp, zipfile.ZipFile(os.path.join(directory, name)) as z:
                paths = []
                for n, info in enumerate(z.infolist()):
                    if info.file_size >= 4:
                        data = z.read(info)
                        if data[:3] == b"\xff\xd8\xff":
                            paths.append(os.path.join(tmp, f"{n}.jpg"))
                            open(paths[-1], "wb").write(data)
                if not paths:
                    continue
                files += len(paths)
                run = subprocess.run([binary] + paths, capture_output=True, text=True)
                line = run.stdout.splitlines()[0] if run.stdout else run.stderr[-200:]
                numbers = [int(w) for w in line.replace("(", " ").split() if w.isdigit()]
                if run.returncode or len(numbers) != 5:
                    failed = True
                    print(f"{name}: {run.stdout.strip() or run.stderr.strip()[-300:]}")
                else:
                    totals = [a + b for a, b in zip(totals, numbers)]
    print(f"{files} JPEG files: {totals[0]} give the same Lepton bytes in the fork and in the published crate "
          f"({totals[1]} of them decode back to the JPEG), {totals[2]} are refused by both, "
          f"{totals[3]} are restored by the fork only, {totals[4]} differences")
    return 1 if failed or totals[4] else 0


if __name__ == "__main__":
    sys.exit(main())
