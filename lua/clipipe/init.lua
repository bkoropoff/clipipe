local M = {}

-- Format string for Windows clipipe executable download
local github_win_url =
    'https://github.com/bkoropoff/clipipe/releases/download/v%s/clipipe.exe'
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

-- State for the background job
local state = {}

-- Default configuration
local defaults = {
    -- Find clipipe in PATH
    path = vim.fn.exepath(is_win and "clipipe.exe" or "clipipe"),
    -- Convert line endings on Windows
    keep_line_endings = false,
    -- Enable on setup
    enable = true,
    -- Timeout waiting for response (ms)
    timeout = 1000,
    -- Interval to poll for response (ms)
    interval = 10,
    -- Build clipipe binary from source, if necessary and possible
    build = true,
    -- Download clipipe binary, if necessary and possible
    download = true
}

local config = defaults

-- Start background process if not already running
local function start()
    if state.proc then
        return true
    end

    if not config.path or config.path == "" then
        return false, "Binary not found"
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
                    vim.notify("clipipe: stdout error: " .. err, vim.log.levels.ERROR)
                    return
                end
                if data then
                    local idx = string.find(data, "\n", 1, true)
                    if idx then
                        local pre = string.sub(data, 1, idx)
                        data = string.sub(data, idx + 1)
                        table.insert(state.buffer, pre)
                        local stdout = table.concat(state.buffer)
                        local parsed = vim.json.decode(stdout)
                        state.buffer = { data }
                        state.response = parsed
                        state.request = false
                    else
                        table.insert(state.buffer, data)
                    end
                end
            end,
        },
        function(obj)
            if obj.code ~= 0 then
                local stderr = obj.stderr
                vim.notify(
                    "clipipe: Process exited with code " ..
                    obj.code .. (stderr and (": " .. stderr:gsub('%s*$', '')) or ""),
                    vim.log.levels.ERROR)
            end
            state.proc = nil
        end)

    if not ok then
        return false, "Failed to start clipipe process: " .. (proc or "unknown error")
    end

    state = {
        proc = proc,
        buffer = {},
        request = false,
        response = nil
    }

    return true
end

-- Send a request, get the response
local function transact(request)
    local ok, err = start()
    if not ok then
        return nil, err
    end

    if state.request then
        return nil, "Overlapped request attempted"
    end
    state.request = true

    ok, err = pcall(function()
        state.proc:write(vim.json.encode(request) .. "\n")
    end)
    if not ok then
        return nil, err or "Unknown error"
    end

    ok = vim.wait(config.timeout, function() return state.response end, config.interval)
    if not ok then
        local proc = state.proc
        state.proc = nil
        if proc then
            proc:kill('TERM')
        end
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
local function win_clipipe_bin()
    return win_localappdata() .. '/clipipe/clipipe.exe'
end

-- Find clipipe binary
local function find_bin()
    if config.path and config.path ~= '' and vim.uv.fs_stat(config.path) then
        return config.path
    elseif not is_wsl and vim.uv.fs_stat(cargo_release_bin) then
        return cargo_release_bin
    elseif is_win then
        local path = win_clipipe_bin()
        if vim.uv.fs_stat(path) then
            return path
        end
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
local function build_bin()
    if is_wsl then
        -- It's possible to cross-compile from within WSL or invoke a host Rust
        -- installation, but this is a niche scenario that's currently not
        -- supported since downloading the prebuilt binary suffices
        return nil
    end

    local cargo = is_win and "cargo.exe" or "cargo"

    if vim.fn.exepath(cargo) ~= "" then
        vim.notify("clipipe: Building binary with cargo...", vim.log.levels.INFO)
        local proc = vim.system({ cargo, 'build', '--release' },
            { cwd = plugin_path, stderr = true })
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
local function download_bin()
    if not is_win then
        -- Prebuilt binaries are only available for Windows
        return nil
    end

    local url = github_win_url:format(version())
    local curl = is_wsl and "curl" or "curl.exe"
    local path = win_clipipe_bin()

    vim.fn.mkdir(vim.fn.fnamemodify(path, ':h'), "p")
    vim.notify("clipipe: Downloading binary...", vim.log.levels.INFO)
    local res = vim.system(
        { curl, '--no-progress-meter', '-f', '-L', '-o', path, url },
        { stderr = true }):wait()
    if res.code ~= 0 then
        local stderr = res.stderr
        vim.notify(
            "clipipe: Failed to download" ..
            (stderr and (': ' .. stderr:gsub('%s*$', '')) or ''),
            vim.log.levels.ERROR)
        return nil
    end

    return path
end

local reg_to_clipboard = {
    ['+'] = "clipboard",
    ['*'] = "default"
}

-- Copy function suitable for g:clipboard
function M.copy(lines, reg)
    local data = table.concat(lines, "\n")
    local request = { action = "copy", data = data, clipboard = reg_to_clipboard[reg] }
    local response, err = transact(request)
    if not response then
        -- An actual error notification ({err = true}, or with vim.notify) will
        -- cause the clipboard provider to return bad data, so just use the
        -- ErrorMsg highlight instead
        vim.api.nvim_echo(
            { { "clipipe: copy failed: " .. (err or "Unknown error"), "ErrorMsg" } }, true,
            {})
    end
end

-- Paste function suitable for g:clipboard
function M.paste(reg)
    local request = { action = "paste", clipboard = reg_to_clipboard[reg] }
    local response, err = transact(request)
    if not response then
        vim.api.nvim_echo(
            { { "clipipe: paste failed: " .. (err or "Unknown error"), "ErrorMsg" } },
            true,
            {})
        return {}
    end
    return vim.split(response.data, "\n", { plain = true })
end

-- Plugin setup
function M.setup(user_config)
    config = vim.tbl_extend("force", defaults, user_config or {})

    -- Locate existing clipipe binary
    local path = find_bin()
    if path then
        -- Verify it's usable
        local ok, err = verify_bin(path)
        if not ok then
            vim.notify("clipipe: ignoring " .. path .. ": " .. (err or "Unknown error"),
                vim.log.levels.WARN)
            path = nil
        end
    end
    if not path then
        -- Try to obtain a binary
        if config.download then
            path = download_bin()
        end
        if not path and config.build then
            path = build_bin()
        end
    end
    config.path = path

    if not config.path then
        vim.notify("clipipe: Couldn't find valid clipipe binary", vim.log.levels.ERROR)
    end

    -- If setup is called more than one, terminate any background process
    if state.proc then
        state.proc:kill('TERM')
        state.proc = nil
    end

    -- Enable clipipe if possible and requested
    if config.path and config.enable then
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
end

return M
