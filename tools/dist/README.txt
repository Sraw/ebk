EBK 转换工具 / EBK converter
============================

把 EPUB 电子书转换成更小的 .ebk 文件。原来的 EPUB 不会被修改或删除。
Converts EPUB books to smaller .ebk files. The EPUB files are never changed or deleted.

用法 / How to use
-----------------
1. 把这个程序（Windows 上是 ebk.exe，macOS 和 Linux 上是 ebk）放进有 .epub 文件的文件夹。
   Put the program (ebk.exe on Windows, ebk on macOS and Linux) in a folder that has .epub files.
2. 双击它。每个 EPUB 旁边会生成一个同名的 .ebk 文件。大的书需要等一会儿。
   Double-click it. An .ebk file appears next to each EPUB. Large books take a while.

也可以把 EPUB 文件或文件夹拖到程序图标上。已经转换过的书会跳过。
You can also drop EPUB files or a folder on the program. Books converted earlier are skipped.

第一次运行 / The first run
-------------------------
Windows: 如果出现"Windows 已保护你的电脑"，点"更多信息"→"仍要运行"。
         If "Windows protected your PC" appears, choose "More info", then "Run anyway".
macOS:   第一次双击会被系统拦下。打开"系统设置 → 隐私与安全性"，在下面点"仍要打开"。
         （较旧的 macOS：右键点程序，选"打开"，再点"打开"。）只需要做一次。
         The first double click is stopped by the system. Open System Settings → Privacy & Security and
         choose "Open Anyway" further down. (Older macOS: right-click the program, choose "Open", then
         "Open" again.) Only needed once.
Linux:   如果双击没有反应：在文件夹里打开终端，运行 ./ebk 。
         （不在终端里运行时没有窗口，结果写在文件夹里的 ebk-convert.log。）
         If a double click does nothing: open a terminal in the folder and run ./ebk .
         (Started without a terminal there is no window; the result is in ebk-convert.log in the folder.)

把 .ebk 变回 EPUB / Getting an EPUB back
----------------------------------------
在终端里运行 / In a terminal:    ebk epub 书.ebk -o 书-还原.epub
得到的 EPUB 里每个文件都与原书相同。已有的文件不会被覆盖，所以要给它另起一个名字。
Every file inside is the same as in the original book. An existing file is never replaced, so give it
a name of its own.

在电脑上读 .ebk / Reading .ebk books on a computer
------------------------------------------------
双击 .ebk 文件，程序会把它转成 EPUB 放进缓存，再用电脑上已有的 EPUB 阅读器打开
（Calibre、Thorium Reader、SumatraPDF、macOS 的"图书"等）。同一本书每次打开都是同一个文件，
阅读器记得读到哪里。缓存最多约 1 GB，久未打开的书会被清掉，下次打开时重新生成。
没有 EPUB 阅读器的话先装一个。
A double click on an .ebk file turns it into an EPUB file in a cache and opens that in the EPUB reader the
computer has (Calibre, Thorium Reader, SumatraPDF, Books on macOS, …). A book opens as the same file every
time, so the reader remembers where you were. The cache keeps about 1 GB; books not opened for a while are
removed and made again when opened. Without an EPUB reader, install one first.

要让双击 .ebk 起作用，先做一次 / For the double click to work, once:
Windows: 双击一次 ebk.exe（像上面那样转换书时就会做）。之后程序不要挪地方；挪了就再双击一次。
         不想要了：在命令行运行 ebk associate --remove 。
         Double-click ebk.exe once (converting books as above does it). Leave the program where it is;
         if you move it, double-click it again. To undo: ebk associate --remove in a command prompt.
macOS:   把 EBK.app 拖进"应用程序"，打开一次（第一次同样要在"隐私与安全性"里点"仍要打开"）。
         之后双击 .ebk 就会用它打开；把 EPUB 文件拖到 EBK.app 上也能转换。
         Drag EBK.app into Applications and open it once (the first time, again "Open Anyway" in Privacy &
         Security). From then on .ebk files open with it; EPUB files dropped on EBK.app are converted.
Linux:   在终端里运行一次 ./ebk associate （撤销：./ebk associate --remove）。
         Run ./ebk associate once in a terminal (to undo: ./ebk associate --remove).

在阅读器和手机上读 / On e-readers and phones
--------------------------------------------
用装了 EBK 插件（ebk.koplugin）的 KOReader：Kobo、Kindle、Android 和 Linux。
（插件在电脑和 Android 上试过；Kobo、Kindle 上还没有在真机上试过。）
With KOReader and the EBK plug-in (ebk.koplugin): Kobo, Kindle, Android and Linux.
(The plug-in has been tried on a computer and on Android; not yet on a real Kobo or Kindle.)

其他命令 / Other commands:    ebk help
