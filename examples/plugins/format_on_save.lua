-- format_on_save.lua — Auto-format files on save using external formatters.
--
-- Supported: Rust (rustfmt), Python (black), JavaScript/TypeScript (prettier)
-- Install: copy to ~/.config/novim/plugins/

novim.on("BufWrite", { pattern = "*.rs" }, function(args)
    local path = novim.buf.path()
    if path then
        novim.fn.shell("rustfmt " .. path .. " 2>/dev/null")
        novim.ui.status("Formatted with rustfmt")
    end
end)

novim.on("BufWrite", { pattern = "*.py" }, function(args)
    local path = novim.buf.path()
    if path then
        novim.fn.shell("black -q " .. path .. " 2>/dev/null")
        novim.ui.status("Formatted with black")
    end
end)

novim.on("BufWrite", { pattern = "*.js" }, function(args)
    local path = novim.buf.path()
    if path then
        novim.fn.shell("prettier --write " .. path .. " 2>/dev/null")
        novim.ui.status("Formatted with prettier")
    end
end)

novim.on("BufWrite", { pattern = "*.ts" }, function(args)
    local path = novim.buf.path()
    if path then
        novim.fn.shell("prettier --write " .. path .. " 2>/dev/null")
        novim.ui.status("Formatted with prettier")
    end
end)
