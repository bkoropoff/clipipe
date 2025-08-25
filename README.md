# clipipe

**clipipe** is a Neovim clipboard provider that avoids starting a new process
on each operation, which can be slow on Windows or WSL.  Instead, it
communicates over pipes with a persistent background process.

## Features

- **Fast**
- **Cross-platform**:
    * Windows (native and WSL)
    * Linux (Wayland and X11)

## Requirements

- **Neovim**: Version 0.10+
- **Rust/Cargo**: Required for building the `clipipe` binary from source
  (optional).
- **curl**: Required for downloading the pre-built binary.

## Installation

### Plugin

Install this repository with your manager of choice.  For example, using
`lazy.nvim`:

```lua
require 'lazy'.setup {
  {
    'bkoropoff/clipipe.nvim',
    opts = {
      -- Optional configuration, defaults shown here:
      path = nil, -- clipipe binary
      keep_line_endings = false, -- Set to true to disable \r\n conversion on Windows
      enable = true, -- Automatically set g:clipboard to enable clipipe
      timeout = 1000, -- Timeout for responses from background process (ms)
      interval = 10, -- Polling interval for responses (ms)
      download = true, -- Download pre-built binary if needed
      build = true, -- Build from source if needed
    }
    end,
  },
}
```

### Clipboard register

If you haven't already, set the `clipboard` option to `unnamed` or
`unnamedplus` in your configuration, e.g.:

```lua
vim.o.clipboard = 'unnamedplus'
```

The distinction is only relevant for X11, where `unnamed` corresponds to the
"primary" (middle-click paste) and `unnamedplus` correponds to the "clipboard"
(Ctrl-V paste).

See the Neovim documentation for more details.

## Manual Setup

### `clipipe` Binary

The plugin attempts to locate the `clipipe` binary using the following steps:
1. **Path**: Looks for `clipipe` (or `clipipe.exe` on Windows/WSL) in your
   `PATH`, unless overridden by the `path` option to `setup`.  The binary is
   only used if it is runnable and the version matches the plugin.
2. **Download**: Downloads a binary from GitHub releases to
   `%LOCALAPPDATA%\clipipe` (Windows/WSL) or the plugin directory (if `download
   = true`)
3. **Build from source** (Not WSL): Tries to use `cargo` to build the bundled
   source code (if `build = true`)

If these fail, you will receive an error message during `setup`.

### Clipboard Provider

If `enable = false` in the options passed to `setup`, you can manually enable
the clipboard provider by calling `enable`, or use the `copy` and `paste`
functions directly, e.g.:

```lua
local clipipe = require 'clipipe'

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
```

## License

[MIT License](LICENSE)
