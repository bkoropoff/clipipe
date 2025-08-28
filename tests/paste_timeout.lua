
mock {
    handlers = {
        query = {
            response = {
                success = true,
                version = vim.env.CARGO_PKG_VERSION,
            }
        },
        paste = {
            response = {
                success = true,
                data = "foobar",
            },
            delay = 1000
        }
    }
}

require 'clipipe'.paste("*")
sleep(150)
expect_error("paste failed", { message = "timed out waiting for response" })
