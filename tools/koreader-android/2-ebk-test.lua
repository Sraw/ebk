-- test driver for Android: what to do is in ebk-test.conf next to the patches directory
local UIManager = require("ui/uimanager")
local Device = require("device")
local Event = require("ui/event")
local dir = require("datastorage"):getDataDir()
local conf = {}
local f = io.open(dir .. "/ebk-test.conf")
if f then for line in f:lines() do local k, v = line:match("^(%w+)=(.*)$"); if k then conf[k] = v end end f:close() end
if conf.start then G_reader_settings:saveSetting("start_with", conf.start) end
local name = conf.name or "run"
local pages = {}
for page in (conf["goto"] or ""):gmatch("%d+") do table.insert(pages, tonumber(page)) end
local delay = tonumber(conf.delay or "12")
for i, page in ipairs(pages) do
    UIManager:scheduleIn(delay + i - 1, function()
        local ui = require("apps/reader/readerui").instance
        if ui then
            ui:handleEvent(Event:new("GotoPage", page))
            UIManager:forceRePaint()
            Device.screen:shot(dir .. "/ebk-test-" .. name .. "." .. page .. ".png")
        end
    end)
end
UIManager:scheduleIn(delay + #pages + 1, function()
    local out = io.open(dir .. "/ebk-test-" .. name .. ".txt", "w")
    local ui = require("apps/reader/readerui").instance
    if ui and ui.document then
        out:write("file=", tostring(ui.document.file), "\n")
        out:write("pages=", tostring(ui.document:getPageCount()), "\n")
        out:write("page=", tostring(ui:getCurrentPage()), "\n")
        out:write("title=", tostring(ui.doc_props and ui.doc_props.display_title), "\n")
        out:write("toc=", tostring(#(ui.toc and ui.toc.toc or {})), "\n")
        out:write("epub=", tostring(ui.document.ebk_epub), "\n")
    else
        out:write("file=none\n")
    end
    out:write("arg=", type(arg) == "table" and table.concat(arg, "|") or tostring(arg), "\n")
    out:write("cwd=", require("libs/libkoreader-lfs").currentdir(), "\n")
    for _, window in ipairs(UIManager._window_stack) do
        if type(window.widget.text) == "string" then out:write("message=", (window.widget.text:gsub("\n", " ")), "\n") end
    end
    out:close()
    if ui then ui:onClose() end
    UIManager:quit(0)
end)
