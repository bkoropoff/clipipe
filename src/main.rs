use serde_json::{json, Value};
use std::env;
use std::io::{self, BufRead, Write};

#[cfg(target_os = "linux")]
use std::io::{Error, ErrorKind};

#[cfg(target_os = "windows")]
use clipboard_win::{formats, get_clipboard, set_clipboard};
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use wl_clipboard_rs::{
    copy::{MimeType as CopyMimeType, Options, Source},
    paste::{get_contents, ClipboardType, Error as PasteError, MimeType as PasteMimeType, Seat},
};
#[cfg(target_os = "linux")]
use x11_clipboard::{Atom, Clipboard as X11Clipboard};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

enum Clipboard {
    #[cfg(target_os = "linux")]
    Wayland,
    #[cfg(target_os = "linux")]
    X11(X11Clipboard),
    #[cfg(target_os = "windows")]
    Windows(bool),
}

impl Clipboard {
    #[cfg(target_os = "linux")]
    fn clipboard_atom(cb: &X11Clipboard, name: Option<&str>) -> Result<Atom, String> {
        Ok(match name {
            None | Some("default") => cb.setter.atoms.primary,
            Some("clipboard") => cb.setter.atoms.clipboard,
            Some(name) => return Err(format!("No such X11 clipboard: {}", name)),
        })
    }

    fn process_request(&mut self, line: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let json: Value = serde_json::from_str(line)?;

        match json.get("action").and_then(|v| v.as_str()) {
            Some("query") => Ok(query()),
            Some("copy") => {
                let data = json
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing or invalid data field")?;

                match *self {
                    #[cfg(target_os = "windows")]
                    Clipboard::Windows(convert) => {
                        (if convert {
                            let data = data.replace("\n", "\r\n");
                            set_clipboard(formats::Unicode, &data)
                        } else {
                            set_clipboard(formats::Unicode, data)
                        })
                        .map_err(|e| e.to_string())?;
                    }
                    #[cfg(target_os = "linux")]
                    Clipboard::Wayland => {
                        Options::new()
                            .copy(Source::Bytes(data.as_bytes().into()), CopyMimeType::Text)?;
                    }
                    #[cfg(target_os = "linux")]
                    Clipboard::X11(ref mut cb) => {
                        let atom = Clipboard::clipboard_atom(
                            cb,
                            json.get("clipboard").and_then(|v| v.as_str()),
                        )?;
                        cb.store(atom, cb.setter.atoms.utf8_string, data.as_bytes())?;
                    }
                }

                Ok(json!({"success": true}))
            }
            Some("paste") => {
                let data = match *self {
                    #[cfg(target_os = "windows")]
                    Clipboard::Windows(convert) => {
                        let data: String = match get_clipboard(formats::Unicode) {
                            Ok(data) => data,
                            Err(e) if e.raw_code() == 6 || e.raw_code() == 1168 => "".into(),
                            Err(e) => return Err(e.to_string().into()),
                        };
                        if convert {
                            data.replace("\r\n", "\n")
                        } else {
                            data
                        }
                    }
                    #[cfg(target_os = "linux")]
                    Clipboard::Wayland => match get_contents(
                        ClipboardType::Regular,
                        Seat::Unspecified,
                        PasteMimeType::Text,
                    ) {
                        Ok((mut pipe, _)) => {
                            let mut contents = vec![];
                            pipe.read_to_end(&mut contents)?;
                            String::from_utf8(contents)?
                        }
                        Err(
                            PasteError::ClipboardEmpty
                            | PasteError::NoSeats
                            | PasteError::NoMimeType,
                        ) => "".into(),
                        Err(err) => return Err(err.into()),
                    },
                    #[cfg(target_os = "linux")]
                    Clipboard::X11(ref mut cb) => {
                        let atom = Clipboard::clipboard_atom(
                            cb,
                            json.get("clipboard").and_then(|v| v.as_str()),
                        )?;
                        let contents = cb.load(
                            atom,
                            cb.setter.atoms.utf8_string,
                            cb.setter.atoms.property,
                            Duration::from_millis(100),
                        )?;
                        String::from_utf8(contents)?
                    }
                };

                Ok(json!({
                    "success": true,
                    "data": data
                }))
            }
            _ => Err("Invalid or missing action".into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn have_env_var(var: &str) -> bool {
    match env::var(var) {
        Ok(v) => v.len() != 0,
        _ => false,
    }
}

fn main() -> io::Result<()> {
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    if env::args().any(|arg| arg == "--query") {
        writeln!(stdout, "{}", query())?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    let mut clipboard = Clipboard::Windows(!env::args().any(|arg| arg == "--keep-line-endings"));
    #[cfg(target_os = "linux")]
    let mut clipboard = if have_env_var("WAYLAND_DISPLAY") {
        Clipboard::Wayland
    } else if have_env_var("DISPLAY") {
        Clipboard::X11(X11Clipboard::new().map_err(|e| Error::new(ErrorKind::Other, e))?)
    } else {
        return Err(Error::new(ErrorKind::Other, "No display server available"));
    };

    for line in stdin.lines() {
        let line = line?;
        let response = match clipboard.process_request(&line) {
            Ok(response) => response,
            Err(e) => json!({
                "success": false,
                "message": e.to_string()
            }),
        };

        writeln!(stdout, "{}", response)?;
        stdout.flush()?;
    }

    Ok(())
}

fn query() -> Value {
    json!({"success": true, "version": VERSION})
}
