use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use super::current::remember_current_wallpaper;

pub fn apply_wallpaper(path: &Path) -> Result<()> {
    let mut errors = Vec::new();

    if command_exists("awww") {
        match apply_with_awww(path) {
            Ok(()) => {
                remember_current_wallpaper(path);
                return Ok(());
            }
            Err(err) => errors.push(format!("awww: {err:#}")),
        }
    }

    if command_exists("swww") {
        match apply_with_swww(path) {
            Ok(()) => {
                remember_current_wallpaper(path);
                return Ok(());
            }
            Err(err) => errors.push(format!("swww: {err:#}")),
        }
    }

    if command_exists("hyprctl") {
        match apply_with_hyprpaper(path) {
            Ok(()) => {
                remember_current_wallpaper(path);
                return Ok(());
            }
            Err(err) => errors.push(format!("hyprpaper: {err:#}")),
        }
    }

    if errors.is_empty() {
        bail!("no encontré `awww`, `swww` ni `hyprctl` en PATH");
    }

    bail!("{}", errors.join("\n"))
}

fn apply_with_awww(path: &Path) -> Result<()> {
    ensure_awww_daemon_running()?;

    let wallpaper = path.to_string_lossy().to_string();

    run_command(
        "awww",
        &[
            "img",
            wallpaper.as_str(),
            "--transition-type",
            "grow",
            "--transition-pos",
            "0.85,0.15",
            "--transition-duration",
            "1.4",
            "--transition-fps",
            "60",
        ],
    )
}

fn apply_with_swww(path: &Path) -> Result<()> {
    ensure_swww_daemon_running()?;

    let wallpaper = path.to_string_lossy().to_string();

    run_command(
        "swww",
        &[
            "img",
            wallpaper.as_str(),
            "--transition-type",
            "grow",
            "--transition-pos",
            "0.85,0.15",
            "--transition-duration",
            "1.4",
            "--transition-fps",
            "60",
        ],
    )
}

fn ensure_awww_daemon_running() -> Result<()> {
    if let Some(socket_path) = awww_socket_path() {
        if socket_path.exists() {
            return Ok(());
        }
    }

    if !command_exists("awww-daemon") {
        bail!("`awww` está instalado, pero no encontré `awww-daemon` en PATH");
    }

    fs::create_dir_all(default_awww_cache_dir()).context("no se pudo crear ~/.cache/awww")?;

    Command::new("awww-daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("no se pudo iniciar awww-daemon")?;

    for _ in 0..30 {
        if let Some(socket_path) = awww_socket_path() {
            if socket_path.exists() {
                return Ok(());
            }
        }

        thread::sleep(Duration::from_millis(50));
    }

    bail!("awww-daemon inició, pero no apareció el socket de IPC")
}

fn ensure_swww_daemon_running() -> Result<()> {
    if swww_is_running() {
        return Ok(());
    }

    if !command_exists("swww-daemon") {
        bail!("`swww` está instalado, pero no encontré `swww-daemon` en PATH");
    }

    Command::new("swww-daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("no se pudo iniciar swww-daemon")?;

    for _ in 0..30 {
        if swww_is_running() {
            return Ok(());
        }

        thread::sleep(Duration::from_millis(50));
    }

    bail!("swww-daemon inició, pero `swww query` todavía falla")
}

fn swww_is_running() -> bool {
    Command::new("swww")
        .arg("query")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn awww_socket_path() -> Option<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")?;
    let wayland_display = env::var_os("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());

    let socket_name = format!("{}-awww-daemon.sock", wayland_display.to_string_lossy());

    Some(PathBuf::from(runtime_dir).join(socket_name))
}

fn default_awww_cache_dir() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    home.join(".cache").join("awww")
}

fn apply_with_hyprpaper(path: &Path) -> Result<()> {
    let wallpaper = path.to_string_lossy().to_string();
    let monitor_wallpaper = format!(",{wallpaper}");

    if let Err(err) = run_command("hyprctl", &["hyprpaper", "preload", wallpaper.as_str()]) {
        log::warn!("hyprpaper preload falló; intento aplicar igual: {err:#}");
    }

    run_command(
        "hyprctl",
        &["hyprpaper", "wallpaper", monitor_wallpaper.as_str()],
    )
}

fn run_command(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("ejecutando {program}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    bail!(
        "`{program}` falló con status {}.\nstdout: {}\nstderr: {}",
        output.status,
        stdout.trim(),
        stderr.trim()
    )
}

fn command_exists(command: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&path_var).any(|dir| dir.join(command).is_file())
}
