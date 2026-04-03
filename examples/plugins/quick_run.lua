-- quick_run.lua — Run the current file based on its language.
--
-- :Run executes the file with the appropriate interpreter/compiler.
-- Install: copy to ~/.config/novim/plugins/

novim.register_command("Run", function(args)
    local path = novim.buf.path()
    if not path then
        novim.ui.status("No file to run")
        return
    end

    local ext = path:match("%.(%w+)$")
    local cmd = nil

    if ext == "py" then
        cmd = "python3 " .. path
    elseif ext == "js" then
        cmd = "node " .. path
    elseif ext == "ts" then
        cmd = "npx tsx " .. path
    elseif ext == "sh" then
        cmd = "bash " .. path
    elseif ext == "lua" then
        cmd = "lua " .. path
    elseif ext == "rs" then
        cmd = "cargo run 2>&1"
    elseif ext == "go" then
        cmd = "go run " .. path
    end

    if cmd then
        local output = novim.fn.shell(cmd)
        -- Show first line of output
        local first_line = output:match("([^\n]*)")
        novim.ui.status("$ " .. cmd .. " → " .. (first_line or ""))
    else
        novim.ui.status("No runner for ." .. (ext or "?"))
    end
end)
