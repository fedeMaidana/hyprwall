use std::path::PathBuf;

pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Thumbnail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thumbnail")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgba_bytes", &self.rgba.len())
            .finish()
    }
}

pub struct Wallpaper {
    pub path: PathBuf,
    pub label: String,
    pub thumb: Thumbnail,
}
