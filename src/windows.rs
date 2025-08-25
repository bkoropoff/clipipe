use std::env;
use std::thread;
use std::time::Duration;

use crate::clipboard::{self, Data, Dest, Result, Source};
use clipboard_win::{formats, get, set, Clipboard};

pub struct Backend {
    convert_line_endings: bool,
}

impl Backend {
    pub fn new() -> Result<Backend> {
        Ok(Backend {
            convert_line_endings: !env::args().any(|arg| arg == "--keep-line-endings"),
        })
    }

    fn clipboard() -> Result<Clipboard> {
        let mut tries: u32 = 10;
        let mut delay: u64 = 10;

        loop {
            break Ok(match Clipboard::new() {
                Ok(cb) => cb,
                Err(e) => {
                    tries = tries - 1;
                    if tries == 0 {
                        return Err(e.to_string().into());
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
            Err(e) if e.raw_code() == 6 || e.raw_code() == 1168 => "".into(),
            Err(e) => return Err(e.to_string().into()),
        })
    }

    fn set(data: &str) -> Result<()> {
        let _cb = Self::clipboard()?;
        Ok(set(formats::Unicode, data).map_err(|e| e.to_string())?)
    }
}

impl clipboard::Backend for Backend {
    fn copy(&mut self, _dest: Dest, data: &str) -> Result<()> {
        Ok((if self.convert_line_endings {
            let data = data.replace("\n", "\r\n");
            Self::set(&data)
        } else {
            Self::set(data)
        })
        .map_err(|e| e.to_string())?)
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
