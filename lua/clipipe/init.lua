local M = {}

-- Format string for clipipe binary download
local github_rel_url =
  'https://github.com/bkoropoff/clipipe/releases/download/v%s/clipipe%s'
-- Map for determining release suffix for system
local system_map = {
  linux = {
    x86_64 = "-linux-x86_64"
  },
  windows_nt = {
    x86_64 = ".exe"
  }
}
-- Location where this plugin was checked out
local init_lua_path = debug.getinfo(1, "S").source:sub(2)
local plugin_path = vim.fs.normalize(vim.fs.dirname(init_lua_path) .. '/../..')
-- Our Cargo.toml
local cargo_toml_path = plugin_path .. '/Cargo.toml'
-- Platform checks
local is_win_native = vim.fn.has("win32") == 1
local is_wsl = vim.fn.has("wsl") == 1
local is_win = is_win_native or is_wsl
-- Where cargo will put clipipe binary
local cargo_release_bin = plugin_path .. '/target/release/clipipe' .. (is_win_native and ".exe" or "")

-- Cache for LOCALAPPDATA on Windows
local _localappdata = nil
-- Cache for version read from Cargo.toml
local _version = nil

-- Default configuration
local defaults = {
  path = nil,
  -- Convert line endings on Windows
  keep_line_endings = false,
  -- Enable on setup
  enable = true,
  -- Start timeout (ms)
  start_timeout = 5000,
  -- Timeout waiting for response (ms)
  timeout = 500,
  -- Interval to poll for response (ms)
  interval = 50,
  -- Build clipipe binary from source, if necessary and possible
  build = true,
  -- Download clipipe binary, if necessary and possible
  download = true
}

local config = defaults

local IN_PROGRESS = {}

-- State for the background job
local state = {
  proc = nil,
  buffer = {},
  request = false,
  response = nil,
  callback = nil
}

local function completed_to_source(obj)
  local stderr = obj.stderr
  if not stderr or stderr == '' then
    return 'exit code ' .. obj.code
  end
  return stderr:gsub('%s*$', '')
end

local function canon_error(error)
  if type(error) == 'string' then
    return { message = error }
  end
  return {
    message = error.message or "unknown error",
    source = error.source and canon_error(error.source)
  }
end

local function make_error(message, source)
  return canon_error { message = message, source = source }
end

local function format_error(error)
  local base = {error.message}
  local source = error.source
  if not source then
    return {base}
  end

  local rest
  if type(source) == 'table' then
    rest = format_error(source)
  else
    rest = {{source}}
  end

  return {base, {": "}, unpack(rest)}
end

local function notify_error(message, source)
  local error = make_error(message, source)

  if config.notify_error then
    config.notify_error(error)
    return
  end

  vim.schedule(function()
    vim.api.nvim_echo({{"clipipe: "}, unpack(format_error(error))}, true, {})
  end)
end

local function notify(msg, level)
  if config.notify then
    config.notify(msg, level)
    return
  end
  vim.schedule(function()
    vim.notify("clipipe: " .. msg, level)
  end)
end

local function reset(proc)
  state.proc = nil
  state.buffer = {}
  state.request = false
  state.response = nil
  state.callback = nil

  if proc then
    local timer = vim.uv.new_timer()
    if not timer then
      error("failed to create timer")
    end
    proc:kill('TERM')
    timer:start(config.timeout, 0, function()
      proc:kill('KILL')
    end)
  end
end

-- Start background process if not already running
local function start()
  -- Already in progress?
  if state.proc == IN_PROGRESS then
    -- Try waiting a short interval to see if it's ready
    local ok = vim.wait(config.interval,
      function() return state.proc ~= IN_PROGRESS end, config.interval)
    if not ok then
      return false, IN_PROGRESS
    end
  end

  -- Already started
  if state.proc then
    return true
  end

  -- Can't start binary if we don't have it
  if not config.path or config.path == "" then
    return false, "binary not found"
  end

  -- Prep startup timeout timer
  local timer = vim.uv.new_timer()
  if not timer then
    return false, "failed to create timer"
  end

  local cmd = { config.path }
  if is_win and config.keep_line_endings then
    table.insert(cmd, "--keep-line-endings")
  end

  -- Run clipipe
  local ok, proc = pcall(vim.system, cmd, {
      text = true,
      stdin = true,
      stderr = true,
      -- Output handler
      stdout = function(err, data)
        if err then
          notify_error("couldn't read stdout", err)
          return
        end
        if not data or not state.proc then
          return
        end
        -- Does this chunk complete a line?
        local idx = string.find(data, "\n", 1, true)
        if idx then
          -- We should only receive a response to a request
          if not state.request then
            notify_error("spurious data", data)
            return
          end
          state.request = false

          -- Split out data after the newline
          local pre = string.sub(data, 1, idx)
          data = string.sub(data, idx + 1)
          -- Form complete line from chunks
          table.insert(state.buffer, pre)
          local stdout = table.concat(state.buffer)
          -- Save remainder as new buffer table
          state.buffer = { data }

          -- Parse it
          local ok, response = pcall(vim.json.decode, stdout)
          if not ok then
            response = {
              success = false,
              message = "couldn't decode JSON response",
              source = response
            }
          end

          -- Decide what to do with it
          local cb = state.callback
          if cb then
            state.callback = nil
            cb(response)
          else
            state.response = response
          end
        else
          table.insert(state.buffer, data)
        end
      end,
    },
    function(obj)
      local cb = state.callback
      reset()

      local err = completed_to_source(obj)
      if cb then
        cb({ success = false, message = "clipipe terminated", source = err })
      else
        notify_error("terminated", err)
      end
    end)
  if not ok then
    return false, make_error("failed to start clipipe", proc)
  end

  state.proc = IN_PROGRESS
  state.request = true
  state.callback = function(response)
    timer:stop()
    if response.success then
      state.proc = proc
    else
      notify_error(response.message, response.source)
      reset(proc)
    end
  end

  timer:start(config.start_timeout, 0, function()
    if state.proc == IN_PROGRESS then
      notify_error("timed out on start")
      reset(proc)
    end
  end)

  local err
  ok, err = pcall(function()
    proc:write(vim.json.encode { action = "query" } .. "\n")
  end)
  if not ok then
    reset(proc)
    return nil, make_error("couldn't write request", err)
  end

  -- Try waiting a short interval to see if it's ready
  ok = vim.wait(config.interval, function() return state.proc ~= IN_PROGRESS end,
    config.interval)
  if not ok then
    return false, IN_PROGRESS
  end

  return true
end

-- Send a request, get the response
local function transact(request)
  local ok, err = start()
  if not ok then
    return nil, err
  end

  -- Only one request can be outstanding at a time
  if state.request or state.response then
    return nil, "overlapped request attempted"
  end
  state.request = true

  -- Write request to pipe
  ok, err = pcall(function()
    state.proc:write(vim.json.encode(request) .. "\n")
  end)
  if not ok then
    state.request = false
    return nil, make_error("couldn't write request", err)
  end

  -- Wait for a response
  ok = vim.wait(config.timeout, function() return state.response ~= nil end, config.interval)
  if not ok then
    reset(state.proc)
    state.request = false
    return nil, "timed out waiting for response"
  end

  local response = state.response
  state.response = nil
  if not response.success then
    return nil, response
  end
  return response, nil
end

-- Get plugin version from Cargo.toml
local function version()
  if _version then
    return _version
  end

  local lines = vim.fn.readfile(cargo_toml_path)

  for _, line in ipairs(lines) do
    local v = line:match('^%s*version%s*=%s*["\']([^"\']+)["\']%s*$')
    if v then
      _version = v
      return v
    end
  end

  error("Could not read plugin version from Cargo.toml")
end

-- Get value of Windows environment variable from within WSL
local function wsl_env_get(var)
  local res = vim.system({ 'cmd.exe', '/c', 'echo %' .. var .. '%' }, { stdout = true })
    :wait()
  return res.stdout:gsub("%s+$", "")
end

-- Find path to LOCALAPPDATA directory
local function win_localappdata()
  if not _localappdata then
    if is_wsl then
      _localappdata = wsl_env_get('LOCALAPPDATA'):gsub("^(%a):\\", "/mnt/%1/"):gsub(
        "\\", "/"):lower()
    else
      _localappdata = vim.env.LOCALAPPDATA:gsub("\\", "/")
    end
  end
  return _localappdata
end

-- Get downloaded clipipe.exe location
local function download_path()
  if is_win then
    return win_localappdata() .. '/clipipe/clipipe.exe'
  else
    return plugin_path .. '/clipipe'
  end
end

-- Find clipipe binary
local function find_bin()
  local path = config.path or vim.fn.exepath(is_win and "clipipe.exe" or "clipipe")
  if path and path ~= '' and vim.uv.fs_stat(path) then
    return path
  end
  if vim.uv.fs_stat(cargo_release_bin) then
    return cargo_release_bin
  end
  path = download_path()
  if vim.uv.fs_stat(path) then
    return path
  end
  return nil
end

local function system(cfg)
  local opts = { cwd = cfg.cwd }
  for _, key in ipairs { 'stdout', 'stderr' } do
    if cfg[key] == 'notify' then
      opts[key] = function(err, data)
        if err then
          notify_error("couldn't read output", err)
          return
        end
        if not data then
          return
        end
        local parts = vim.split(data, "\n", { plain = true, trimempty = true })
        notify(vim.trim(parts[#parts]))
      end
    elseif cfg[key] then
      opts[key] = true
    end
  end

  opts.stderr = opts.stderr or true

  local cr = coroutine.running()

  local ok = pcall(function()
    vim.system(cfg.command, opts,
      function(res)
        if res.code ~= 0 then
          local err = make_error("error running " .. vim.inspect(cfg.command),
            completed_to_source(res))
          vim.schedule(function() coroutine.resume(cr, nil, err) end)
        else
          vim.schedule(function() coroutine.resume(cr, res) end)
        end
      end)
  end)
  if not ok then
    return nil, make_error("failed to run " .. vim.inspect(cfg.command))
  end

  return coroutine.yield()
end

-- Query information from clipipe binary
local function query_bin(path)
  local res, err = system {
    command = { path, '--query' }
  }
  if not res then
    return nil, err
  end

  return vim.json.decode(res.stdout)
end

-- Verify clipipe binary is usable (version matches plugin)
local function verify_bin(path)
  local ver = version()
  local response, err = query_bin(path)
  if not response then
    return false, err
  end
  if response.version ~= ver then
    return false,
      make_error("version mismatch",
        "binary version (" ..
        response.version .. ") doesn't match plugin (" .. ver .. ")")
  end
  return true
end

-- Attempt to build clipipe binary
local function build_bin()
  if is_wsl then
    -- It's possible to cross-compile from within WSL or invoke a host Rust
    -- installation, but this is a niche scenario that's currently not
    -- supported since downloading the prebuilt binary suffices
    return nil
  end

  local cargo = is_win and "cargo.exe" or "cargo"

  if vim.fn.exepath(cargo) == "" then
    return nil
  end

  notify("building with cargo")

  local ok, err = system {
    command = { cargo, "build", "--release" },
    stderr = 'notify',
    cwd = plugin_path
  }
  if not ok then
    notify_error("failed to build", err)
    return nil
  end
  notify("build successful")
  return cargo_release_bin
end

-- Attempt to download prebuilt clipipe binary
local function download_bin()
  -- Grab system info
  local info = vim.uv.os_uname()
  local os = is_wsl and "windows_nt" or info.sysname:lower()
  local cpu = info.machine

  -- Determine URL suffix for download
  local suffix = (system_map[os] or {})[cpu]
  if not suffix then
    -- Prebuilt binary not available
    return nil
  end

  -- What to download, how, and where
  local url = github_rel_url:format(version(), suffix)
  local curl = is_win_native and "curl.exe" or "curl"
  local path = download_path()

  -- Ensure destination directory exists
  vim.fn.mkdir(vim.fn.fnamemodify(path, ':h'), "p")


  -- Do it
  notify("downloading binary")
  local ok, err = system {
    command = { curl, '--no-progress-meter', '-f', '-L', '-o', path, url },
    status = "downloading clipipe binary"
  }
  if not ok then
    notify_error("failed to download", err)
    return nil
  end

  if not is_win then
    vim.uv.fs_chmod(path, 448)
  end

  notify("download successful")
  return path
end

local function build()
  if state.proc then
    -- Rebuilding while running is a bad idea, defer to next restart
    return
  end

  local cr = coroutine.create(function()
    -- Look for existing clipipe binary first
    local path = find_bin()
    if path then
      -- Verify it's usable
      local ok, err = verify_bin(path)
      if not ok then
        notify("ignoring " .. path .. ": " .. (err or "unknown error"),
          vim.log.levels.INFO)
        path = nil
      end
    end
    -- Download it?
    if not path and config.download then
      path = download_bin()
    end
    -- Build it?
    if not path and config.build then
      path = build_bin()
    end
    config.path = path
    state.proc = nil
  end)
  state.proc = IN_PROGRESS
  vim.schedule(function() coroutine.resume(cr) end)
end

-- Register to clipboard identifier mapping
local reg_to_clipboard = {
  ['+'] = "clipboard",
  ['*'] = "primary"
}

-- Copy function suitable for g:clipboard
function M.copy(lines, dest)
  local data = table.concat(lines, "\n")
  local request = { action = "copy", data = data, clipboard = reg_to_clipboard[dest] or dest }
  local response, err = transact(request)
  if not response then
    if err == IN_PROGRESS then
      notify("waiting for startup", vim.log.levels.INFO)
    else
      notify_error("copy failed", err)
    end
  end
end

-- Paste function suitable for g:clipboard
function M.paste(source)
  local request = { action = "paste", clipboard = reg_to_clipboard[source] or source }
  local response, err = transact(request)
  if not response then
    if err == IN_PROGRESS then
      notify("waiting for startup", vim.log.levels.INFO)
    else
      notify_error("paste failed", err)
    end
    return {}
  end
  return vim.split(response.data, "\n", { plain = true })
end

-- Plugin setup
function M.setup(user_config)
  config = vim.tbl_extend("force", defaults, user_config or {})

  config.path = config.path or find_bin()
  if not config.path and config.download or config.build then
    -- Run build now in case it hasn't happened already
    build()
  end

  if config.enable then
    M.enable()
  end
end

-- Enable plugin (configure g:clipboard)
function M.enable()
  vim.g.clipboard = {
    name = "clipipe",
    copy = {
      ["+"] = function(lines) M.copy(lines, '+') end,
      ["*"] = function(lines) M.copy(lines, '*') end,
    },
    paste = {
      ["+"] = function() return M.paste('+') end,
      ["*"] = function() return M.paste('*') end,
    }
  }

  -- Eagerly kick off background process
  start()
end

return M
