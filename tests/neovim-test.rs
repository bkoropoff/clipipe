use std::process::Command;

const ROOT: &'static str = env!("CARGO_MANIFEST_DIR");
const HARNESS: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/harness.lua");
const CLIPIPE: &'static str = env!("CARGO_BIN_EXE_clipipe");

fn spawn(test: &str) {
    let mut cmd = Command::new("nvim");
    cmd.args(["--clean", "--headless", "-l", HARNESS, ROOT, CLIPIPE, test]);
    assert!(cmd.status().expect("Failed to start neovim").success());
}

macro_rules! test {
    ($name:ident) => {
        #[test]
        fn $name() {
            spawn(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/", stringify!($name), ".lua"));
        }
    };
    ($name:ident, $($rest:ident),+) => {
        test!($name);
        test!($rest);+
    }
}

test!(basic);
