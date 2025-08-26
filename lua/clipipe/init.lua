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
-- Where cargo will put clipipe binary (on Linux)
local cargo_release_bin = plugin_path .. '/target/release/clipipe'
-- Platform checks
local is_win_native = vim.fn.has("win32") == 1
local is_wsl = vim.fn.has("wsl") == 1
local is_win = is_win_native or is_wsl

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
    interval = 10,
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

-- Delayed notify
local function notify(msg, level)
    vim.schedule(function()
        if type(level) == 'string' then
            -- Bypass vim.notify, go straight to messages
            vim.api.nvim_echo({{msg, level}}, true, {})
        else
            vim.notify(msg, level)
        end
    end)
end

local function reset(proc)
    state.proc = nil
    state.buffer = {}
    state.request = false
    state.response = nil
    state.callback = nil

    if proc then
        proc:kill('TERM')
        proc:wait(100)
    end
end

-- Start background process if not already running
local function start()
    if state.proc == IN_PROGRESS then
        local ok = vim.wait(config.timeout,
            function() return state.proc ~= IN_PROGRESS end, config.interval)
        if not ok then
            return false, IN_PROGRESS
        end
    end

    if state.proc then
        return true
    end

    if not config.path or config.path == "" then
        return false, "Binary not found"
    end

    local timer = vim.uv.new_timer()
    if not timer then
        return false, "Failed to create timer"
    end

    local cmd = { config.path }
    if is_win and config.keep_line_endings then
        table.insert(cmd, "--keep-line-endings")
    end

    local ok, proc = pcall(vim.system, cmd, {
            text = true,
            stdin = true,
            stderr = true,
            stdout = function(err, data)
                if err then
                    notify("clipipe: stdout error: " .. err, 'ErrorMsg')
                    return
                end
                if not data then
                    return
                end
                local idx = string.find(data, "\n", 1, true)
                if idx then
                    if not state.request then
                        notify("clipipe: spurious data: " .. data)
                        return
                    end
                    local pre = string.sub(data, 1, idx)
                    data = string.sub(data, idx + 1)
                    table.insert(state.buffer, pre)
                    local stdout = table.concat(state.buffer)
                    local parsed = vim.json.decode(stdout)
                    state.buffer = { data }
                    state.request = false
                    local cb = state.callback
                    if cb then
                        state.callback = nil
                        cb(parsed)
                    else
                        state.response = parsed
                    end
                else
                    table.insert(state.buffer, data)
                end
            end,
        },
        function(obj)
            reset()

            local err = nil
            if obj.code ~= 0 then
                local stderr = obj.stderr or "unknown"
                err = stderr:gsub('%s*$', '') or ""
            end

            local cb = state.callback
            if cb then
                state.callback = nil
                cb(nil, err)
            else
                notify(
                    "clipipe: Process exited with error: " .. err,
                    'ErrorMsg')
            end
        end)
    if not ok then
        return false, "Failed to start clipipe process: " .. (proc or "unknown error")
    end

    state.proc = IN_PROGRESS
    state.request = true
    state.callback = function(response, err)
        timer:stop()
        if response then
            state.proc = proc
        else
            notify("clipipe: Failed to start clipipe: " .. err, "ErrorMsg")
        end
    end

    timer:start(config.start_timeout, 0, function()
        if state.proc == IN_PROGRESS then
            notify("clipipe: Timed out waiting for startup", "ErrorMsg")
            reset(proc)
        end
    end)

    local err
    ok, err = pcall(function()
        proc:write(vim.json.encode { action = "query" } .. "\n")
    end)
    if not ok then
        reset(proc)
        return nil, "Failed to send query: " .. (err or "unknown error")
    end

    ok = vim.wait(config.timeout, function() return state.proc ~= IN_PROGRESS end,
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

    if state.request or state.response then
        return nil, "Overlapped request attempted"
    end
    state.request = true

    ok, err = pcall(function()
        state.proc:write(vim.json.encode(request) .. "\n")
    end)
    if not ok then
        state.request = false
        return nil, err or "Unknown error"
    end

    ok = vim.wait(config.timeout, function() return state.response ~= nil end, config.interval)
    if not ok then
        if state.proc then
            state.proc:kill('TERM')
            state.proc:wait(100)
        end
        state.request = false
        return nil, "Timeout waiting for response"
    end

    local response = state.response
    state.response = nil
    if not response.success then
        return nil, response.message
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

-- Get default clipipe.exe location
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

-- Query information from clipipe binary
local function query_bin(path)
    local res = vim.system({ path, '--query' }, { stdout = true, stderr = true }):wait()
    if res.code ~= 0 then
        local stderr = res.stderr
        return nil,
            "Process exited with code " ..
            res.code .. (stderr and (": " .. stderr:gsub('%s*$', '')) or "")
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
            "Binary version (" ..
            response.version .. ") doesn't match plugin (" .. ver .. ")"
    end
    return true
end

-- Attempt to build clipipe binary
local function build_bin(opts)
    if is_wsl then
        -- It's possible to cross-compile from within WSL or invoke a host Rust
        -- installation, but this is a niche scenario that's currently not
        -- supported since downloading the prebuilt binary suffices
        return nil
    end

    local cargo = is_win and "cargo.exe" or "cargo"

    if vim.fn.exepath(cargo) ~= "" then
        vim.notify("clipipe: Building binary with cargo...", vim.log.levels.INFO)
        local ready = false
        local proc = vim.system({ cargo, 'build', '--release' },
            { cwd = plugin_path, stderr = true },
            function() ready = true end
        )
        if opts.yield then
            while not ready do
                coroutine.yield("Waiting for build...")
            end
        end
        local res = proc:wait()
        if res.code ~= 0 then
            local stderr = res.stderr
            vim.notify(
                "clipipe: Build failed with code " ..
                res.code .. (stderr and (": " .. stderr:gsub('%s*$', '')) or ""),
                vim.log.levels.ERROR)
        else
            return cargo_release_bin
        end
    end
    return nil
end

-- Attempt to download prebuilt clipipe binary
local function download_bin(opts)
    local info = vim.uv.os_uname()
    local os = is_wsl and "windows_nt" or info.sysname:lower()
    local cpu = info.machine
    local suffix = (system_map[os] or {})[cpu]
    if not suffix then
        -- Prebuilt binary not available
        return nil
    end

    local url = github_rel_url:format(version(), suffix)
    local curl = is_win_native and "curl.exe" or "curl"
    local path = download_path()

    vim.fn.mkdir(vim.fn.fnamemodify(path, ':h'), "p")
    vim.notify("clipipe: Downloading binary...", vim.log.levels.INFO)
    local ready = false
    local proc = vim.system(
        { curl, '--no-progress-meter', '-f', '-L', '-o', path, url },
        { stderr = true },
        function() ready = true end
    )
    if opts.yield then
        while not ready do
            coroutine.yield("Waiting for download...")
        end
    end
    local res = proc:wait()
    if res.code ~= 0 then
        local stderr = res.stderr
        vim.notify(
            "clipipe: Failed to download" ..
            (stderr and (': ' .. stderr:gsub('%s*$', '')) or ''),
            vim.log.levels.ERROR)
        return nil
    end

    if not is_win then
        vim.uv.fs_chmod(path, 448)
    end

    return path
end

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
            notify("clipipe: waiting for startup", vim.log.levels.INFO)
        else
            notify("clipipe: copy failed: " .. (err or "Unknown error"), "ErrorMsg")
        end
    end
end

-- Paste function suitable for g:clipboard
function M.paste(source)
    local request = { action = "paste", clipboard = reg_to_clipboard[source] or source }
    local response, err = transact(request)
    if not response then
        if err == IN_PROGRESS then
            notify("clipipe: waiting for startup", vim.log.levels.INFO)
        else
            notify("clipipe: paste failed: " .. (err or "Unknown error"), "ErrorMsg")
        end
        return {}
    end
    return vim.split(response.data, "\n", { plain = true })
end

-- Plugin setup
function M.setup(user_config)
    config = vim.tbl_extend("force", defaults, user_config or {})

    if config.download or config.build then
        -- Run build now in case it hasn't happened already
        M.build(config)
    else
        -- Trust user path or search without building
        config.path = config.path or find_bin()
    end

    -- If setup is called more than one, terminate any background process
    if state.proc then
        state.proc:kill('TERM')
        state.proc = nil
    end

    if config.enable and config.path then
        M.enable()
    end
end

-- Download or build clipipe binary
function M.build(opts)
    opts = opts or {}
    -- Look for existing clipipe binary first
    local path = find_bin()
    if path then
        -- Verify it's usable
        local ok, err = verify_bin(path)
        if not ok then
            vim.notify("clipipe: ignoring " .. path .. ": " .. (err or "Unknown error"),
                vim.log.levels.INFO)
            path = nil
        end
    end
    if not path and opts.download then
        path = download_bin(opts)
    end
    if not path and opts.build then
        path = build_bin(opts)
    end
    config.path = path
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

    start()
end

return M
