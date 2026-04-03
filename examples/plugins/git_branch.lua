-- git_branch.lua — Show current git branch on file open.
--
-- Install: copy to ~/.config/novim/plugins/

novim.on("BufOpen", function(args)
    local branch = novim.fn.shell("git branch --show-current 2>/dev/null"):gsub("\n", "")
    if branch ~= "" then
        novim.ui.status("Branch: " .. branch)
    end
end)
