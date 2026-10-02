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

怎么读 .ebk / Reading .ebk books
--------------------------------
用装了 EBK 插件（ebk.koplugin）的 KOReader，支持 Kobo、Kindle、Android 和电脑。
With KOReader and the EBK plug-in (ebk.koplugin): Kobo, Kindle, Android and desktop computers.

其他命令 / Other commands:    ebk help
