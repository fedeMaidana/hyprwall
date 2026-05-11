mod applier;
mod current;
mod loader;
mod model;

pub use applier::apply_wallpaper;
pub use current::current_wallpaper_path;
pub use loader::scan_wallpapers;
pub use model::{Thumbnail, Wallpaper};
