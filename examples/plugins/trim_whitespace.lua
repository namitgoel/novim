-- trim_whitespace.lua — Strip trailing whitespace on save.
--
-- Install: copy to ~/.config/novim/plugins/

novim.on("BufWrite", function(args)
    local count = novim.buf.line_count()
    local lines = novim.buf.get_lines(0, count)
    local trimmed = {}
    local changed = false
    for i = 1, #lines do
        local line = lines[i]:gsub("%s+$", "")
        if line ~= lines[i] then
            changed = true
        end
        trimmed[i] = line
    end
    if changed then
        novim.buf.set_lines(0, count, trimmed)
        novim.ui.status("Trimmed trailing whitespace")
    end
end)
