use anyhow::{Context, Result};
use image::imageops::FilterType;
use std::{fs, path::Path};

use crate::{
    style,
    wallpaper::model::{Thumbnail, Wallpaper},
};

pub fn scan_wallpapers(dir: &Path) -> Result<Vec<Wallpaper>> {
    let mut paths = Vec::new();

    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && is_supported_image(&path) {
            paths.push(path.canonicalize().unwrap_or(path));
        }
    }

    paths.sort_by_key(|path| path.file_name().map(|name| name.to_owned()));

    let mut wallpapers = Vec::new();

    for path in paths {
        match load_wallpaper(&path) {
            Ok(wallpaper) => wallpapers.push(wallpaper),
            Err(err) => log::warn!("ignorando {}: {err:?}", path.display()),
        }
    }

    Ok(wallpapers)
}

pub fn load_wallpaper(path: &Path) -> Result<Wallpaper> {
    let image = image::open(path).with_context(|| format!("image::open {}", path.display()))?;

    let thumb = image
        .resize_to_fill(
            style::card::WIDTH as u32,
            style::card::HEIGHT as u32,
            FilterType::Lanczos3,
        )
        .to_rgba8();

    Ok(Wallpaper {
        path: path.to_path_buf(),
        label: wallpaper_label(path),
        thumb: Thumbnail {
            width: thumb.width(),
            height: thumb.height(),
            rgba: thumb.into_raw(),
        },
    })
}

fn wallpaper_label(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("wallpaper");

    raw.replace('_', " ")
        .replace('-', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
        .unwrap_or(false)
}
