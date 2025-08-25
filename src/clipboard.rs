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

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub trait Backend {
    fn copy(&mut self, dest: Dest, data: &str) -> Result<()>;
    fn paste(&mut self, src: Source) -> Result<Data>;
}
