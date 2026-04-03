-- bookmark.lua — Simple line bookmarks with keybindings.
--
-- Ctrl+b in normal mode: toggle bookmark at current line
-- :Bookmarks: list all bookmarks in a selectable popup (Enter to jump)
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
    local refs = {}
    for key, bm in pairs(bookmarks) do
        items[#items + 1] = bm.path .. ":" .. (bm.line + 1)
        refs[#refs + 1] = bm
    end
    if #items == 0 then
        novim.ui.popup("Bookmarks", {"No bookmarks set.", "", "Use Ctrl+b to add bookmarks."}, { width = 40, height = 7 })
    else
        novim.ui.popup("Bookmarks", items, {
            width = 60,
            on_select = function(index, text)
                local bm = refs[index]
                if bm then
                    -- Open the file and jump to the line
                    novim.exec("e " .. bm.path)
                    novim.buf.set_cursor(bm.line, 0)
                    novim.ui.status("Jumped to bookmark: " .. text)
                end
            end
        })
    end
end)
