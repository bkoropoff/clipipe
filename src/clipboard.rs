// Source of a paste
pub enum Source {
    Default,
    Primary,
    Clipboard,
}

// Destination of a copy
pub enum Dest {
    Default,
    Primary,
    Clipboard,
    Both,
}

// Data returned from a paste
pub struct Data {
    pub data: String,
    // Mime type, if known.  Strictly advisory, only text is supported.
    pub mime: Option<String>,
}

// Information about an error
#[derive(Debug)]
pub enum ErrorDetail {
    // No display server to connect to
    #[cfg(target_os = "linux")]
    NoDisplayServer,
    // Invalid UTF-8 data received
    InvalidUtf8,
    // Generic system error.  FIXME: make more granular
    System,
}

// Cipboard operation error
#[derive(Debug)]
pub struct Error {
    detail: ErrorDetail,
    source: Option<Box<dyn std::error::Error>>,
}

impl std::fmt::Display for ErrorDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match *self {
            #[cfg(target_os = "linux")]
            ErrorDetail::NoDisplayServer => write!(f, "no display server available"),
            ErrorDetail::InvalidUtf8 => write!(f, "invalid UTF-8"),
            ErrorDetail::System => write!(f, "system error"),
        }
    }
}

impl Error {
    #[cfg(target_os = "linux")]
    pub fn new(detail: ErrorDetail) -> Self {
        Error {
            detail,
            source: None,
        }
    }

    pub fn new_with_source<E: std::error::Error + 'static>(detail: ErrorDetail, source: E) -> Self {
        Error {
            detail,
            source: Some(source.into()),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        self.detail.fmt(f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref())
    }
}

impl std::convert::From<std::string::FromUtf8Error> for Error {
    fn from(value: std::string::FromUtf8Error) -> Error {
        Error::new_with_source(ErrorDetail::InvalidUtf8, value)
    }
}

impl std::convert::From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Error {
        Error::new_with_source(ErrorDetail::System, value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Backend {
    // Copy to clipboard.  Note that data is not Data; all copies are text/plain
    fn copy(&mut self, dest: Dest, data: &str) -> Result<()>;
    // Paste from clipboard
    fn paste(&mut self, source: Source) -> Result<Data>;
}
