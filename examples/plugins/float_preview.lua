-- float_preview.lua — Floating window examples.
--
-- :Preview     Open a floating preview of the current file stats
-- :Changelog   Show a scrollable changelog in a floating window
-- Ctrl+h       Quick help float with keybinding cheatsheet
--
-- Install: copy to ~/.config/novim/plugins/

-- :Preview — file stats in a floating window
novim.register_command("Preview", function(args)
    local path = novim.buf.path() or "[No Name]"
    local lines = novim.buf.line_count()
    local text = novim.buf.get_text()
    local chars = #text
    local words = 0
    for _ in text:gmatch("%S+") do
        words = words + 1
    end

    local dirty = novim.buf.is_dirty() and " [Modified]" or ""

    novim.ui.float("File Preview", {
        "",
        "  File:    " .. path .. dirty,
        "  Lines:   " .. lines,
        "  Words:   " .. words,
        "  Chars:   " .. chars,
        "  Size:    " .. string.format("%.1f KB", chars / 1024),
        "",
        "  Press q or Esc to close",
    }, { width = 50, height = 12 })
end)

-- :Changelog — scrollable content in a float
novim.register_command("Changelog", function(args)
    local entries = {
        "v2.1.0 — Tree-sitter & Floating Windows",
        "=========================================",
        "",
        "New Features:",
        "  - Tree-sitter symbol navigation (Ctrl+T, :symbols)",
        "  - Floating windows (novim.ui.float() plugin API)",
        "  - Quickfix list (:copen, :cnext, :cprev, :make)",
        "  - Command window (q:)",
        "  - gq format text with motion",
        "  - Plugin manifest (plugin.toml)",
        "",
        "v2.0.0 — Vim/tmux/WezTerm Parity",
        "==================================",
        "",
        "  - Tab completion in : mode",
        "  - Ctrl+A/Ctrl+X increment/decrement",
        "  - gU/gu + motion case operations",
        "  - :read, :sort, :w !cmd, ==",
        "  - 24-bit true color terminal",
        "  - OSC 8 clickable hyperlinks",
        "  - OSC 133 prompt markers",
        "  - Copy mode selection (v/y)",
        "  - Status bar customization",
        "",
        "  j/k to scroll, q or Esc to close",
    }
    novim.ui.float("Changelog", entries, { width = 55, height = 20 })
end)

-- Ctrl+h — quick help cheatsheet float
novim.keymap("NORMAL", "Ctrl+h", function()
    novim.ui.float("Quick Reference", {
        "",
        "  Navigation         Editing          Panes",
        "  ----------         -------          -----",
        "  hjkl   move        i/a    insert    Ctrl+W s  split H",
        "  w/b/e  word        o/O    open      Ctrl+W v  split V",
        "  gg/G   file        dd     delete    Ctrl+W q  close",
        "  f/t    find        cc     change    Ctrl+W z  zoom",
        "  %      bracket     u      undo      Ctrl+W t  terminal",
        "  */#    search      .      repeat",
        "  Ctrl+T symbols     gq     format    Commands",
        "  H/M/L  screen      >>     indent    --------",
        "                                       :symbols  list fns",
        "  Search             Visual            :make     build",
        "  ------             ------            :cn/:cp   quickfix",
        "  /pat   search      v      select    :sort     sort",
        "  n/N    next/prev   >/<    indent    :read     insert",
        "  :%s    replace     ~/U/u  case      q:        cmd history",
        "",
        "  Press q or Esc to close",
    }, { width = 58, height = 22 })
end)
