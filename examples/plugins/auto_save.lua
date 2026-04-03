-- auto_save.lua — Automatically save dirty buffers when leaving insert mode.
--
-- Install: copy to ~/.config/novim/plugins/

novim.on("ModeChanged", function(args)
    if args.from == "INSERT" and args.to == "NORMAL" then
        if novim.buf.is_dirty() then
            novim.exec("w")
        end
    end
end)
