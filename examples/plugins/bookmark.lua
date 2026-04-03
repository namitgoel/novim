-- bookmark.lua — Simple line bookmarks with keybindings.
--
-- Ctrl+b in normal mode: toggle bookmark at current line
-- :Bookmarks: list all bookmarks in status bar
-- Install: copy to ~/.config/novim/plugins/

local bookmarks = {}

novim.keymap("NORMAL", "Ctrl+b", function()
    local cur = novim.buf.cursor()
    local path = novim.buf.path() or ""
    local key = path .. ":" .. cur.line

    if bookmarks[key] then
        bookmarks[key] = nil
        novim.ui.status("Bookmark removed: line " .. (cur.line + 1))
    else
        bookmarks[key] = { path = path, line = cur.line }
        novim.ui.status("Bookmark set: line " .. (cur.line + 1))
    end
end)

novim.register_command("Bookmarks", function(args)
    local items = {}
    for key, bm in pairs(bookmarks) do
        items[#items + 1] = bm.path .. ":" .. (bm.line + 1)
    end
    if #items == 0 then
        novim.ui.status("No bookmarks")
    else
        novim.ui.status("Bookmarks: " .. table.concat(items, " | "))
    end
end)
