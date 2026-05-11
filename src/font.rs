use anyhow::{Context, Result, anyhow, bail};
use fontdue::{Font, FontSettings};
use std::{env, fs, process::Command};

pub fn load_ui_font() -> Result<Font> {
    if let Ok(path) = env::var("WALL_SELECT_FONT") {
        log::info!("using UI font from WALL_SELECT_FONT: {path}");
        return load_font_from_path(&path);
    }

    let preferred_fonts = [
        "JetBrainsMono Nerd Font:style=Medium",
        "JetBrains Mono:style=Medium",
        "Iosevka Nerd Font:style=Regular",
        "Iosevka:style=Regular",
        "FiraCode Nerd Font:style=Retina",
        "Fira Code:style=Retina",
        "Inter:style=Medium",
        "Noto Sans:style=Regular",
        "Cantarell:style=Regular",
        "Roboto:style=Regular",
        "DejaVu Sans:style=Book",
    ];

    for family in preferred_fonts {
        if let Ok((font, path)) = load_font_from_fontconfig(family) {
            log::info!("using UI font: {family} -> {path}");
            return Ok(font);
        }
    }

    let fallback_paths = [
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Medium.ttf",
        "/usr/share/fonts/TTF/JetBrainsMono-Medium.ttf",
        "/usr/share/fonts/TTF/IosevkaNerdFont-Regular.ttf",
        "/usr/share/fonts/TTF/Iosevka-Regular.ttf",
        "/usr/share/fonts/TTF/FiraCodeNerdFont-Retina.ttf",
        "/usr/share/fonts/TTF/FiraCode-Retina.ttf",
        "/usr/share/fonts/TTF/Inter-Medium.ttf",
        "/usr/share/fonts/inter/Inter-Medium.ttf",
        "/usr/share/fonts/TTF/Inter-Regular.ttf",
        "/usr/share/fonts/inter/Inter-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/NotoSans-Regular.ttf",
        "/usr/share/fonts/cantarell/Cantarell-VF.otf",
        "/usr/share/fonts/TTF/Roboto-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    ];

    for path in fallback_paths {
        if let Ok(font) = load_font_from_path(path) {
            log::info!("using UI font from fallback path: {path}");
            return Ok(font);
        }
    }

    bail!("no encontré una fuente usable para dibujar labels");
}

fn load_font_from_fontconfig(family: &str) -> Result<(Font, String)> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}", family])
        .output()
        .with_context(|| format!("no se pudo ejecutar fc-match para {family}"))?;

    if !output.status.success() {
        bail!("fc-match falló para {family}");
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();

    if path.is_empty() {
        bail!("fc-match no devolvió ruta para {family}");
    }

    let font = load_font_from_path(&path)?;

    Ok((font, path))
}

fn load_font_from_path(path: &str) -> Result<Font> {
    let bytes = fs::read(path).with_context(|| format!("no se pudo leer la fuente {path}"))?;

    Font::from_bytes(bytes, FontSettings::default())
        .map_err(|err| anyhow!("no se pudo cargar la fuente {path}: {err}"))
}
