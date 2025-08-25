use std::env;

use crate::clipboard::{self, Data, Dest, Result, Source};
use clipboard_win::{formats, get_clipboard, set_clipboard};

pub struct Backend {
    convert_line_endings: bool,
}

impl Backend {
    pub fn new() -> Result<Backend> {
        Ok(Backend {
            convert_line_endings: !env::args().any(|arg| arg == "--keep-line-endings"),
        })
    }
}

impl clipboard::Backend for Backend {
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
