local root = arg[1]
local binary = arg[2]
local test = arg[3]

local errors = {}

local stderr = vim.uv.new_pipe();
stderr:open(2)

function _G.notify(message, level)
    vim.uv.write(stderr, message .. "\n")
end

local function notify_error(error)
  table.insert(errors, error)
end

local defaults = {
  path = binary,
  build = false,
  download = false,
  enable = false,
  start_timeout = 100,
  timeout = 100,
  notify_error = notify_error,
  notify = _G.notify
}

-- Add plugin root to runtime path
vim.opt.runtimepath:prepend(root)

local clipipe = require 'clipipe'

-- Minimal clipipe setup
clipipe.setup(defaults)

local config = vim.tbl_extend('keep', defaults, {})

function _G.setup(spec)
  config = vim.tbl_extend('keep', spec, config)
  clipipe.setup(config)
end

function _G.mock(spec)
  vim.env.CLIPIPE_MOCK = vim.json.encode(spec)
  setup {
    path = vim.fn.fnamemodify(binary, ":h") .. '/examples/mock'
  }
end

local function deep_equal(a, b)
  if type(a) == 'table' and type(b) == 'table' then
    for k, v in pairs(a) do
      if not deep_equal(b[k], v) then
        return false
      end
    end
    for k, _ in pairs(b) do
      if a[k] == nil then
        return false
      end
    end
    return true
  end
  return a == b
end

-- Harness functions
function _G.assert_eq(a, b)
  if not deep_equal(a, b) then
    error("Assertion failed: " .. vim.inspect(a) .. " ~= " .. vim.inspect(b))
  end
end

function _G.expect_error(message, source)
  local found
  local needle = { message = message, source = source }
  for i, error in ipairs(errors) do
    if deep_equal(error, needle) then
      found = i
      break
    end
  end
  if found then
    table.remove(errors, found)
  else
    error("Expected error not found: " .. message)
  end
end

function _G.sleep(ms)
  vim.wait(ms, function() return false end)
end

dofile(test)

if #errors ~= 0 then
  error("Unexpected errors: " .. vim.inspect(errors))
end
