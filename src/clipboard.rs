pub enum Source {
    Default,
    Primary,
    Clipboard,
}

pub enum Dest {
    Default,
    Primary,
    Clipboard,
    Both,
}

pub struct Data {
    pub data: String,
    pub mime: Option<String>,
}

#[derive(Debug)]
pub enum ErrorDetail {
    #[cfg(target_os = "linux")]
    NoDisplayServer,
    InvalidUtf8,
    System,
}

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
        self.source.as_ref().map(|e| &**e)
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
    fn copy(&mut self, dest: Dest, data: &str) -> Result<()>;
    fn paste(&mut self, src: Source) -> Result<Data>;
}
