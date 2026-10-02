--[[--
Opens EBK files (.ebk).

An EBK file holds the files of an EPUB book, packed smaller. KOReader's EPUB engine reads EPUB files, so this
plug-in writes the book as an EPUB file into KOReader's cache directory - with the `ebk` program next to this
file, or on Android with the library of the same code - and gives that file to the engine. To the rest of
KOReader the book is the .ebk file: history, reading position, notes and settings belong to it.
--]]--

local CreDocument = require("document/credocument")
local DataStorage = require("datastorage")
local Device = require("device")
local DocumentRegistry = require("document/documentregistry")
local InfoMessage = require("ui/widget/infomessage")
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local ffi = require("ffi")
local ffiUtil = require("ffi/util")
local lfs = require("libs/libkoreader-lfs")
local logger = require("logger")
local md5 = require("ffi/sha2").md5
local util = require("util")

local plugin_dir = ffiUtil.realpath(debug.getinfo(1, "S").source:match("^@(.*)/[^/]*$") or ".") or "."
local cache_dir = DataStorage:getDataDir() .. "/cache/ebk"
-- Books written as EPUB are kept for the next time, up to this many bytes; the least recently opened go first.
local CACHE_BYTES = (G_reader_settings:readSetting("ebk_cache_mb") or 256) * 1024 * 1024

local chinese = (G_reader_settings:readSetting("language") or ""):sub(1, 2) == "zh"
local function say(zh, en)
    return chinese and zh or en
end

-- The helper programs for this processor, the likeliest first.
local function programs()
    local arch = jit.arch
    if jit.os == "OSX" then
        return { "ebk-macos" }
    elseif arch == "x64" then
        return { "ebk-x86_64" }
    elseif arch == "arm64" then
        return { "ebk-aarch64", "ebk-armv7" }
    elseif arch == "arm" then
        -- the second one is for processors older than ARMv7 (the first Kindles)
        return { "ebk-armv7", "ebk-armv6" }
    end
    return {}
end

local library -- the loaded library, where it is used instead of a program
local function loadLibrary()
    if library then
        return library
    end
    ffi.cdef[[
        int ebk_to_epub(const char *ebk, const char *epub, int store, char *message, size_t message_len);
    ]]
    local name = os.getenv("EBK_PLUGIN_LIBRARY") or ("libebkffi-" .. (jit.arch == "arm64" and "aarch64" or jit.arch == "arm" and "armv7" or jit.arch) .. ".so")
    local path = name:sub(1, 1) == "/" and name or (plugin_dir .. "/lib/" .. name)
    if lfs.attributes(path, "mode") ~= "file" then
        error(say("这个插件里没有适合这台设备处理器的库", "the plug-in has no library for this device's processor") .. " (" .. name .. ")", 0)
    end
    if Device:isAndroid() then
        -- Android loads code only from the application's own directory, not from shared storage, where
        -- plug-ins are: keep a copy there (the current directory is the application's).
        local own = lfs.currentdir() .. "/libebkffi.so"
        if lfs.attributes(own, "size") ~= lfs.attributes(path, "size") or lfs.attributes(own, "modification") < lfs.attributes(path, "modification") then
            ffiUtil.copyFile(path, own)
        end
        path = own
    end
    library = ffi.load(path)
    return library
end

local use_library = Device:isAndroid() or os.getenv("EBK_PLUGIN_LIBRARY") ~= nil

--- Writes the book as `epub`. Returns true, or nil and the reason.
local function export(ebk, epub)
    if use_library then
        local ok, lib = pcall(loadLibrary)
        if not ok then
            return nil, tostring(lib)
        end
        local message = ffi.new("char[?]", 1024)
        if lib.ebk_to_epub(ebk, epub, 1, message, 1024) == 0 then
            return true
        end
        return nil, ffi.string(message)
    end
    local reason = say("这个插件里没有适合这台设备处理器的程序", "the plug-in has no program for this device's processor")
    for _, name in ipairs(programs()) do
        local program = plugin_dir .. "/bin/" .. name
        if lfs.attributes(program, "mode") == "file" then
            -- unpacked from a ZIP file the program may have lost the permission to run
            if lfs.attributes(program, "permissions"):sub(3, 3) ~= "x" then
                os.execute("chmod +x " .. util.shell_escape({ program }))
            end
            local pipe = io.popen(util.shell_escape({ program, "epub", ebk, "-o", epub, "--store" }) .. " 2>&1")
            local said = pipe and pipe:read("*a") or ""
            if pipe then
                pipe:close()
            end
            -- the program writes the file under another name first: it is there only when it is whole
            if lfs.attributes(epub, "mode") == "file" then
                return true
            end
            -- "ebk: <reason>" is the program's answer; anything else means it did not run here: try the next
            local answer = said:match("ebk: ([^\n]*)")
            if answer then
                return nil, answer
            end
            reason = say("程序无法在这台设备上运行", "the program does not run on this device") .. " (" .. name .. ")"
        end
    end
    return nil, reason
end

--- Removes what a run that was stopped left behind, and the books opened longest ago until the cache, with
--- `room` more bytes, keeps to its limit. `keep` is not removed.
local function tidyCache(room, keep)
    local files, total = {}, room
    for name in lfs.dir(cache_dir) do
        local path = cache_dir .. "/" .. name
        local attr = lfs.attributes(path)
        if attr and attr.mode == "file" then
            if name:match("%.epub$") then
                table.insert(files, { path = path, size = attr.size, time = attr.modification })
                total = total + attr.size
            elseif os.time() - attr.modification > 600 then
                os.remove(path) -- a file being written, of a run that did not finish
            else
                total = total + attr.size
            end
        end
    end
    table.sort(files, function(a, b) return a.time < b.time end)
    for _, file in ipairs(files) do
        if total <= CACHE_BYTES then
            break
        end
        if file.path ~= keep then
            os.remove(file.path)
            total = total - file.size
        end
    end
end

--- The EPUB file for an EBK file, written now unless it is there from an earlier time. Returns nil and the
--- reason if the book cannot be read.
local function epubFor(file)
    local attr = lfs.attributes(file)
    if not attr then
        return nil, say("文件不存在", "the file does not exist")
    end
    util.makePath(cache_dir)
    -- another file under the same name, or the same file changed, gets another EPUB
    local epub = cache_dir .. "/" .. md5(ffiUtil.realpath(file) .. "\n" .. attr.size .. "\n" .. attr.modification) .. ".epub"
    if lfs.attributes(epub, "mode") == "file" then
        lfs.touch(epub) -- opened now: the last to be removed
        return epub
    end
    -- KOReader also opens books in a second process, to read titles and covers for the file browser: if that
    -- one was writing the same book, the file is there now although this attempt was refused
    -- make room first, for about what the book will take: a full device would refuse it
    tidyCache(math.min(attr.size * 2, CACHE_BYTES))
    local ok, reason = export(file, epub)
    if not ok and lfs.attributes(epub, "mode") ~= "file" then
        return nil, reason
    end
    tidyCache(0, epub)
    return epub
end

local EbkDocument = CreDocument:extend{
    provider = "ebk",
    provider_name = "EBK",
}

function EbkDocument:init()
    local epub, reason = epubFor(self.file)
    if not epub then
        logger.warn("EBK: cannot read", self.file, reason)
        UIManager:show(InfoMessage:new{ text = say("无法打开这本 EBK 书：\n", "Cannot open this EBK book:\n") .. tostring(reason) })
        error("EBK: " .. tostring(reason))
    end
    self.ebk_epub = epub
    -- KOReader asks by this name whether a document is the EPUB engine's, and this one is
    self.provider = CreDocument.provider
    CreDocument.init(self)
end

function EbkDocument:loadDocument(full_document)
    if not self._loaded then
        if self._document:loadDocument(self.ebk_epub, full_document == false) then
            self._loaded = true
        else
            logger.warn("EBK: the engine cannot load", self.ebk_epub)
        end
    end
    return self._loaded
end

DocumentRegistry:addProvider("ebk", "application/x-ebk", EbkDocument, 100)

local Ebk = WidgetContainer:extend{
    name = "ebk",
    is_doc_only = false,
}

local started -- the first plug-in instance of this run has been made

local function isBook(file)
    return type(file) == "string" and util.getFileNameSuffix(file):lower() == "ebk" and lfs.attributes(file, "mode") == "file"
end

--- The EBK book KOReader was started with, if any: named on the command line, or the last one opened when
--- KOReader is set to start with that.
local function bookAtStart()
    local named -- a file or folder on the command line, where there is one: KOReader opens that
    for _, a in ipairs(type(arg) == "table" and arg or {}) do
        if lfs.attributes(a, "mode") then
            named = a
        end
    end
    if named then
        return isBook(named) and named or nil
    end
    local last = G_reader_settings:readSetting("lastfile")
    if G_reader_settings:readSetting("start_with") == "last" and isBook(last) then
        return last
    end
end

function Ebk:init()
    self.ui.menu:registerToMainMenu(self)
    if started then
        return
    end
    started = true
    -- KOReader looks for a reader for the book it starts with before it loads plug-ins. For an EBK book it
    -- finds none, says so and shows the file browser - which is what is being set up now. Open the book.
    local book = not self.ui.document and bookAtStart()
    if book then
        local _, name = util.splitFilePathName(book)
        for _, window in ipairs(type(UIManager._window_stack) == "table" and UIManager._window_stack or {}) do
            local widget = window.widget
            -- the message that the file is not supported
            if type(widget) == "table" and type(widget.text) == "string" and widget.text:find(name, 1, true) then
                UIManager:close(widget)
                break
            end
        end
        UIManager:nextTick(function()
            require("apps/reader/readerui"):showReader(book)
        end)
    end
end

function Ebk:addToMainMenu(menu_items)
    menu_items.ebk = {
        text = say("EBK 缓存", "EBK cache"),
        sorting_hint = "more_tools",
        callback = function()
            local count, bytes = 0, 0
            if lfs.attributes(cache_dir, "mode") == "directory" then
                for name in lfs.dir(cache_dir) do
                    local size = name:match("%.epub$") and lfs.attributes(cache_dir .. "/" .. name, "size")
                    if size then
                        count, bytes = count + 1, bytes + size
                    end
                end
            end
            local ConfirmBox = require("ui/widget/confirmbox")
            UIManager:show(ConfirmBox:new{
                text = string.format(say(
                    "打开 EBK 书时会先把它展开成 EPUB 放在缓存里，下次打开就不用再等。\n\n现在缓存了 %d 本，共 %.1f MB（上限约 %d MB）。\n\n清空缓存不会影响书、阅读进度和笔记。",
                    "An EBK book is written as an EPUB file into a cache when it is opened, so that the next time is fast.\n\n%d books are cached now, %.1f MB (the limit is about %d MB).\n\nEmptying the cache does not touch books, reading positions or notes."),
                    count, bytes / 1048576, CACHE_BYTES / 1048576),
                ok_text = say("清空缓存", "Empty the cache"),
                ok_callback = function()
                    if lfs.attributes(cache_dir, "mode") ~= "directory" then
                        return
                    end
                    local open = self.ui.document and self.ui.document.ebk_epub
                    for name in lfs.dir(cache_dir) do
                        local path = cache_dir .. "/" .. name
                        if path ~= open and lfs.attributes(path, "mode") == "file" then
                            os.remove(path)
                        end
                    end
                end,
            })
        end,
    }
end

return Ebk
