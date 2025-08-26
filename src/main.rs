use serde_json::{Map, Value};
use std::env;
use std::error::Error;
use std::io::{self, BufRead, Write};

mod clipboard;

use clipboard::{Backend, Data, Dest, Source};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as backend;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

// FIXME: maybe use a specialized error type for some of this file
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// Clipboard action representation
enum Action<'a> {
    Copy(Dest, &'a str),
    Paste(Source),
    Query,
}

// Parsing from JSON
impl<'a> Action<'a> {
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

    pub fn parse(doc: &'a Map<String, Value>) -> Result<Action<'a>> {
        Ok(match doc.get("action") {
            None => return Err("No action specified".into()),
            Some(&Value::String(ref name)) => match name.as_ref() {
                "copy" => Action::Copy(
                    Self::dest(doc.get("clipboard"))?,
                    Self::data(doc.get("data"))?,
                ),
                "paste" => Action::Paste(Self::source(doc.get("clipboard"))?),
                "query" => Action::Query,
                name => return Err(format!("Invalid action: {}", name).into()),
            },
            Some(value) => return Err(format!("Expected string for action: {}", value).into()),
        })
    }
}

struct Clipipe {
    backend: backend::Backend,
}

impl Clipipe {
    // Query version number
    fn query() -> Map<String, Value> {
        let mut res = Map::new();
        res.insert("version".into(), VERSION.into());
        res
    }

    // Process request object, return response object
    fn request(&mut self, obj: Map<String, Value>) -> Result<Map<String, Value>> {
        Ok(match Action::parse(&obj)? {
            Action::Query => Self::query(),
            Action::Copy(dest, data) => {
                self.backend.copy(dest, data)?;
                Map::new()
            }
            Action::Paste(source) => {
                let Data { data, mime } = self.backend.paste(source)?;
                let mut res = Map::new();
                res.insert("data".into(), data.into());
                if let Some(mime) = mime {
                    res.insert("mime".into(), mime.into());
                }
                res
            }
        })
    }

    fn new() -> Result<Clipipe> {
        Ok(Clipipe {
            backend: backend::Backend::new()?,
        })
    }
}

// Convert error to JSON, capturing source chain
fn error_to_json<E: Error + ?Sized>(error: &E, map: &mut Map<String, Value>) {
    map.insert("message".into(), error.to_string().into());
    if let Some(source) = error.source() {
        let mut sub = Map::new();
        error_to_json(&source, &mut sub);
        map.insert("source".into(), sub.into());
    }
}

fn run() -> Result<()> {
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    // Quick query path, used to decide if binary is right version
    if env::args().any(|arg| arg == "--query") {
        writeln!(stdout, "{}", Value::Object(Clipipe::query()))?;
        return Ok(());
    }

    let mut clipipe = Clipipe::new()?;

    for line in stdin.lines() {
        let res: Value = match serde_json::from_str(line?.as_ref())
            .map_err(|e| e.into())
            .and_then(|obj| clipipe.request(obj))
        {
            Ok(mut res) => {
                // Add success/error discriminator
                res.insert("success".into(), true.into());
                res.into()
            }
            Err(e) => {
                let mut res = Map::new();
                res.insert("success".into(), false.into());
                error_to_json(&*e, &mut res);
                res.into()
            }
        };

        writeln!(stdout, "{}", res)?;
        stdout.flush()?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    run().or_else(|err| writeln!(io::stderr(), "Error: {}", err))
}
