use std::path::PathBuf;

pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct Wallpaper {
    pub path: PathBuf,
    pub label: String,
    pub thumb: Thumbnail,
}
