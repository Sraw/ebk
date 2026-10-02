KOReader 的 EBK 插件 / EBK plug-in for KOReader
==============================================

装上以后，KOReader 能像打开 EPUB 一样打开 .ebk 书：目录、跳转、脚注、进度、笔记都照常。
With it KOReader opens .ebk books like EPUB books: contents, links, footnotes, reading position, notes.

安装 / Installing
-----------------
把整个 ebk.koplugin 文件夹复制到 KOReader 的 plugins 文件夹里，然后重启 KOReader。
Copy the whole folder ebk.koplugin into KOReader's plugins folder, then restart KOReader.

  Kobo:     .adds/koreader/plugins/
  Kindle:   koreader/plugins/
  Android:  koreader/plugins/   （在内部存储里，没有就新建 / on the internal storage; create it if missing）
            只支持 ARM 处理器的设备（绝大多数手机和平板）/ devices with ARM processors only (nearly all)
  Linux:    ~/.config/koreader/plugins/
  macOS:    ~/Library/Application Support/koreader/plugins/

使用 / Using
------------
在文件浏览器里点 .ebk 书即可。第一次打开一本书要等一下（图片多的书在电子书阅读器上可能要十几秒），
之后再打开就快了。
Tap an .ebk book in the file browser. The first time a book is opened takes a moment (a book with many
pictures may take ten seconds or more on an e-reader); after that it opens fast.

插件把书展开成 EPUB 放在 KOReader 的缓存里（cache/ebk，默认大约 256 MB，超出后最久没打开的先删；
正在读的那本不会删）。文件浏览器显示封面时也会在后台展开文件夹里的 EBK 书。
"工具 → 更多工具 → EBK 缓存"里可以查看和清空。清空不影响书、进度和笔记。
The plug-in keeps each opened book as an EPUB file in KOReader's cache (cache/ebk, about 256 MB by default;
beyond that the books opened longest ago are removed, never the one being read). The file browser, when it
shows covers, also prepares the EBK books of a folder in the background. See and empty the cache under
Tools → More tools → EBK cache. Emptying it does not touch books, reading positions or notes.
