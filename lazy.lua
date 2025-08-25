return {
    "bkoropoff/clipipe",
    opts = {
        path = nil,
        keep_line_endings = false,
        enable = true,
        timeout = 1000,
        interval = 10,
        build = true,
        download = true
    },
    build = function(plugin)
        local lazy_plugin = require 'lazy.core.plugin'
        local opts = vim.tbl_extend('keep', { yield = true }, lazy_plugin.values(plugin, "opts") or {})
        require 'clipipe'.build(opts)
    end
}
