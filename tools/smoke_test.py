#!/usr/bin/env python3
"""Usage: smoke_test.py <the ebk program> <work dir> [<directory with JPEG files>]

What a user does with the program, on whatever system this runs on (Windows, macOS, Linux; only Python's
standard library is needed): the program is put into a folder with EPUB files and started without arguments,
started again, given files and folders as if they were dropped on it, and the results are checked - with file
names in Chinese and with spaces, with one file that is not a book, and never changing an EPUB. On Windows it
is also started in a console window of its own, where it has to wait for the Enter key before the window goes.
Reading: an .ebk file given to the program (a double click on it) is handed, as an EPUB file in a cache, to the
reading program - here one that writes down what it got; on Windows also by its file type, as Explorer does, after
the program took the type; on Linux `associate` writes its desktop entry (into a folder of the test).
"""
import hashlib, os, shutil, subprocess, sys, time, zipfile

CONTAINER = '<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'


def make_epub(path, title, chapters, pictures=()):
    items = "".join(f'<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>' for i in range(len(chapters)))
    items += "".join(f'<item id="p{i}" href="{os.path.basename(p)}" media-type="image/jpeg"/>' for i, p in enumerate(pictures))
    spine = "".join(f'<itemref idref="c{i}"/>' for i in range(len(chapters)))
    opf = (f'<?xml version="1.0" encoding="utf-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">'
           f'<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">smoke-{title}</dc:identifier><dc:title>{title}</dc:title>'
           f'<dc:language>zh</dc:language></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>')
    with zipfile.ZipFile(path, "w") as z:
        z.writestr(zipfile.ZipInfo("mimetype"), "application/epub+zip")
        z.writestr("META-INF/container.xml", CONTAINER, zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/content.opf", opf, zipfile.ZIP_DEFLATED)
        for i, text in enumerate(chapters):
            page = f'<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>{title}</title></head><body>{text}</body></html>'
            z.writestr(f"OEBPS/c{i}.xhtml", page, zipfile.ZIP_DEFLATED)
        for p in pictures:
            z.write(p, "OEBPS/" + os.path.basename(p))


def digest(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def main():
    if len(sys.argv) not in (3, 4):
        sys.exit(__doc__)
    sys.stdout.reconfigure(encoding="utf-8")  # the names of the test files are Chinese; a Windows console's code page may not be
    program, work = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
    jpegs = sorted(os.path.join(sys.argv[3], n) for n in os.listdir(sys.argv[3]) if n.endswith(".jpg")) if len(sys.argv) == 4 else []
    shutil.rmtree(work, ignore_errors=True)
    folder, elsewhere, dropped = (os.path.join(work, n) for n in ("书 架", "elsewhere", "dropped"))
    for d in (folder, elsewhere, dropped, os.path.join(dropped, "sub")):
        os.makedirs(d)
    exe = os.path.join(folder, os.path.basename(program))
    shutil.copy(program, exe)
    os.chmod(exe, 0o755)
    text = "".join(f"<p>第 {i} 段。天地玄黄，宇宙洪荒；日月盈昃，辰宿列张。The quick brown fox, paragraph {i}.</p>" for i in range(400))
    books = {"六韬 上卷.epub": ("六韬", [text, text[:5000]], ()), "plain.epub": ("Plain", [text], ()), "pictures.EPUB": ("Pictures", [text[:2000]], jpegs)}
    for name, (title, chapters, pictures) in books.items():
        make_epub(os.path.join(folder, name), title, chapters, pictures)
    open(os.path.join(folder, "broken.epub"), "wb").write(b"this is not a ZIP file")
    before = {name: digest(os.path.join(folder, name)) for name in os.listdir(folder)}
    english = dict(os.environ, LANG="en_US.UTF-8", LC_ALL="en_US.UTF-8", LC_MESSAGES="en_US.UTF-8")
    failures = []

    def check(name, good, detail=""):
        print(f"{'ok  ' if good else 'FAIL'}  {name}{'  ' + str(detail)[:300] if detail and not good else ''}", flush=True)
        if not good:
            failures.append(name)

    def run(*args, env=english):
        r = subprocess.run([exe, *args], cwd=elsewhere, env=env, stdin=subprocess.DEVNULL, capture_output=True, timeout=600)
        return r.returncode, (r.stdout + r.stderr).decode("utf-8", "replace")

    def ebk_of(name):
        return os.path.join(folder, os.path.splitext(name)[0] + ".ebk")

    # --- as a double click: no arguments, the current directory is somewhere else
    code, text_out = run()
    made = all(os.path.isfile(ebk_of(name)) for name in books)
    check("double click: every book of the program's folder is converted", made and "3 converted" in text_out, text_out)
    check("double click: the file that is not a book is named, and the exit status says so", code == 1 and "1 failed: broken.epub" in text_out, (code, text_out))
    check("double click: nothing is written anywhere else", os.listdir(elsewhere) == [], os.listdir(elsewhere))
    check("double click: what it did is in ebk-convert.log (there was no window)", os.path.isfile(os.path.join(folder, "ebk-convert.log")))
    check("the EPUB files are untouched", all(digest(os.path.join(folder, name)) == d for name, d in before.items()))
    for name in books:
        code, out = run("verify", ebk_of(name), "--epub", os.path.join(folder, name))
        check(f"verify {name}: every file is in the EBK file as it is in the EPUB", code == 0 and "identical to the EPUB" in out, out)
    smaller = sum(os.path.getsize(ebk_of(n)) for n in books) < sum(os.path.getsize(os.path.join(folder, n)) for n in books)
    check("the EBK files are smaller than the EPUB files", smaller)

    sizes = {name: (os.path.getsize(ebk_of(name)), os.path.getmtime(ebk_of(name))) for name in books}
    code, text_out = run()
    check("second double click: converts nothing again", "3 skipped" in text_out and sizes == {name: (os.path.getsize(ebk_of(name)), os.path.getmtime(ebk_of(name))) for name in books}, text_out)

    if os.name != "nt":
        code, text_out = run(env=dict(os.environ, LANG="zh_CN.UTF-8", LC_ALL="zh_CN.UTF-8", LC_MESSAGES="zh_CN.UTF-8"))
        check("with a Chinese locale it speaks Chinese", "跳过 3 本" in text_out, text_out)

    # --- as files dropped on it: a file, a folder, something that is not a book
    shutil.copy(os.path.join(folder, "六韬 上卷.epub"), dropped)
    shutil.copy(os.path.join(folder, "plain.epub"), os.path.join(dropped, "sub"))
    note = os.path.join(dropped, "读我.txt")
    open(note, "w", encoding="utf-8").write("not a book")
    code, text_out = run(os.path.join(dropped, "六韬 上卷.epub"), os.path.join(dropped, "sub"), note)
    check("dropped file and folder: both converted, the other file named", code == 0 and "2 converted" in text_out and "not an .epub file" in text_out
          and os.path.isfile(os.path.join(dropped, "六韬 上卷.ebk")) and os.path.isfile(os.path.join(dropped, "sub", "plain.ebk")), (code, text_out))
    check("dropped: the same EBK file as from the double click", digest(os.path.join(dropped, "六韬 上卷.ebk")) == digest(ebk_of("六韬 上卷.epub")))

    # --- back to EPUB, and the members as files
    again = os.path.join(work, "再来 一次.epub")
    code, out = run("epub", ebk_of("pictures.EPUB"), "-o", again)
    code2, out2 = run("verify", ebk_of("pictures.EPUB"), "--epub", again)
    with zipfile.ZipFile(again) as z:
        first = z.infolist()[0]
        readable = z.testzip() is None and (first.filename, first.compress_type) == ("mimetype", 0)
    check("epub: the EBK file as an EPUB file again, with the same files", code == 0 and code2 == 0 and readable, out + out2)
    code, out = run("epub", ebk_of("pictures.EPUB"), "-o", again)
    check("epub: an existing file is not replaced", code == 1 and "exists already" in out, out)
    code, out = run("epub", ebk_of("pictures.EPUB"))
    # where upper and lower case are the same name (Windows, macOS) the EPUB is in the way and the program refuses
    refused_or_other = code == 1 or sorted(n for n in os.listdir(folder) if n.lower() == "pictures.epub") == ["pictures.EPUB", "pictures.epub"]
    check("epub: nor the EPUB next to the EBK file", refused_or_other and digest(os.path.join(folder, "pictures.EPUB")) == before["pictures.EPUB"], out)
    out_dir = os.path.join(work, "extracted")
    code, out = run("extract", ebk_of("pictures.EPUB"), out_dir)
    with zipfile.ZipFile(os.path.join(folder, "pictures.EPUB")) as z:
        same = code == 0 and all(open(os.path.join(out_dir, *n.split("/")), "rb").read() == z.read(n) for n in z.namelist())
    check("extract: the files of the book, each as in the EPUB", same, out)

    # --- reading: the book goes to the reading program as an EPUB file in a cache
    cache, record = os.path.join(work, "cache"), os.path.join(work, "opened.txt")
    recorder = os.path.join(work, "record.py")
    open(recorder, "w", encoding="utf-8").write(f"#!{sys.executable}\nimport sys\nopen({record!r}, 'a', encoding='utf-8').write(sys.argv[1] + '\\n')\n")
    if os.name == "nt":
        opener = os.path.join(work, "record.cmd")
        open(opener, "w").write(f'@"{sys.executable}" "{recorder}" %*\n')
    else:
        opener = recorder
        os.chmod(recorder, 0o755)
    reading = dict(english, EBK_OPENER=opener, EBK_CACHE_DIR=cache)

    def opened():
        return open(record, encoding="utf-8").read().splitlines() if os.path.exists(record) else []

    book = ebk_of("六韬 上卷.epub")
    code, out = run(book, env=reading)
    got = opened()
    check("reading: an .ebk file given to the program goes to the reading program as an EPUB file in the cache",
          code == 0 and len(got) == 1 and got[0].startswith(cache) and got[0].endswith("六韬 上卷.epub") and os.path.isfile(got[0]), (code, out, got))
    if got:
        code, out = run("verify", book, "--epub", got[0])
        check("reading: that EPUB file has the files of the book", code == 0, out)
        written = os.path.getmtime(got[0])
        time.sleep(1.1)
        code, out = run("open", book, env=reading)
        check("reading: opened again, the same file, not written again", code == 0 and opened() == [got[0]] * 2 and os.path.getmtime(got[0]) == written, (out, opened()))
    damaged = os.path.join(work, "damaged.ebk")
    open(damaged, "wb").write(open(book, "rb").read()[:-100])
    code, out = run(damaged, env=reading)
    check("reading: a damaged .ebk file gives a message and goes nowhere", code == 1 and "Cannot open damaged.ebk" in out and len(opened()) == 2, (code, out))

    if sys.platform.startswith("linux"):
        data = os.path.join(work, "xdg-data")
        xdg = dict(english, XDG_DATA_HOME=data, XDG_CONFIG_HOME=os.path.join(work, "xdg-config"))
        desktop = os.path.join(data, "applications", "ebk.desktop")
        code, out = run("associate", env=xdg)
        entry = open(desktop, encoding="utf-8").read() if os.path.exists(desktop) else ""
        check("Linux: associate writes a desktop entry that opens .ebk files with this program",
              code == 0 and f'Exec="{exe}" open %F' in entry and "MimeType=application/x-ebk;" in entry and os.path.isfile(os.path.join(data, "mime", "packages", "ebk.xml")), (out, entry))
        code, out = run("associate", "--remove", env=xdg)
        check("Linux: associate --remove takes it away", code == 0 and not os.path.exists(desktop), out)
    if sys.platform == "darwin":
        code, out = run("associate")
        check("macOS: associate points to EBK.app", code == 1 and "EBK.app" in out, out)
    if os.name == "nt":
        import winreg
        command = lambda: winreg.QueryValue(winreg.HKEY_CURRENT_USER, r"Software\Classes\EBK.Book\shell\open\command")
        check("Windows: started without a command, the program took the .ebk file type", command().lower() == f'"{exe}" "%1"'.lower(), command())
        subprocess.run(["cmd", "/c", "start", "", book], env=reading, timeout=60)
        deadline = time.time() + 120
        while time.time() < deadline and len(opened()) < 3:
            time.sleep(0.5)
        check("Windows: an .ebk file opened by its type, as by a double click, reaches the reading program", opened()[2:] == opened()[:1], opened())
        code, out = run("associate", "--remove")
        try:
            gone = not command()
        except OSError:
            gone = True
        check("Windows: associate --remove gives the file type back", code == 0 and gone, out)
        run()
        try:
            gone = not command()
        except OSError:
            gone = True
        check("Windows: after that a double click on the program does not take it again", gone)

    # --- Windows: in a console window made for it, the program waits for Enter; without one it does not
    if os.name == "nt":
        os.remove(ebk_of("plain.epub"))
        p = subprocess.Popen([exe], cwd=elsewhere, creationflags=subprocess.CREATE_NEW_CONSOLE)
        deadline = time.time() + 120
        while time.time() < deadline and not os.path.isfile(ebk_of("plain.epub")):
            time.sleep(0.5)
        time.sleep(5)
        waiting = os.path.isfile(ebk_of("plain.epub")) and p.poll() is None
        p.kill()
        check("Windows: in its own console window it converts, then waits for Enter", waiting)

    print(f"\n{len(failures)} failures" + (": " + "; ".join(failures) if failures else ""))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
