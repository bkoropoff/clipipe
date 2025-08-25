use crate::clipboard::{self, Data, Dest, Result, Source};

use std::env;
use std::io::Read;
use std::time::Duration;

use wl_clipboard_rs::{
    copy::{
        ClipboardType as CopyClipboardType, MimeType as CopyMimeType, Options, Source as CopySource,
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

impl clipboard::Backend for X11Backend {
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

impl clipboard::Backend for WaylandBackend {
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
                Err(PasteError::ClipboardEmpty | PasteError::NoSeats | PasteError::NoMimeType) => {
                    Data {
                        data: "".into(),
                        mime: None,
                    }
                }
                Err(err) => return Err(err.into()),
            },
        )
    }
}

pub enum Backend {
    Wayland(WaylandBackend),
    X11(X11Backend),
}

impl clipboard::Backend for Backend {
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
