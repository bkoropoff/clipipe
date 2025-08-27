local root = arg[1]
local clipipe = arg[2]
local test = arg[3]

-- Add plugin root to runtime path
vim.opt.runtimepath:prepend(root)

-- Minimal clipipe setup
require 'clipipe'.setup {
    path = clipipe,
    build = false,
    download = false,
    enable = false
}

-- Harness functions
function _G.assert_eq(a, b)
    if a ~= b then
        error("Assertion failed: " .. vim.inspect(a) .. " ~= " .. vim.inspect(b))
    end
end

dofile(test)
