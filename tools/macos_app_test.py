#!/usr/bin/env python3
"""Usage (on a Mac): macos_app_test.py <EBK.app> <work dir>

EBK.app the way a user uses it, through Launch Services as Finder does: an EPUB file dropped on the app becomes an
.ebk file next to it; then a double click on that .ebk file (`open` by its file type) gives the book, as an EPUB
file in ~/Library/Caches/EBK, to the EPUB reader of the Mac. Dialogs the app shows are left open and the app is
stopped at the end.
"""
import glob, os, plistlib, shutil, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from smoke_test import make_epub  # noqa: E402

LSREGISTER = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"


def main():
    app, work = (os.path.abspath(a) for a in sys.argv[1:3])
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    failures = []

    def check(name, good, detail=""):
        print(f"{'ok  ' if good else 'FAIL'}  {name}{'  ' + str(detail)[:400] if detail and not good else ''}", flush=True)
        if not good:
            failures.append(name)

    def wait_for(found, seconds=120):
        deadline = time.time() + seconds
        while time.time() < deadline and not found():
            time.sleep(0.5)
        return found()

    info = plistlib.load(open(os.path.join(app, "Contents", "Info.plist"), "rb"))
    owned = [t for t in info["CFBundleDocumentTypes"] if t.get("LSHandlerRank") == "Owner"]
    check("the app declares .ebk files and owns them", [t["UTTypeTagSpecification"]["public.filename-extension"] for t in info["UTExportedTypeDeclarations"]] == [["ebk"]] and len(owned) == 1)
    check("the app is signed (ad hoc) and its seal holds", subprocess.run(["codesign", "--verify", "--deep", "--strict", app]).returncode == 0)
    subprocess.run([LSREGISTER, "-f", app], check=True)

    name = "书 一本.epub"
    epub = os.path.join(work, name)
    make_epub(epub, "Test", ["<p>" + "天地玄黄，宇宙洪荒。The quick brown fox. " * 400 + "</p>"])
    ebk = epub[:-5] + ".ebk"
    subprocess.run(["open", "-a", app, epub], check=True)
    check("an EPUB file dropped on the app becomes an .ebk file next to it", wait_for(lambda: os.path.isfile(ebk)))
    time.sleep(3)  # the report of the conversion is on the screen now
    subprocess.run(["pkill", "-f", app + "/Contents/MacOS/"])
    out = subprocess.run(["mdls", "-name", "kMDItemContentType", ebk], capture_output=True, text=True).stdout.strip()
    print("      content type of the .ebk file:", out)

    cache = os.path.expanduser("~/Library/Caches/EBK")
    before = set(glob.glob(os.path.join(cache, "*", "*.epub")))
    subprocess.run(["open", ebk], check=True)
    made = lambda: set(glob.glob(os.path.join(cache, "*", "书 一本.epub"))) - before
    check("a double click on the .ebk file: the book goes, as an EPUB file in the cache, to the EPUB reader", wait_for(made), sorted(before))
    if made():
        path = made().pop()
        code = subprocess.run([os.path.join(app, "Contents", "Resources", "ebk"), "verify", ebk, "--epub", path]).returncode
        check("that EPUB file has the files of the book", code == 0)
    time.sleep(5)
    readers = subprocess.run(["ps", "-axo", "comm"], capture_output=True, text=True).stdout
    print("      Books running:", "Books.app" in readers)
    subprocess.run(["pkill", "-f", app + "/Contents/MacOS/"])
    subprocess.run(["pkill", "-x", "Books"])

    print(f"\n{len(failures)} failures" + (": " + "; ".join(failures) if failures else ""))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
