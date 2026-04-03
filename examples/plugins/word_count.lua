-- word_count.lua — Show word/line/char count with a keymap.
--
-- Press Ctrl+g in normal mode to see file stats.
-- Install: copy to ~/.config/novim/plugins/

novim.keymap("NORMAL", "Ctrl+g", function()
    local text = novim.buf.get_text()
    local lines = novim.buf.line_count()
    local chars = #text
    local words = 0
    for _ in text:gmatch("%S+") do
        words = words + 1
    end
    local path = novim.buf.path() or "[No Name]"
    novim.ui.status(path .. " — " .. lines .. " lines, " .. words .. " words, " .. chars .. " chars")
end)
