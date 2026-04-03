-- zen_mode.lua — Toggle distraction-free mode.
--
-- :Zen toggles line numbers and wrap for focused writing.
-- Install: copy to ~/.config/novim/plugins/

local zen_active = false
local saved_wrap = false
local saved_ln = "hybrid"

novim.register_command("Zen", function(args)
    if zen_active then
        -- Restore previous settings
        novim.opt.set("word_wrap", saved_wrap)
        novim.opt.set("line_numbers", saved_ln)
        novim.ui.status("Zen mode OFF")
        zen_active = false
    else
        -- Save current settings and enter zen
        saved_wrap = novim.opt.get("word_wrap")
        saved_ln = novim.opt.get("line_numbers")
        novim.opt.set("word_wrap", true)
        novim.opt.set("line_numbers", "off")
        novim.ui.status("Zen mode ON")
        zen_active = true
    end
end)
