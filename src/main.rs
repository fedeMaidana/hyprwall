use anyhow::Result;

mod app;
mod cli;
mod font;
mod geometry;
mod hyprcolor;
mod layout;
mod model;
mod picker;
mod render;
mod style;
mod wallpaper;

use app::AppState;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let wallpaper_dir = cli::wallpaper_dir_from_args();

    AppState::run(wallpaper_dir)
}
