use std::{env, path::PathBuf};

pub fn wallpaper_dir_from_args() -> PathBuf {
    env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_wallpaper_dir)
}

fn default_wallpaper_dir() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    home.join("Pictures").join("Wallpapers")
}
