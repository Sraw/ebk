# EBK

**[English version ↓](#ebk-english)**

EBK 是一种电子书阅读格式，由 EPUB 转换而来，比 EPUB 小：整体小约四分之一，纯文字的书小三到四成，网文小近一半。
书里的每个文件（章节、样式、图片、字体）都按原来的路径保存，读出来与 EPUB 里的逐字节相同；阅读时用 EPUB 的排版引擎显示，
所以分章、目录跳转、排版都和 EPUB 一样。变的只是外面的容器：

- 文字拼在一起按块（每块约 4 MB）用 brotli 压缩，几乎和整本一起压一样小；
- 每本书自带一张码表，常用字只占 1–2 个字节，中文因此再小约 10%；
- JPEG 图片用 Lepton 无损地重新压缩：小约 20%，还原出来与原图逐字节相同。

现在的阅读方式（KOReader 插件、电脑上双击打开）都是第一次打开时把整本书转成 EPUB 放进缓存，交给 EPUB 阅读器显示，
所以第一次打开要等一会儿，之后直接用缓存。格式本身允许只解压某一章所在的那一块，但现在的工具没有用到这一点。

EBK 是单向的：能从 `.ebk` 得到一个文件内容相同的 EPUB，但不是原来那个 ZIP 文件（文件顺序、时间等不保留）。
EBK 没有 DRM，也不加密。

## 体积

1,209 本书：1,065 本公开的书（Project Gutenberg 的英文、中文和插图本，维基文库，Standard Ebooks，IDPF 样例，
Pro Git，24 种其它语言），加上 144 本中文网文抽样（网文不公开，只给合计）。比值 = EBK 的字节数 ÷ EPUB 的字节数，越小越好。

**整本文件，按图片占 EPUB 的比例**

| 图片占比 | 本数 | EPUB | EBK | 比值 |
|---|---|---|---|---|
| 不到 5% | 312 | 845.0 MB | 596.4 MB | 0.706（小 29%） |
| 5–50% | 506 | 159.1 MB | 117.7 MB | 0.740（小 26%） |
| 50% 以上（以图为主） | 391 | 647.1 MB | 548.7 MB | 0.848（小 15%） |
| **全部** | **1,209** | **1,651.2 MB** | **1,262.7 MB** | **0.765（小 24%）** |

**不带图：只算文字部分（去掉图片、字体、音视频），按文字**

| 文字 | 本数 | 比值 |
|---|---|---|
| 纯英文 | 197 | 0.633（小 37%） |
| 纯中文 | 100 | 0.639（小 36%） |
| 中文网文（从 TXT 转成的 EPUB） | 144 | 0.540（小 46%） |
| 中英混排 | 392 | 0.616（小 38%） |
| 其它文字和语言 | 376 | 0.594（小 41%） |
| **全部** | **1,209** | **0.582（小 42%）** |

"中英混排"大多是带英文版权页的 Gutenberg 中文书；真正中英混排的技术书 Pro Git 中文版是 0.665。

**带图：图片本身**

| 图片 | 本数 | 比值 |
|---|---|---|
| JPEG（重新压缩） | 159 | 0.814（小 19%） |
| PNG、GIF（原样存放） | 816 | 0.995（几乎不变） |
| 两种都有 | 88 | 0.860 |

所以图片多、又是 PNG 的书，EBK 只比 EPUB 小 1–2%。没有一本书比 EPUB 大。

**速度**

- 转换比较慢：文字用 brotli 的最高压缩档，十几 MB 的纯文字大书在普通电脑上要十几秒，大多数书一两秒。
- 读取：文字和读 EPUB 的 ZIP 差不多快；JPEG 要先还原，比直接读慢得多，带大量 JPEG 的书第一次打开要多等一会儿。

## 怎么用

### 获取程序

现在还没有公开发布。程序由 `tools/dist.sh` 打包到 `dist/`：

| 包 | 内容 |
|---|---|
| `ebk-windows.zip` | `ebk.exe` |
| `ebk-macos.zip` | `ebk`（Apple 芯片和 Intel 通用）；带 `EBK.app` 的版本由 GitHub 上的 macOS 任务构建（下载件 `ebk-macos`） |
| `ebk-linux.zip` | `ebk`（x86-64，静态链接） |
| `ebk.koplugin.zip` | KOReader 插件 |

每个包里都有中英文的使用说明 `README.txt`。

### 把 EPUB 转成 EBK

1. 把程序（Windows 上是 `ebk.exe`）放进有 `.epub` 文件的文件夹。
2. 双击它。每个 EPUB 旁边会生成同名的 `.ebk` 文件；大书要等一会儿。

也可以把 EPUB 文件或文件夹拖到程序上。原来的 EPUB 不会被修改或删除；已经转换过的书会跳过。
窗口里的提示按系统语言显示中文或英文；没有窗口时（例如 Linux 上在文件管理器里双击），结果写在文件夹里的 `ebk-convert.log`。

第一次运行：

- Windows：如果出现"Windows 已保护你的电脑"，点"更多信息"→"仍要运行"。
- macOS：程序没有 Apple 开发者签名，第一次会被拦下。打开"系统设置 → 隐私与安全性"，点"仍要打开"。只需要一次。
- Linux：如果双击没有反应，在终端里运行 `./ebk`。

### 在电脑上读

双击 `.ebk` 文件：程序把它转成 EPUB 放进缓存，再用电脑上已有的 EPUB 阅读器打开，例如 Calibre、Thorium Reader、
SumatraPDF，或 macOS 的"图书"。同一本书每次都是同一个缓存文件，所以阅读器记得读到哪里。缓存最多约 1 GB，
久未打开的书会被清掉，下次打开时重新生成。

要让双击 `.ebk` 起作用，先做一次：

| 系统 | 做法 | 撤销 |
|---|---|---|
| Windows | 双击一次 `ebk.exe`（转换书时就会做）。之后别挪动程序，挪了就再双击一次。你已经给 `.ebk` 选过别的程序时，它不改你的选择。 | `ebk associate --remove`（之后双击程序也不会再接管） |
| macOS | 把 `EBK.app` 拖进"应用程序"，打开一次。之后双击 `.ebk` 就用它打开；把 EPUB 拖到 `EBK.app` 上也能转换。 | 删掉 `EBK.app` |
| Linux | 在终端里运行一次 `./ebk associate`。之后别挪动程序。 | `./ebk associate --remove` |

### 在阅读器和手机上读：KOReader

KOReader 装上插件后能直接打开 `.ebk`，支持 Kobo、Kindle（需要已经装好 KOReader）、Android 和 Linux。
把 `ebk.koplugin.zip` 解压，把 `ebk.koplugin` 文件夹放进 KOReader 的 `plugins` 文件夹，重启 KOReader；各设备的路径见 `koreader/README.txt`。
插件把书转成 EPUB 放进 KOReader 的缓存（默认最多 256 MB），交给 KOReader 的 EPUB 引擎显示；阅读进度、书签、笔记都记在 `.ebk` 文件名下。

### 变回 EPUB，以及命令行

```
ebk epub 书.ebk -o 书-还原.epub         # 文件内容与原书相同的 EPUB；不会覆盖已有的文件
ebk convert book.epub                   # 转换一本，写出 book.ebk（写完后逐个读回核对）
ebk open book.ebk                       # 用电脑上的 EPUB 阅读器打开
ebk associate [--remove]                # 让双击 .ebk 用这个程序打开（Windows、Linux）
ebk verify book.ebk --epub book.epub    # 核对每个文件与 EPUB 里的是否相同
ebk info book.ebk --members             # 文件的结构
ebk extract book.ebk out/               # 把书里的文件取出来
ebk folder/ a.epub                      # 不带命令：转换这些，和双击一样
```

## 已知的局限

- 没有在真机上测过：Kindle、Kobo 上的程序只在 `qemu-user` 里跑过，Android 只在虚拟机里跑过。
- Windows 和 macOS 的程序在 GitHub 的机器上测过（Windows x86-64 和 ARM，macOS 的 Apple 芯片和 Intel），但没有真正用鼠标双击过。
- macOS 第一次打开会被系统拦一次（没有开发者签名）。
- PNG 图片几乎压不动。
- 试过、但决定不用的：常用词组编码（只省约 2%），以及 PPMd 压缩（文字能再小 11–21%，但在 Kindle 上解压太慢）。

## 仓库里有什么

| | |
|---|---|
| `spec/ebk-format-1.0.md` | 格式规范（英文为准；`ebk-format-1.0.zh.md` 是起草时的中文版） |
| `crates/ebk` | 读写库，Rust |
| `crates/ebk-cli` | 程序 `ebk` |
| `crates/ebk-ffi` | `ebk epub` 的 C 接口共享库（Android 上插件不能启动程序） |
| `koreader/` | KOReader 插件 |
| `third_party/lepton_jpeg` | 存储方式 4 所依据的 JPEG 编解码器：`lepton_jpeg` 0.5.8 的分支，在这里维护（编码结果不变，能拒绝损坏的文件；见其中的 `EBK-CHANGES.md`） |
| `tools/` | 打包脚本，Python 写的独立读取器（`ebk_check.py`），测试向量，各种测试 |
| `fuzz/` | 模糊测试（libFuzzer，稳定版工具链） |

## 构建和测试

```
cargo build --release
cargo test --release
python tools/ebk_vectors.py target/release/ebk /tmp/vectors              # 每个文件违反一条规则的测试向量
python tools/ebk_check.py target/release/ebk out/ out.json corpus/       # 转换，再用 Python 读取器检查
python tools/ebk_differential.py target/release/ebk /tmp/vectors         # 两个读取器结论是否一致
fuzz/run.sh raw 600                                                      # 还有 index、roundtrip、epub、jpeg、lepton、lepton_header、jpeg_encode
python tools/lepton_compare.py corpus/                                   # 改了编解码器之后：编码结果与发布的版本相同
python tools/koreader_test.py <KOReader> dist/ebk.koplugin target/release/ebk /tmp/ko book.epub pictures.epub target/release/libebkffi.so
                                                                         # 插件在桌面版 KOReader 里，无显示器
python tools/smoke_test.py dist/ebk-linux/ebk /tmp/smoke crates/ebk/tests/data   # 打包好的程序，按用户的用法；任何系统
tools/dist.sh                                                            # dist/：Windows、macOS、Linux 的程序和 KOReader 插件
```

`tools/dist.sh` 在 Linux 上为其它系统构建，需要：

- 它列出的 Rust 目标（`rustup target add`）；
- 作为链接器的 zig（放在 `tools/zig`）和 `cargo-zigbuild`（`cargo install --root tools/cargo-tools cargo-zigbuild`）；
- 构建插件的 Android 库时还要 Android NDK（`ANDROID_NDK_HOME`，或 `ANDROID_HOME` 下的 SDK 里的 NDK）。

`.github/workflows/platforms.yml` 在 GitHub 的 Windows 和 macOS 机器上运行打包好的程序（`smoke_test.py` 和 Rust 测试），
并在 Mac 上构建、测试 `EBK.app`（`tools/dist/macos/build-app.sh`、`tools/macos_app_test.py`）。

其它说明：

- Python 工具需要 `brotli`。
- `ebk_check.py` 还需要先构建 `tools/lepton-check`（在那里运行 `cargo build --release`），它用发布的编解码器解 JPEG，
  而不是 `third_party/` 里的那份。
- 各个 crate 的版本 0.1.0 是软件的版本，格式的版本是 1.0。
- 测试用的书不在仓库里：体积数据来自公有领域和自由许可的书（Project Gutenberg、维基文库、Standard Ebooks 等）。

## 许可

代码：MIT 或 Apache-2.0，任选（`LICENSE-MIT`、`LICENSE-APACHE`）。规范：CC BY 4.0。
`third_party/lepton_jpeg` 是 Apache-2.0（Microsoft）。

---

# EBK (English)

EBK is a reading format for e-books, made from EPUB, and smaller than EPUB: about a quarter smaller overall, a third
smaller for books of plain text, nearly half for Chinese web novels. Every file of the book (chapters, style sheets,
pictures, fonts) is kept under its original path and reads back byte for byte as in the EPUB; it is shown by an EPUB
rendering engine, so chapters, the table of contents and the layout are those of the EPUB. Only the container changes:

- text is joined and compressed with brotli in blocks of about 4 MB: nearly as small as one stream for the whole book;
- each book carries a code page of its own, in which frequent characters take 1–2 bytes: about 10% more off Chinese;
- JPEG pictures are recompressed losslessly with Lepton: about 20% smaller, the same bytes back.

The ways of reading there are now (the KOReader plug-in, a double click on a computer) turn the whole book into an
EPUB file in a cache the first time it is opened and give that to an EPUB reader: the first opening takes a moment,
later ones use the cache. The format would let a reader decompress only the block a chapter is in, but these tools do
not make use of it.

EBK is one-way: an `.ebk` file gives an EPUB with the same files, but not the ZIP file it was made from (order and
time stamps are not kept). EBK has no DRM and no encryption.

## Size

1,209 books: 1,065 public ones (Project Gutenberg in English, Chinese and illustrated editions, Chinese Wikisource,
Standard Ebooks, the IDPF samples, Pro Git, 24 other languages) and a sample of 144 Chinese web novels (not public;
totals only). Ratio = bytes of the EBK file ÷ bytes of the EPUB; smaller is better.

**Whole files, by the share of pictures in the EPUB**

| Pictures | Books | EPUB | EBK | Ratio |
|---|---|---|---|---|
| under 5% | 312 | 845.0 MB | 596.4 MB | 0.706 (29% smaller) |
| 5–50% | 506 | 159.1 MB | 117.7 MB | 0.740 (26% smaller) |
| over 50% (mostly pictures) | 391 | 647.1 MB | 548.7 MB | 0.848 (15% smaller) |
| **all** | **1,209** | **1,651.2 MB** | **1,262.7 MB** | **0.765 (24% smaller)** |

**Without pictures: the text only (pictures, fonts, audio and video left out), by script**

| Text | Books | Ratio |
|---|---|---|
| English | 197 | 0.633 (37% smaller) |
| Chinese | 100 | 0.639 (36% smaller) |
| Chinese web novels (EPUB made from TXT) | 144 | 0.540 (46% smaller) |
| Chinese and English mixed | 392 | 0.616 (38% smaller) |
| other scripts and languages | 376 | 0.594 (41% smaller) |
| **all** | **1,209** | **0.582 (42% smaller)** |

Most of the "mixed" books are Gutenberg's Chinese books with their English licence; a technical book that really
mixes both, Pro Git in Chinese, is 0.665.

**With pictures: the pictures themselves**

| Pictures | Books | Ratio |
|---|---|---|
| JPEG (recompressed) | 159 | 0.814 (19% smaller) |
| PNG, GIF (stored as they are) | 816 | 0.995 (hardly any change) |
| both | 88 | 0.860 |

So a book of many PNG pictures is only 1–2% smaller than its EPUB. No book is larger than its EPUB.

**Speed**

- Converting is slow, because text is compressed with brotli's highest setting: a large book of plain text (over 10 MB)
  takes ten seconds or more on an ordinary computer, most books a second or two.
- Reading: text about as fast as from the ZIP of an EPUB; JPEG has to be restored first, which is much slower, so a book
  with many JPEG pictures takes longer to open the first time.

## Use

### Getting the program

There is no public release yet. `tools/dist.sh` packs the programs into `dist/`:

| Package | Contents |
|---|---|
| `ebk-windows.zip` | `ebk.exe` |
| `ebk-macos.zip` | `ebk` (Apple silicon and Intel); the package with `EBK.app` is built by the macOS job on GitHub (artifact `ebk-macos`) |
| `ebk-linux.zip` | `ebk` (x86-64, statically linked) |
| `ebk.koplugin.zip` | the KOReader plug-in |

Each package has instructions in Chinese and English, `README.txt`.

### Converting EPUB to EBK

1. Put the program (`ebk.exe` on Windows) into a folder with `.epub` files.
2. Double-click it. An `.ebk` file appears next to each EPUB; large books take a while.

You can also drop EPUB files or a folder on the program. The EPUB files are never changed or deleted; books converted
earlier are skipped. The window speaks Chinese or English after the language of the system; without a window (a double
click in a Linux file manager) the result is in `ebk-convert.log` in the folder.

The first run:

- Windows: if "Windows protected your PC" appears, choose "More info", then "Run anyway".
- macOS: the program has no Apple developer signature and is stopped the first time. Open System Settings → Privacy &
  Security and choose "Open Anyway". Only once.
- Linux: if a double click does nothing, run `./ebk` in a terminal.

### Reading on a computer

Double-click an `.ebk` file: the program turns it into an EPUB file in a cache and opens that in the EPUB reader the
computer has, such as Calibre, Thorium Reader, SumatraPDF or Books on macOS. A book is the same cached file every time,
so the reader remembers where you were. The cache keeps about 1 GB; books not opened for a while are removed and made
again when opened.

For the double click to work, once:

| System | What to do | To undo |
|---|---|---|
| Windows | Double-click `ebk.exe` once (converting books does it). Leave the program where it is; if you move it, double-click it again. A program you chose for `.ebk` files yourself stays. | `ebk associate --remove` (a double click on the program then leaves the file type alone) |
| macOS | Drag `EBK.app` into Applications and open it once. From then on `.ebk` files open with it; EPUB files dropped on `EBK.app` are converted. | delete `EBK.app` |
| Linux | Run `./ebk associate` once in a terminal. Leave the program where it is. | `./ebk associate --remove` |

### Reading on e-readers and phones: KOReader

With the plug-in, KOReader opens `.ebk` files: Kobo, Kindle (with KOReader installed), Android and Linux. Unpack
`ebk.koplugin.zip`, put the folder `ebk.koplugin` into KOReader's `plugins` folder and restart KOReader; the folder on
each device is in `koreader/README.txt`. The plug-in turns the book into an EPUB file in KOReader's cache (at most
256 MB by default) and gives that to KOReader's EPUB engine; reading position, bookmarks and notes belong to the `.ebk` file.

### Back to EPUB, and the command line

```
ebk epub book.ebk -o again.epub         # an EPUB with the same files; an existing file is never replaced
ebk convert book.epub                   # convert one book to book.ebk (every file is read back and checked)
ebk open book.ebk                       # open it in this computer's EPUB reader
ebk associate [--remove]                # a double click on an .ebk file opens it with this program (Windows, Linux)
ebk verify book.ebk --epub book.epub    # is every file the same as in the EPUB
ebk info book.ebk --members             # the layout of the file
ebk extract book.ebk out/               # the files of the book
ebk folder/ a.epub                      # no command: convert these, as a double click does
```

## Known limits

- Not tried on real devices: the programs for Kindle and Kobo ran under `qemu-user`, Android only on a virtual device.
- The Windows and macOS programs were tested on GitHub's machines (Windows x86-64 and ARM, macOS on Apple silicon and
  Intel), but never with a real double click of the mouse.
- macOS stops the program once the first time (no developer signature).
- PNG pictures hardly get smaller.
- Tried and not used: coding frequent phrases (about 2%), and PPMd (text 11–21% smaller, but too slow to decode on a
  Kindle).

## What is here

| | |
|---|---|
| `spec/ebk-format-1.0.md` | the specification (English, which governs; `ebk-format-1.0.zh.md` is the Chinese text it was drafted in) |
| `crates/ebk` | reader and writer, Rust |
| `crates/ebk-cli` | the program `ebk` |
| `crates/ebk-ffi` | `ebk epub` as a shared library with a C interface (on Android a plug-in cannot start a program) |
| `koreader/` | the KOReader plug-in |
| `third_party/lepton_jpeg` | the JPEG codec storage mode 4 is defined by: a fork of `lepton_jpeg` 0.5.8 maintained here (same coded stream, refuses damaged files; see its `EBK-CHANGES.md`) |
| `tools/` | packaging, an independent reader in Python (`ebk_check.py`), test vectors, tests |
| `fuzz/` | fuzz targets (libFuzzer, stable toolchain) |

## Building and testing

```
cargo build --release
cargo test --release
python tools/ebk_vectors.py target/release/ebk /tmp/vectors              # files that break one rule each
python tools/ebk_check.py target/release/ebk out/ out.json corpus/       # convert, then check with the Python reader
python tools/ebk_differential.py target/release/ebk /tmp/vectors         # do the two readers agree
fuzz/run.sh raw 600                                                      # also: index, roundtrip, epub, jpeg, lepton, lepton_header, jpeg_encode
python tools/lepton_compare.py corpus/                                   # after a change to the codec: same coded stream as the published crate
python tools/koreader_test.py <KOReader> dist/ebk.koplugin target/release/ebk /tmp/ko book.epub pictures.epub target/release/libebkffi.so
                                                                         # the plug-in in the desktop KOReader, without a display
python tools/smoke_test.py dist/ebk-linux/ebk /tmp/smoke crates/ebk/tests/data   # the packaged program as a user uses it; any system
tools/dist.sh                                                            # dist/: the programs for Windows, macOS, Linux and the KOReader plug-in
```

`tools/dist.sh` builds for the other systems from Linux. It needs:

- the Rust targets it names (`rustup target add`);
- zig in `tools/zig` and `cargo-zigbuild` (`cargo install --root tools/cargo-tools cargo-zigbuild`) as the linker;
- for the Android libraries of the plug-in, the Android NDK (`ANDROID_NDK_HOME`, or the one in the SDK at `ANDROID_HOME`).

`.github/workflows/platforms.yml` runs the packaged programs on GitHub's Windows and macOS machines (`smoke_test.py`
and the Rust tests), and builds and tries `EBK.app` on a Mac (`tools/dist/macos/build-app.sh`,
`tools/macos_app_test.py`).

Other notes:

- The Python tools need `brotli`.
- `ebk_check.py` also needs `tools/lepton-check` built (`cargo build --release` there). It decodes JPEG members with
  the published codec, not the copy in `third_party/`.
- The crates are version 0.1.0; that is the version of the software, not of the format (1.0).
- The test books are not in the repository; the sizes above come from public-domain and freely licensed books (Project
  Gutenberg, Wikisource, Standard Ebooks and others).

## Licence

Code: MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). Specification: CC BY 4.0.
`third_party/lepton_jpeg` is Apache-2.0 (Microsoft).
