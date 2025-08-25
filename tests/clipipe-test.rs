use serde_json::{json, Value};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};

fn clipipe_bin() -> PathBuf {
    let test_bin = std::env::current_exe().expect("Couldn't find test binary path");
    let test_dir = test_bin
        .parent()
        .unwrap_or_else(|| panic!("Couldn't find test bin directory"));
    let profile_dir = test_dir
        .parent()
        .unwrap_or_else(|| panic!("Couldn't find profile directory"));
    profile_dir.join(if cfg!(target_os = "windows") {
        "clipipe.exe"
    } else {
        "clipipe"
    })
}

struct Clipipe<I, O> {
    child: Child,
    input: I,
    output: O,
    _guard: MutexGuard<'static, ()>,
}

impl<I, O> Drop for Clipipe<I, O> {
    fn drop(&mut self) {
        self.child.kill().expect("Couldn't terminate clipipe")
    }
}

impl<I: BufRead, O: Write> Clipipe<I, O> {
    fn request(&mut self, message: Value) -> Value {
        writeln!(self.output, "{}", message).expect("Couldn't write to output");
        self.output.flush().expect("Couldn't flush output");
        let mut buffer = String::new();
        self.input
            .read_line(&mut buffer)
            .expect("Couldn't read input");
        Value::from_str(&buffer).expect("Invalid JSON")
    }
}

#[derive(Debug, Clone)]
enum DisplayServer {
    #[cfg(target_os = "linux")]
    Wayland,
    #[cfg(target_os = "linux")]
    X11,
    #[cfg(target_os = "windows")]
    Windows,
}

impl Copy for DisplayServer {}

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn spawn(server: DisplayServer) -> Clipipe<impl BufRead, impl Write> {
    let mut cmd = Command::new(clipipe_bin());
    cmd.stdout(Stdio::piped()).stdin(Stdio::piped());
    match server {
        #[cfg(target_os = "linux")]
        DisplayServer::Wayland => {
            cmd.env_remove("DISPLAY");
        }
        #[cfg(target_os = "linux")]
        DisplayServer::X11 => {
            cmd.env_remove("WAYLAND_DISPLAY");
        }
        #[cfg(target_os = "windows")]
        DisplayServer::Windows => (),
    };

    let mut child = cmd.spawn().expect("Couldn't run clipipe");
    let input = BufReader::new(child.stdout.take().unwrap());
    let output = BufWriter::new(child.stdin.take().unwrap());
    Clipipe {
        child,
        input,
        output,
        _guard: TEST_MUTEX.lock().unwrap(),
    }
}

mod tests {
    use super::*;
    use rstest::rstest;
    use rstest_reuse::{self, *};

    #[cfg(target_os = "linux")]
    mod template {
        use super::*;
        #[template]
        #[rstest]
        #[case::wayland(DisplayServer::Wayland)]
        #[case::x11(DisplayServer::X11)]
        fn display(#[case] _server: DisplayServer) {}
    }

    #[cfg(target_os = "windows")]
    mod template {
        use super::*;
        #[template]
        #[rstest]
        #[case::windows(DisplayServer::Windows)]
        fn display(#[case] _server: DisplayServer) {}
    }

    #[apply(template::display)]
    fn copy_paste(#[case] server: DisplayServer) {
        let mut clipipe = spawn(server);
        let data = format!("{:?}", server);
        assert_eq!(
            clipipe.request(json!({"action": "copy", "data": data})),
            json!({"success": true})
        );

        let response = clipipe.request(json!({"action": "paste"}));
        println!("{}", response);
        assert_eq!(response["success"], Value::Bool(true));
        assert_eq!(response["data"], Value::String(data));
        if let Some(mime) = response.get("mime") {
            assert_eq!(mime, "text/plain")
        }
    }
}
