#!/usr/bin/env python3
"""Usage: koreader_test.py <unpacked KOReader> <plug-in folder> <ebk program> <work dir> <book.epub> <illustrated.epub> [libebkffi.so]

Runs the desktop build of KOReader without a display (SDL's dummy video driver) with the plug-in installed, and
checks that an EBK book opens as the EPUB it was made from does: the same pages, title, contents and pixels;
that the reading position is kept with the .ebk file; that KOReader can start with an EBK book; that a damaged
file gives a message; that the cache keeps to its limit. KOReader is driven by a patch file in its own
directory for patches, which reads what to do from the environment.

<unpacked KOReader> is the directory with reader.lua (from the AppImage: squashfs-root/usr/lib/koreader).
"""
import os, shutil, subprocess, sys, time

from PIL import Image, ImageChops

# pages of the illustrated book that are compared: enough of them in a row that some show pictures
PICTURE_PAGES = range(8, 22)

DRIVER = r"""
local UIManager = require("ui/uimanager")
local Device = require("device")
local Event = require("ui/event")
local env = os.getenv
for _, name in ipairs{ "start_with", "ebk_cache_mb" } do
    local value = env("EBK_TEST_SET_" .. name)
    if value then G_reader_settings:saveSetting(name, tonumber(value) or value) end
end
local delay = tonumber(env("EBK_TEST_DELAY") or "5")
local emptied
local function instance()
    return require("apps/reader/readerui").instance or require("apps/filemanager/filemanager").instance
end
if env("EBK_TEST_OPEN") then -- as a tap on the book in the file browser does
    UIManager:scheduleIn(delay, function() require("apps/reader/readerui"):showReader(env("EBK_TEST_OPEN")) end)
    delay = delay + 4
end
if env("EBK_TEST_EMPTY_CACHE") then -- the menu entry, and "Empty the cache" in the box it shows
    UIManager:scheduleIn(delay, function()
        local items = {}
        instance().ebk:addToMainMenu(items)
        items.ebk.callback()
        for _, window in ipairs(UIManager._window_stack) do
            if window.widget.ok_callback then
                window.widget.ok_callback()
                UIManager:close(window.widget)
                emptied = true
                break
            end
        end
    end)
    delay = delay + 1
end
-- EBK_TEST_GOTO is a list of pages: each is shown and its screen kept as <shot>.<page>.png; the last stays open
local pages = {}
for page in (env("EBK_TEST_GOTO") or ""):gmatch("%d+") do table.insert(pages, tonumber(page)) end
for i, page in ipairs(pages) do
    UIManager:scheduleIn(delay + i - 1, function()
        local ui = require("apps/reader/readerui").instance
        if ui then
            ui:handleEvent(Event:new("GotoPage", page))
            UIManager:forceRePaint()
            Device.screen:shot(env("EBK_TEST_SHOT") .. "." .. page .. ".png")
        end
    end)
end
delay = delay + #pages
UIManager:scheduleIn(delay + 1, function()
    Device.screen:shot(env("EBK_TEST_SHOT"))
    local out = io.open(env("EBK_TEST_OUT"), "w")
    local ui = require("apps/reader/readerui").instance
    if ui and ui.document then
        out:write("file=", tostring(ui.document.file), "\n")
        out:write("pages=", tostring(ui.document:getPageCount()), "\n")
        out:write("page=", tostring(ui:getCurrentPage()), "\n")
        out:write("title=", tostring(ui.doc_props and ui.doc_props.display_title), "\n")
        out:write("toc=", tostring(#(ui.toc and ui.toc.toc or {})), "\n")
    else
        out:write("file=none\n")
    end
    local fm = require("apps/filemanager/filemanager").instance
    if fm and fm.file_chooser then
        for _, item in ipairs(fm.file_chooser.item_table) do out:write("listed=", tostring(item.text), "\n") end
    end
    for _, window in ipairs(UIManager._window_stack) do
        if type(window.widget.text) == "string" then out:write("message=", (window.widget.text:gsub("\n", " ")), "\n") end
    end
    if emptied then out:write("emptied=yes\n") end
    out:close()
    if ui then ui:onClose() end -- as leaving the book does: the reading position is saved
    UIManager:quit(0)
end)
"""


def main():
    if len(sys.argv) not in (7, 8):
        sys.exit(__doc__)
    koreader, plugin, ebk, work, book, illustrated = (os.path.abspath(a) for a in sys.argv[1:7])
    library = os.path.abspath(sys.argv[7]) if len(sys.argv) == 8 else None
    shutil.rmtree(work, ignore_errors=True)
    home, books = os.path.join(work, "home"), os.path.join(work, "books")
    os.makedirs(os.path.join(home, "patches"))
    os.makedirs(os.path.join(home, "plugins"))
    os.makedirs(books)
    open(os.path.join(home, "patches", "2-ebk-test.lua"), "w").write(DRIVER)
    shutil.copytree(plugin, os.path.join(home, "plugins", "ebk.koplugin"))
    cache = os.path.join(home, "cache", "ebk")
    failures = []

    def check(name, good, detail=""):
        print(f"{'ok  ' if good else 'FAIL'}  {name}{'  ' + detail if detail else ''}")
        if not good:
            failures.append(name)

    def run(name, *args, **env):
        """-> what the driver reported, as {key: [values]}; the screen is in <work>/<name>.png"""
        out = os.path.join(work, name + ".txt")
        env = dict(os.environ, KO_HOME=home, SDL_VIDEODRIVER="dummy", SDL_VIDEO_DRIVER="dummy", KO_MULTIUSER="1", LC_ALL="en_US.UTF-8",
                   EBK_TEST_OUT=out, EBK_TEST_SHOT=os.path.join(work, name + ".png"), **{k: str(v) for k, v in env.items()})
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
        r = subprocess.run(["./reader.lua", *args], cwd=koreader, env=env, capture_output=True, text=True, timeout=180)
        open(os.path.join(work, name + ".log"), "w").write(r.stdout + r.stderr)
        report = {}
        if os.path.exists(out):
            for line in open(out):
                key, _, value = line.rstrip("\n").partition("=")
                report.setdefault(key, []).append(value)
        report["crashed"] = r.returncode != 0 or "stack traceback" in r.stdout + r.stderr
        return report

    def screen(name):
        return Image.open(os.path.join(work, name + ".png")).convert("RGB")

    def same_screen(a, b):
        a, b = screen(a), screen(b)
        return a.size == b.size and ImageChops.difference(a, b).getbbox() is None

    def coloured(name):
        """whether the screen shows a picture in colour (text and the interface are grey)"""
        r, g, b = screen(name).split()
        return ImageChops.difference(r, g).getbbox() is not None or ImageChops.difference(g, b).getbbox() is not None

    def book_fields(report):
        return {k: report.get(k) for k in ("pages", "page", "title", "toc")}

    def convert(epub, name):
        shutil.copy(epub, os.path.join(books, name + ".epub"))
        subprocess.run([ebk, "convert", os.path.join(books, name + ".epub"), "-o", os.path.join(books, name + ".ebk")], check=True, capture_output=True)
        return os.path.join(books, name + ".epub"), os.path.join(books, name + ".ebk")

    epub, book_ebk = convert(book, "book")
    ill_epub, ill_ebk = convert(illustrated, "pictures")
    open(os.path.join(books, "damaged.ebk"), "wb").write(open(book_ebk, "rb").read()[:-40])

    run("first-run")  # the first run shows the guide for new users,
    no_books = os.path.join(work, "no-books")  # a folder without EBK books: the file browser prepares none
    os.makedirs(no_books)
    fresh = run("fresh-empty", no_books, EBK_TEST_EMPTY_CACHE=1)
    check("before any EBK book was opened: \"Empty the cache\" does nothing, and nothing breaks", fresh.get("emptied") == ["yes"] and not fresh["crashed"] and not os.path.exists(cache), str(fresh))
    run("warm-up", epub)  # and the first book a notice about colour
    for name, source, converted, pages in (("book", epub, book_ebk, (30, 12)), ("pictures", ill_epub, ill_ebk, (*PICTURE_PAGES, 5))):
        goto = ",".join(map(str, pages))
        as_epub = run(name + "-epub", source, EBK_TEST_GOTO=goto)
        as_ebk = run(name + "-ebk", converted, EBK_TEST_GOTO=goto)
        check(f"{name}: the EBK file opens as a book", as_ebk.get("file") == [converted] and not as_ebk["crashed"], str(as_ebk.get("file")))
        check(f"{name}: the same pages, title and contents as the EPUB", book_fields(as_ebk) == book_fields(as_epub) and as_ebk.get("page") == [str(pages[-1])], str(book_fields(as_ebk)))
        check(f"{name}: the same pixels on pages {goto}", same_screen(name + "-epub", name + "-ebk") and all(same_screen(f"{name}-epub.png.{p}", f"{name}-ebk.png.{p}") for p in pages))
    with_pictures = [p for p in PICTURE_PAGES if coloured(f"pictures-ebk.png.{p}")]
    check("pictures: pages with pictures were among those compared", bool(with_pictures), str(with_pictures))

    cached = sorted(os.listdir(cache))
    again = run("again", book_ebk)
    check("opened again: at the page it was left at", again.get("page") == ["12"], str(again.get("page")))
    check("opened again: the same pixels", same_screen("book-ebk", "again"))
    check("opened again: the cached EPUB is used", sorted(os.listdir(cache)) == cached and len(cached) == 2, str(cached))
    check("the reading position is kept with the .ebk file", os.path.isfile(os.path.join(books, "book.sdr", "metadata.ebk.lua")))

    tapped = run("tapped", books, EBK_TEST_OPEN=book_ebk)
    check("opened from the file browser", tapped.get("file") == [book_ebk] and tapped.get("page") == ["12"] and not tapped["crashed"], str(tapped))

    run("set-last", book_ebk, EBK_TEST_SET_start_with="last")
    for name, options in (("start-with-last", ()), ("start-with-last-and-an-option", ("-d",))):
        last = run(name, *options)
        check(f"KOReader starts with the last book, an EBK book ({' '.join(options) or 'no options'})", last.get("file") == [book_ebk] and last.get("page") == ["12"] and "message" not in last, str(last))
    # started on another book, named as desktop systems name it; the last book must not come up instead
    from urllib.parse import quote
    other = os.path.join(books, "另一本 书 100%.ebk")  # a name that such an address has to escape
    shutil.copy(ill_ebk, other)
    named = run("named-by-address", "file://" + quote(other))
    check("started with a book named by a file:// address, it opens that book", named.get("file") == [other] and "message" not in named, str(named))
    back = run("back-to-last", book_ebk)
    check("(and the first book again, for the cases that follow)", back.get("file") == [book_ebk], str(back.get("file")))
    run("unset-last", book_ebk, EBK_TEST_SET_start_with="filemanager")

    browser = run("browser", books)
    check("the file browser lists .ebk files", {"book.ebk", "pictures.ebk"} <= set(browser.get("listed", [])), str(browser.get("listed")))

    cached = sorted(os.listdir(cache))
    damaged = run("damaged", os.path.join(books, "damaged.ebk"))
    check("a damaged file gives a message, not a crash", damaged.get("file") == ["none"] and not damaged["crashed"] and any("EBK" in m for m in damaged.get("message", [])), str(damaged.get("message")))
    check("a damaged file leaves nothing in the cache", sorted(os.listdir(cache)) == cached, str(os.listdir(cache)))

    emptied = run("empty", book_ebk, EBK_TEST_EMPTY_CACHE=1)
    check("\"Empty the cache\" leaves only the book that is open", emptied.get("emptied") == ["yes"] and len(os.listdir(cache)) == 1 and emptied.get("file") == [book_ebk], str(os.listdir(cache)))

    shutil.rmtree(cache)
    os.makedirs(cache)
    stale = os.path.join(cache, ".ebk-1-0.tmp")  # as a run that was stopped while writing leaves it
    open(stale, "wb").write(b"x" * 1000)
    os.utime(stale, (time.time() - 7200,) * 2)
    run("limit-1", ill_ebk, EBK_TEST_SET_ebk_cache_mb=1)
    limited = run("limit-2", book_ebk)
    check("the cache keeps to its limit, and keeps the book that is open", len(os.listdir(cache)) == 1 and limited.get("file") == [book_ebk], str(os.listdir(cache)))
    check("what a stopped run left in the cache is removed", not os.path.exists(stale))
    run("limit-off", book_ebk, EBK_TEST_SET_ebk_cache_mb=256)

    changing = os.path.join(books, "changing.ebk")
    shutil.copy(ill_ebk, changing)
    first = run("changing-1", changing)
    shutil.copy(book_ebk, changing)
    second = run("changing-2", changing)
    check("another book under the same name is opened as that book", first.get("title") == as_ebk.get("title") and second.get("title") == again.get("title") != first.get("title"), str((first.get("title"), second.get("title"))))

    if library:
        shutil.rmtree(cache)
        by_library = run("library", ill_ebk, EBK_PLUGIN_LIBRARY=library, EBK_TEST_GOTO=5)
        check("with the library instead of the program (as on Android): the same pixels", by_library.get("file") == [ill_ebk] and same_screen("pictures-epub", "library"), str(by_library.get("file")))
        damaged = run("library-damaged", os.path.join(books, "damaged.ebk"), EBK_PLUGIN_LIBRARY=library)
        check("with the library: a damaged file gives a message", damaged.get("file") == ["none"] and not damaged["crashed"] and any("EBK" in m for m in damaged.get("message", [])), str(damaged.get("message")))

    shutil.rmtree(os.path.join(home, "plugins", "ebk.koplugin", "bin"))
    shutil.rmtree(cache)
    none = run("no-program", book_ebk)
    check("without a program for the processor: a message", none.get("file") == ["none"] and not none["crashed"] and any("EBK" in m for m in none.get("message", [])), str(none.get("message")))

    print(f"\n{len(failures)} failures" + (": " + "; ".join(failures) if failures else ""))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
