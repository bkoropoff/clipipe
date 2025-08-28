use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, Write};
use std::thread::sleep;
use std::time::Duration;

#[derive(Deserialize)]
struct Handler {
    response: Map<String, Value>,
    #[serde(default)]
    delay: u64,
}

#[derive(Deserialize)]
struct Mock {
    handlers: HashMap<String, Handler>,
}

impl Mock {
    fn request(&self, message: Map<String, Value>) -> Map<String, Value> {
        let action = message
            .get("action")
            .and_then(|v| v.as_str())
            .expect("No action in message");
        let handler = self.handlers.get(action).expect("No handler for action");
        let duration = Duration::from_millis(handler.delay);
        sleep(duration);
        handler.response.clone()
    }
}

pub fn main() {
    let mock: Mock = serde_json::from_str(
        env::var("CLIPIPE_MOCK")
            .expect("CLIPIPE_MOCK not set")
            .as_ref(),
    ).expect("Malformed mock specification");

    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    if env::args().any(|arg| arg == "--query") {
        let mut message = Map::new();
        message.insert("action".into(), "query".into());
        writeln!(stdout, "{}", Value::Object(mock.request(message))).expect("Couldn't write to stdout");
        return;
    }

    for line in stdin.lines() {
        let message: Map<String, Value> =
            serde_json::from_str(line.expect("Could not read stdin").as_ref())
                .expect("Invalid request JSON");
        let response = mock.request(message);
        writeln!(stdout, "{}", Value::Object(response)).expect("Couldn't write to stdout");
        stdout.flush().expect("Couldn't flush stdout");
    }
}
