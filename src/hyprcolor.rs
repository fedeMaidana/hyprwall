//! Minimal reader for the hyprcolor palette (`~/.cache/hyprcolors/colors.json`).
//!
//! Hand-parsed on purpose: the file is produced by our own hyprcolor with a
//! stable `"key": "#rrggbb"` shape, and hyprwall stays dependency-free.

use std::{env, fs, path::PathBuf};

use crate::style::Color;

/// Panel hue taken from the wallpaper palette, keeping the panel's own
/// translucency. `None` when the palette is missing or malformed.
pub fn panel_color() -> Option<Color> {
    let content = fs::read_to_string(colors_json_path()?).ok()?;
    let hex = extract_hex(&content, "background")?;
    let (r, g, b) = parse_hex(&hex)?;

    Some(Color {
        r,
        g,
        b,
        a: Color::PANEL.a,
    })
}

fn colors_json_path() -> Option<PathBuf> {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return Some(
            PathBuf::from(cache_home)
                .join("hyprcolors")
                .join("colors.json"),
        );
    }

    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".cache")
            .join("hyprcolors")
            .join("colors.json")
    })
}

fn extract_hex(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let rest = &json[json.find(&needle)? + needle.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;

    Some(rest[..end].to_string())
}

fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
    let value = value.strip_prefix('#')?;

    if value.len() != 6 || !value.is_ascii() {
        return None;
    }

    let r = u8::from_str_radix(&value[0..2], 16).ok()?;
    let g = u8::from_str_radix(&value[2..4], 16).ok()?;
    let b = u8::from_str_radix(&value[4..6], 16).ok()?;

    Some((r, g, b))
}

#[cfg(test)]
#[path = "../tests/unit/hyprcolor.rs"]
mod tests;
