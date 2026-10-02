# The plug-in in KOReader on Android

How the plug-in was tried on an Android virtual device (API 36, x86-64 with ARM64 translation; KOReader
v2026.07.1, the arm64 package). Not automated end to end; the steps:

```
adb install koreader-android-arm64-v2026.07.1.apk
adb shell appops set org.koreader.launcher MANAGE_EXTERNAL_STORAGE allow
adb shell mkdir -p /sdcard/koreader/patches /sdcard/koreader/plugins /sdcard/Books
adb push 2-ebk-test.lua /sdcard/koreader/patches/         # the test driver: reads /sdcard/koreader/ebk-test.conf
adb push ../../dist/ebk.koplugin /sdcard/koreader/plugins/
adb push book.epub book.ebk pictures.epub pictures.ebk damaged.ebk /sdcard/Books/
./run.sh book-epub /sdcard/Books/book.epub goto=30,12     # one run: report and screens into ./out
./run.sh book-ebk /sdcard/Books/book.ebk goto=30,12
./run.sh pictures-epub /sdcard/Books/pictures.epub goto=8,9,10,11,12,13,14,15,16,17,18,19,20,21,5
./run.sh pictures-ebk /sdcard/Books/pictures.ebk goto=8,9,10,11,12,13,14,15,16,17,18,19,20,21,5
python compare.py out                                     # the screens of the two, page for page
./run.sh again /sdcard/Books/book.ebk                     # opens at the page it was left at
./run.sh damaged /sdcard/Books/damaged.ebk                # a message, no book
./run.sh setlast /sdcard/Books/book.ebk start=last; ./run.sh startlast ""; ./run.sh unset /sdcard/Books/book.ebk start=filemanager
```

The first start of KOReader after it is installed should be one without the test driver in place.

On the virtual device KOReader sometimes leaves right after it starts - before it looks for plug-ins, and with
EPUB files as well as EBK files; `run.sh` starts it again when that happens.
