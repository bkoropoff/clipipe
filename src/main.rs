use serde_json::{json, Map, Value};
use std::env;
use std::io::{self, BufRead, Write};

mod clipboard;

use clipboard::{Backend, Data, Dest, Result, Source};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as backend;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

enum Action<'a> {
    Copy(Dest, &'a str),
    Paste(Source),
    Query,
}

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
    fn query() -> Map<String, Value> {
        let mut res = Map::new();
        res.insert("version".into(), VERSION.into());
        res
    }

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
                res.insert("success".into(), Value::Bool(true));
                res.insert("data".into(), Value::String(data));
                if let Some(mime) = mime {
                    res.insert("mime".into(), Value::String(mime));
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

fn run() -> Result<()> {
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    if env::args().any(|arg| arg == "--query") {
        writeln!(stdout, "{}", Value::Object(Clipipe::query()))?;
        return Ok(());
    }

    let mut clipipe = Clipipe::new()?;

    for line in stdin.lines() {
        let res = match serde_json::from_str(line?.as_ref())
            .map_err(|e| e.into())
            .and_then(|obj| clipipe.request(obj))
        {
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
