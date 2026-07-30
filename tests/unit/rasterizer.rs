use super::*;
use crate::{font::load_ui_font, layout, render::scene::build_scene, wallpaper::scan_wallpapers};
use std::path::Path;

#[test]
#[ignore]
fn render_dump_png() {
    let dir = std::env::var("WALL_DIR").unwrap_or_else(|_| "/tmp/bench_wallpapers".into());
    let out = std::env::var("WALL_OUT").unwrap_or_else(|_| "/tmp/hyprwall_render.png".into());
    let scale: f32 = std::env::var("SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);

    let wallpapers = scan_wallpapers(Path::new(&dir)).expect("scan_wallpapers");
    assert!(!wallpapers.is_empty());
    let font = load_ui_font().expect("font");

    let (logical_w, logical_h) = (1280u32, 560u32);
    let phys_w = (logical_w as f32 * scale).round() as u32;
    let phys_h = (logical_h as f32 * scale).round() as u32;
    let layout_result = layout::compute(logical_w, logical_h, wallpapers.len(), 0);
    let scene = build_scene(
        logical_w,
        logical_h,
        &layout_result,
        &wallpapers,
        0,
        Some(2),
        crate::style::Color::PANEL,
    );

    let mut canvas = vec![0u8; (phys_w * phys_h * 4) as usize];
    rasterize(&mut canvas, phys_w, phys_h, scale, &scene, &font);

    let mut rgba = vec![0u8; canvas.len()];
    let bg = [40u8, 40, 50];
    for (d, s) in rgba.chunks_exact_mut(4).zip(canvas.chunks_exact(4)) {
        let a = s[3] as u16;
        let inv = 255 - a;
        d[0] = ((s[2] as u16 * a + bg[0] as u16 * inv) / 255) as u8;
        d[1] = ((s[1] as u16 * a + bg[1] as u16 * inv) / 255) as u8;
        d[2] = ((s[0] as u16 * a + bg[2] as u16 * inv) / 255) as u8;
        d[3] = 255;
    }

    let int_size = tiny_skia::IntSize::from_wh(phys_w, phys_h).unwrap();
    let pixmap = tiny_skia::Pixmap::from_vec(rgba, int_size).expect("from_vec");
    pixmap.save_png(&out).expect("save_png");
    eprintln!("wrote {out} ({phys_w}x{phys_h} @ scale={scale})");
}
