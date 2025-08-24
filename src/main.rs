use serde_json::{json, Map, Value};
use std::env;
use std::io::{self, BufRead, Write};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

enum Source {
    Default,
    Primary,
    Clipboard,
}

enum Dest {
    Default,
    Primary,
    Clipboard,
    Both,
}

struct Data {
    data: String,
    mime: Option<String>,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

trait Backend {
    fn copy(&mut self, dest: Dest, data: &str) -> Result<()>;
    fn paste(&mut self, src: Source) -> Result<Data>;
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{Data, Dest, Result, Source};
    use clipboard_win::{formats, get_clipboard, set_clipboard};

    pub struct Backend {
        convert_line_endings: bool,
    }

    impl Backend {
        pub fn new(convert_line_endings: bool) -> Backend {
            Backend {
                convert_line_endings,
            }
        }
    }

    impl super::Backend for Backend {
        fn copy(&mut self, _dest: Dest, data: &str) -> Result<()> {
            Ok((if self.convert_line_endings {
                let data = data.replace("\n", "\r\n");
                set_clipboard(formats::Unicode, &data)
            } else {
                set_clipboard(formats::Unicode, data)
            })
            .map_err(|e| e.to_string())?)
        }

        fn paste(&mut self, _src: Source) -> Result<Data> {
            let mut data: String = match get_clipboard(formats::Unicode) {
                Ok(data) => data,
                Err(e) if e.raw_code() == 6 || e.raw_code() == 1168 => "".into(),
                Err(e) => return Err(e.to_string().into()),
            };
            if self.convert_line_endings {
                data = data.replace("\r\n", "\n");
            }
            Ok(Data {
                data: data,
                mime: None,
            })
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Data, Dest, Result, Source};

    use std::env;
    use std::io::Read;
    use std::time::Duration;

    use wl_clipboard_rs::{
        copy::{
            ClipboardType as CopyClipboardType, MimeType as CopyMimeType, Options,
            Source as CopySource,
        },
        paste::{
            get_contents, ClipboardType as PasteClipboardType, Error as PasteError,
            MimeType as PasteMimeType, Seat,
        },
        utils::is_primary_selection_supported,
    };

    use x11_clipboard::{Atom, Clipboard as X11Clipboard};

    pub struct WaylandBackend {
        primary_supported: bool,
    }

    impl WaylandBackend {
        pub fn new() -> WaylandBackend {
            WaylandBackend {
                primary_supported: is_primary_selection_supported().is_ok(),
            }
        }

        fn copy_type(&self, dest: Dest) -> CopyClipboardType {
            match (dest, self.primary_supported) {
                // The regular clipboard seems to be "default" on Wayland (e.g. wl-paste)
                (Dest::Default | Dest::Clipboard, _) => CopyClipboardType::Regular,
                (Dest::Primary, true) => CopyClipboardType::Primary,
                (Dest::Both, true) => CopyClipboardType::Both,
                // Silently fall back to regular clipboard for compatibility
                (Dest::Primary | Dest::Both, false) => CopyClipboardType::Regular,
            }
        }

        fn paste_type(&self, source: Source) -> PasteClipboardType {
            match (source, self.primary_supported) {
                (Source::Default | Source::Clipboard, _) => PasteClipboardType::Regular,
                (Source::Primary, true) => PasteClipboardType::Primary,
                // Silently fall back to regular clipboard for compatibility
                (Source::Primary, false) => PasteClipboardType::Regular,
            }
        }
    }

    pub struct X11Backend {
        backend: X11Clipboard,
        both: [Atom; 2],
    }

    impl X11Backend {
        pub fn new() -> Result<X11Backend> {
            let backend = X11Clipboard::new()?;
            let primary = backend.setter.atoms.primary;
            let clipboard = backend.setter.atoms.clipboard;

            Ok(X11Backend {
                backend: backend,
                both: [primary, clipboard],
            })
        }

        fn source_atom(&self, source: Source) -> Atom {
            match source {
                Source::Default | Source::Primary => self.backend.setter.atoms.primary,
                Source::Clipboard => self.backend.setter.atoms.clipboard,
            }
        }

        fn dest_atoms(&self, dest: Dest) -> &[Atom] {
            match dest {
                Dest::Default | Dest::Primary => {
                    std::slice::from_ref(&self.backend.setter.atoms.primary)
                }
                Dest::Clipboard => std::slice::from_ref(&self.backend.setter.atoms.clipboard),
                Dest::Both => &self.both,
            }
        }
    }

    impl super::Backend for X11Backend {
        fn copy(&mut self, dest: Dest, data: &str) -> Result<()> {
            for atom in self.dest_atoms(dest) {
                self.backend.store(
                    *atom,
                    self.backend.setter.atoms.utf8_string,
                    data.as_bytes(),
                )?;
            }
            Ok(())
        }

        fn paste(&mut self, source: Source) -> Result<Data> {
            let contents = self.backend.load(
                self.source_atom(source),
                self.backend.setter.atoms.utf8_string,
                self.backend.setter.atoms.property,
                Duration::from_millis(100),
            )?;
            Ok(Data {
                data: String::from_utf8(contents)?,
                mime: None,
            })
        }
    }

    impl super::Backend for WaylandBackend {
        fn copy(&mut self, dest: Dest, data: &str) -> Result<()> {
            let mut opts = Options::new();
            opts.clipboard(self.copy_type(dest));
            opts.copy(
                CopySource::Bytes(data.as_bytes().into()),
                CopyMimeType::Text,
            )?;
            Ok(())
        }

        fn paste(&mut self, src: Source) -> Result<Data> {
            Ok(
                match get_contents(
                    self.paste_type(src),
                    Seat::Unspecified,
                    // FIXME: this is not flexible enough, need to inspect offer types manually
                    PasteMimeType::TextWithPriority("text/plain"),
                ) {
                    Ok((mut pipe, mime)) => {
                        let mut contents = vec![];
                        pipe.read_to_end(&mut contents)?;

                        let mime = if mime.starts_with("text/_moz") {
                            // HACK: ignore weird internal types from Firefox
                            contents.clear();
                            None
                        } else {
                            Some(mime)
                        };

                        Data {
                            data: String::from_utf8(contents)?,
                            mime: mime,
                        }
                    }
                    Err(
                        PasteError::ClipboardEmpty | PasteError::NoSeats | PasteError::NoMimeType,
                    ) => Data {
                        data: "".into(),
                        mime: None,
                    },
                    Err(err) => return Err(err.into()),
                },
            )
        }
    }

    pub enum Backend {
        Wayland(WaylandBackend),
        X11(X11Backend),
    }

    impl super::Backend for Backend {
        fn copy(&mut self, dest: Dest, data: &str) -> Result<()> {
            match *self {
                Backend::Wayland(ref mut wl) => wl.copy(dest, data),
                Backend::X11(ref mut x11) => x11.copy(dest, data),
            }
        }

        fn paste(&mut self, src: Source) -> Result<Data> {
            match *self {
                Backend::Wayland(ref mut wl) => wl.paste(src),
                Backend::X11(ref mut x11) => x11.paste(src),
            }
        }
    }

    fn have_env_var(var: &str) -> bool {
        match env::var(var) {
            Ok(v) => v.len() != 0,
            _ => false,
        }
    }

    impl Backend {
        pub fn new() -> Result<Backend> {
            Ok(if have_env_var("WAYLAND_DISPLAY") {
                Backend::Wayland(WaylandBackend::new())
            } else if have_env_var("DISPLAY") {
                Backend::X11(X11Backend::new()?)
            } else {
                return Err("No display server available".into());
            })
        }
    }
}

enum ClipboardAction<'a> {
    Copy(Dest, &'a str),
    Paste(Source),
    Query,
}

impl<'a> ClipboardAction<'a> {
    fn source(name: Option<&Value>) -> Result<Source> {
        Ok(match name {
            None => Source::Default,
            Some(&Value::String(ref name)) => match name.as_ref() {
                "default" => Source::Default,
                "clipboard" => Source::Clipboard,
                "primary" => Source::Primary,
                _ => return Err(format!("Invalid clipboard source: {}", name).into()),
            },
            Some(value) => return Err(format!("Invalid clipboard source: {}", value).into()),
        })
    }

    fn dest(name: Option<&Value>) -> Result<Dest> {
        Ok(match name {
            None => Dest::Default,
            Some(&Value::String(ref name)) => match name.as_ref() {
                "default" => Dest::Default,
                "clipboard" => Dest::Clipboard,
                "primary" => Dest::Primary,
                "both" => Dest::Both,
                _ => return Err(format!("Invalid clipboard destination: {}", name).into()),
            },
            Some(value) => return Err(format!("Invalid clipboard destination: {}", value).into()),
        })
    }

    fn data<'b>(data: Option<&'b Value>) -> Result<&'b str> {
        Ok(match data {
            None => return Err("Request is missing `data`".into()),
            Some(&Value::String(ref data)) => data.as_ref(),
            Some(value) => return Err(format!("Invalid clipboard data: {}", value).into()),
        })
    }

    pub fn parse(doc: &'a Map<String, Value>) -> Result<ClipboardAction<'a>> {
        Ok(match doc.get("action") {
            None => return Err("No action specified".into()),
            Some(&Value::String(ref name)) => match name.as_ref() {
                "copy" => ClipboardAction::Copy(
                    Self::dest(doc.get("clipboard"))?,
                    Self::data(doc.get("data"))?,
                ),
                "paste" => ClipboardAction::Paste(Self::source(doc.get("clipboard"))?),
                "query" => ClipboardAction::Query,
                name => return Err(format!("Invalid action: {}", name).into()),
            },
            Some(value) => return Err(format!("Expected string for action: {}", value).into()),
        })
    }
}

fn query() -> Map<String, Value> {
    let mut res = Map::new();
    res.insert("version".into(), VERSION.into());
    res
}

struct Clipboard<B> {
    backend: B,
}

impl<B: Backend> Clipboard<B> {
    fn process_request(&mut self, line: &str) -> Result<Map<String, Value>> {
        let obj: Map<String, Value> = serde_json::from_str(line)?;

        Ok(match ClipboardAction::parse(&obj)? {
            ClipboardAction::Query => query(),
            ClipboardAction::Copy(dest, data) => {
                self.backend.copy(dest, data)?;
                Map::new()
            }
            ClipboardAction::Paste(source) => {
                let Data { data, mime } = self.backend.paste(source)?;
                let mut res = Map::new();
                res.insert("success".into(), Value::Bool(true));
                res.insert("data".into(), Value::String(data));
                if let Some(mime) = mime {
                    res.insert("mime".into(), Value::String(mime));
                }
                res
            }
        })
    }
}

fn new_clipboard() -> Result<Clipboard<impl Backend>> {
    Ok(Clipboard {
        #[cfg(target_os = "linux")]
        backend: linux::Backend::new()?,
        #[cfg(target_os = "windows")]
        backend: windows::Backend::new(!env::args().any(|arg| arg == "--keep-line-endings")),
    })
}

fn run() -> Result<()> {
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    if env::args().any(|arg| arg == "--query") {
        writeln!(stdout, "{}", Value::Object(query()))?;
        return Ok(());
    }

    let mut clipboard = new_clipboard()?;

    for line in stdin.lines() {
        let line = line?;
        let res = match clipboard.process_request(&line) {
            Ok(mut res) => {
                res.insert("success".into(), Value::Bool(true));
                Value::Object(res)
            }
            Err(e) => json!({
                "success": false,
                "message": e.to_string()
            }),
        };

        writeln!(stdout, "{}", res)?;
        stdout.flush()?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    run().or_else(|err| writeln!(io::stderr(), "Error: {}", err))
}
