local clipipe = require 'clipipe'
local text = "foobar"

clipipe.enable()
vim.fn.setreg("+", text)
local res = vim.fn.getreg("+")
assert_eq(text, text)
