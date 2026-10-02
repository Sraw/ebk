# EBK

EBK is a reading format for e-books, made from EPUB. An `.ebk` file holds the files of an EPUB under their
original paths and gives each one back byte for byte; a reading system reads them by path and renders them with
an EPUB renderer. What changes is the container:

- text is joined and compressed in blocks with brotli, so a book is nearly as small as if it were compressed as
  one stream, and a chapter can still be opened without decompressing the rest;
- text can be recoded with a small code page made for the book, which saves about 10% on Chinese;
- JPEG files are recompressed reversibly with Lepton (about 20% smaller, the same bytes back).

On the test corpus (1,065 EPUB files, 1.45 GB) EBK files are 0.80 of the EPUB size; leaving images aside, the text
of 639 of them is 0.64 of what it takes in the EPUB; a sample of long Chinese web novels is 0.54. See
`report/evaluation.md`.

EBK is one-way: it does not give back the EPUB file it was made from (ZIP structure, order and time stamps are
not kept), and it has no DRM, no encryption and no content model of its own.

## What is here

| | |
|---|---|
| `spec/ebk-format-1.0.md` | the specification (English; `ebk-format-1.0.zh.md` is the Chinese text it was drafted in) |
| `crates/ebk` | reader and writer, Rust |
| `crates/ebk-cli` | the `ebk` program: converts a folder of EPUB files when started without a command; `convert`, `epub`, `info`, `extract`, `verify` |
| `crates/ebk-ffi` | `ebk epub` as a shared library with a C interface (Android, where a plug-in cannot start a program) |
| `koreader/` | a plug-in with which KOReader opens `.ebk` files (Kobo, Kindle, Android, desktop) |
| `crates/ebk-wasm`, `reader/` | the reader as WebAssembly and a web page that opens `.ebk` files (rendering by foliate-js) |
| `third_party/lepton_jpeg` | the JPEG codec that storage mode 4 is defined by: a fork of `lepton_jpeg` 0.5.8 maintained here (same coded stream, hardened against damaged files; see its `EBK-CHANGES.md`) |
| `tools/` | an independent reader in Python for checking (`ebk_check.py`), test vectors (`ebk_vectors.py`), a comparison of the two readers, wasm32 and browser tests |
| `fuzz/` | fuzz targets (libFuzzer, stable toolchain) |
| `phase0/` … `phase4/`, `report/` | measurements and results, in Chinese |

## Use

Without a command line: put the program `ebk` (`ebk.exe` on Windows) into a folder with EPUB files and double-click
it, or drop EPUB files or a folder on it. Each book gets an `.ebk` file next to it; the EPUB files are not changed or
deleted, and books converted earlier are skipped. What it did is shown in the window, in Chinese or English after the
language of the system, and written to `ebk-convert.log` in the folder when there is no window.
`tools/dist/README.txt` is the text for users that goes into the packages.

To read `.ebk` books: the web page in `reader/`, or KOReader with the plug-in `koreader/ebk.koplugin` (copy the folder
from the package into KOReader's `plugins` folder; `koreader/README.txt`). The plug-in writes the book as an EPUB file
into KOReader's cache and gives that to KOReader's EPUB engine, so a book looks and behaves as its EPUB does; history,
reading position and notes belong to the `.ebk` file.

```
cargo build --release
target/release/ebk convert book.epub            # writes book.ebk, after reading every member back
target/release/ebk info book.ebk --members
target/release/ebk verify book.ebk --epub book.epub
target/release/ebk extract book.ebk out/        # the members as files
target/release/ebk epub book.ebk -o again.epub  # the members as an EPUB file again (the same files, another ZIP)
target/release/ebk folder/ a.epub               # no command: convert these, as a double click does for its folder

tools/dist.sh                                   # dist/: the program for Windows, macOS, Linux; the KOReader plug-in

reader/build.sh                                 # needs the Rust target wasm32-unknown-unknown
python3 -m http.server -d reader 8000           # then open http://localhost:8000 and choose a file
```

## Tests

```
cargo test --release
python tools/ebk_vectors.py target/release/ebk /tmp/vectors              # files that break one rule each
python tools/ebk_check.py target/release/ebk out/ out.json corpus/       # convert, then check with the Python reader
python tools/ebk_differential.py target/release/ebk /tmp/vectors         # do the two readers agree
tools/wasm32-check/run.sh 256 $(ls /tmp/vectors/*.ebk | grep -v bomb)           # native against wasm32
node tools/browser/api-test.mjs /tmp/vectors target/release/ebk          # the web reader's API in Chromium and Firefox
fuzz/run.sh raw 600                                                      # also: index, roundtrip, epub, jpeg, lepton, lepton_header, jpeg_encode
python tools/lepton_compare.py corpus/                                   # after a change to the codec: same coded stream as the published crate
python tools/koreader_test.py <KOReader> dist/ebk.koplugin target/release/ebk /tmp/ko book.epub pictures.epub target/release/libebkffi.so
                                                                         # the plug-in in the desktop KOReader, without a display
python tools/smoke_test.py dist/ebk-linux/ebk /tmp/smoke crates/ebk/tests/data   # the packaged program as a user uses it; any system
```

`tools/dist.sh` builds for the other systems from Linux: it needs the Rust targets it names (`rustup target add`), zig
in `tools/zig` and `cargo-zigbuild` in `tools/cargo-tools` (`cargo install --root tools/cargo-tools cargo-zigbuild`)
as the linker, and for the Android libraries of the plug-in the Android NDK (`ANDROID_NDK_HOME`, or the one in the
Android SDK at `ANDROID_HOME`). The programs for Windows
and macOS have not been run here: `.github/workflows/platforms.yml` runs them on GitHub's machines (`smoke_test.py`
and the Rust tests). The programs for ARM Linux (e-readers) were run under `qemu-user`, not on a device. The plug-in
was run in the desktop KOReader for Linux and, with the library, in KOReader on an Android virtual device
(`tools/koreader-android/`), both v2026.07.1; not on an e-reader or a phone.

The Python tools need `brotli`; the browser tests need Playwright (`tools/browser/package.json`). `ebk_check.py` also
needs `tools/lepton-check` built (`cargo build --release` there: it decodes JPEG members with the published codec, not
the copy in `third_party/`) and uses `.omc/autoresearch/ebook-container-compression/evaluate.py` to tell images from text.
The crates are version 0.1.0; that is the version of the software, not of the format (1.0).
The test corpus is not in the repository. It is public-domain and freely licensed books (Project Gutenberg, Wikisource,
Standard Ebooks, the IDPF samples, Pro Git), downloaded by `.omc/autoresearch/ebook-container-compression/fetch_corpus.py`,
`fetch_zh.py`, `phase0/fetch_free.py` and `phase0/fetch_lang.py`.

## Licence

Code: MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). Specification: CC BY 4.0.
`third_party/lepton_jpeg` is Apache-2.0 (Microsoft); `reader/foliate-js` is MIT (John Factotum).
