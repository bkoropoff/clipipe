mock {
    handlers = {
        query = {
            response = {
                success = true,
                version = vim.env.CARGO_PKG_VERSION,
            },
            delay = 1000
        }
    }
}

require 'clipipe'.enable()
sleep(150)
expect_error("timed out on start")
