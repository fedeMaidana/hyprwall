use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn current_wallpaper_path() -> Option<PathBuf> {
    current_from_hyprpaper()
        .or_else(current_from_swww)
        .or_else(current_from_cache)
}

pub(super) fn remember_current_wallpaper(path: &Path) {
    let Some(cache_file) = current_wallpaper_cache_file() else {
        return;
    };

    let Some(cache_dir) = cache_file.parent() else {
        return;
    };

    if let Err(err) = fs::create_dir_all(cache_dir) {
        log::warn!("no se pudo crear cache dir para wallpaper actual: {err:?}");
        return;
    }

    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    if let Err(err) = fs::write(&cache_file, path.to_string_lossy().as_bytes()) {
        log::warn!(
            "no se pudo guardar wallpaper actual en {}: {err:?}",
            cache_file.display()
        );
    }

    remember_current_wallpaper_symlink(&path);
}

fn remember_current_wallpaper_symlink(path: &Path) {
    let Some(symlink_path) = current_wallpaper_image_symlink() else {
        return;
    };

    if symlink_path.exists() || symlink_path.is_symlink() {
        if let Err(err) = fs::remove_file(&symlink_path) {
            log::warn!(
                "no se pudo reemplazar symlink de wallpaper actual {}: {err:?}",
                symlink_path.display()
            );
            return;
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        if let Err(err) = symlink(path, &symlink_path) {
            log::warn!(
                "no se pudo crear symlink de wallpaper actual {} -> {}: {err:?}",
                symlink_path.display(),
                path.display()
            );
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(err) = fs::copy(path, &symlink_path) {
            log::warn!(
                "no se pudo copiar wallpaper actual a {}: {err:?}",
                symlink_path.display()
            );
        }
    }
}

fn current_from_hyprpaper() -> Option<PathBuf> {
    let output = Command::new("hyprctl")
        .args(["hyprpaper", "listactive"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    first_existing_path_from_text(&String::from_utf8_lossy(&output.stdout))
}

fn current_from_swww() -> Option<PathBuf> {
    let output = Command::new("swww").arg("query").output().ok()?;

    if !output.status.success() {
        return None;
    }

    first_existing_path_from_text(&String::from_utf8_lossy(&output.stdout))
}

fn current_from_cache() -> Option<PathBuf> {
    let cache_file = current_wallpaper_cache_file()?;
    let path = fs::read_to_string(cache_file).ok()?;
    let path = PathBuf::from(path.trim());

    path.exists().then_some(path)
}

fn first_existing_path_from_text(text: &str) -> Option<PathBuf> {
    text.lines().find_map(existing_path_from_line)
}

fn existing_path_from_line(line: &str) -> Option<PathBuf> {
    let candidates = [
        value_after(line, "image:"),
        value_after(line, "Image:"),
        value_after(line, "path:"),
        value_after(line, "Path:"),
        value_after(line, ":"),
        value_after(line, "="),
    ];

    candidates
        .into_iter()
        .flatten()
        .map(clean_path)
        .find(|path| path.exists())
}

fn value_after<'a>(line: &'a str, separator: &str) -> Option<&'a str> {
    line.split_once(separator).map(|(_, value)| value.trim())
}

fn clean_path(path: &str) -> PathBuf {
    let path = path.trim().trim_matches('"').trim_matches('\'');

    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    PathBuf::from(path)
}

fn current_wallpaper_cache_file() -> Option<PathBuf> {
    hyprwall_cache_dir().map(|dir| dir.join("current_wallpaper"))
}

fn current_wallpaper_image_symlink() -> Option<PathBuf> {
    hyprwall_cache_dir().map(|dir| dir.join("current_wallpaper_image"))
}

fn hyprwall_cache_dir() -> Option<PathBuf> {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(cache_home).join("hyprwall"));
    }

    env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache").join("hyprwall"))
}
