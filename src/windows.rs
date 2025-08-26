use std::env;
use std::thread;
use std::time::Duration;

use crate::clipboard::{self, Data, Dest, Error, ErrorDetail, Result, Source};
use clipboard_win::{formats, get, set, Clipboard, ErrorCode};

// ErrorCode doesn't implement std::error::Error for some reason, so wrap it
#[derive(Debug)]
struct ErrorCodeError(ErrorCode);

impl std::fmt::Display for ErrorCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        self.0.fmt(f)
    }
}

impl std::error::Error for ErrorCodeError {}

impl From<ErrorCode> for Error {
    fn from(value: ErrorCode) -> Error {
        Error::new_with_source(ErrorDetail::System, ErrorCodeError(value))
    }
}

pub struct Backend {
    convert_line_endings: bool,
}

impl Backend {
    pub fn new() -> Result<Backend> {
        Ok(Backend {
            // FIXME: not the best way to plumb this setting through
            convert_line_endings: !env::args().any(|arg| arg == "--keep-line-endings"),
        })
    }

    // Get clipboard lock guard.
    //
    // The Windows clipboard has to be globally locked to be accessed, with contention resulting in
    // errors.  Retry with backoff.
    fn clipboard() -> Result<Clipboard> {
        let mut tries: u32 = 10;
        let mut delay: u64 = 10;

        loop {
            break Ok(match Clipboard::new() {
                Ok(cb) => cb,
                Err(e) => {
                    tries = tries - 1;
                    if tries == 0 {
                        return Err(e.into());
                    }
                    thread::sleep(Duration::from_millis(delay));
                    delay = (delay * 2).min(500);
                    continue;
                }
            });
        }
    }

    fn get() -> Result<String> {
        let _cb = Self::clipboard()?;
        Ok(match get(formats::Unicode) {
            Ok(data) => data,
            // FIXME: magic constants
            Err(e) if e.raw_code() == 6 || e.raw_code() == 1168 => "".into(),
            Err(e) => return Err(e.into()),
        })
    }

    fn set(data: &str) -> Result<()> {
        let _cb = Self::clipboard()?;
        Ok(set(formats::Unicode, data)?)
    }
}

impl clipboard::Backend for Backend {
    fn copy(&mut self, _dest: Dest, data: &str) -> Result<()> {
        Ok((if self.convert_line_endings {
            let data = data.replace("\n", "\r\n");
            Self::set(&data)
        } else {
            Self::set(data)
        })?)
    }

    fn paste(&mut self, _src: Source) -> Result<Data> {
        let mut data = Self::get()?;
        if self.convert_line_endings {
            data = data.replace("\r\n", "\n");
        }
        Ok(Data {
            data: data,
            mime: None,
        })
    }
}
