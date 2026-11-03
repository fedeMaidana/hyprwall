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

    let logical_w = std::env::var("WIDTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1280u32);
    let logical_h = std::env::var("HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(560u32);
    let position = std::env::var("POSITION")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let phys_w = (logical_w as f32 * scale).round() as u32;
    let phys_h = (logical_h as f32 * scale).round() as u32;
    let layout_result = layout::compute(logical_w, logical_h, wallpapers.len(), position);
    let selection = crate::render::scene::Selection {
        selected: position.round() as usize % wallpapers.len(),
        hovered: Some(2),
        applied: None,
    };
    let scene = build_scene(
        logical_w,
        logical_h,
        &layout_result,
        &wallpapers,
        selection,
        crate::hyprcolor::DynamicColors::FALLBACK,
    );

    let mut canvas = vec![0u8; (phys_w * phys_h * 4) as usize];
    Rasterizer::default()
        .render(phys_w, phys_h, scale, &scene, &font)
        .expect("render")
        .copy_to(&mut canvas, true);

    let mut rgba = vec![0u8; canvas.len()];
    let bg = [40u8, 40, 50];
    for (d, s) in rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(canvas.as_chunks::<4>().0.iter())
    {
        let a = s[3] as u16;
        let inv = 255 - a;
        d[0] = (s[2] as u16 + bg[0] as u16 * inv / 255) as u8;
        d[1] = (s[1] as u16 + bg[1] as u16 * inv / 255) as u8;
        d[2] = (s[0] as u16 + bg[2] as u16 * inv / 255) as u8;
        d[3] = 255;
    }

    let int_size = tiny_skia::IntSize::from_wh(phys_w, phys_h).unwrap();
    let pixmap = tiny_skia::Pixmap::from_vec(rgba, int_size).expect("from_vec");
    pixmap.save_png(&out).expect("save_png");
    eprintln!("wrote {out} ({phys_w}x{phys_h} @ scale={scale})");
}

fn sample_wallpapers() -> Vec<crate::wallpaper::Wallpaper> {
    use crate::wallpaper::{Thumbnail, Wallpaper};
    (0..11)
        .map(|n| {
            let (width, height) = (624, 568);
            let mut rgba = vec![0; width * height * 4];
            for (i, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                pixel.copy_from_slice(&[
                    (i % width * 255 / width) as u8,
                    (i / width * 255 / height) as u8,
                    (n * 23) as u8,
                    255,
                ]);
            }
            Wallpaper {
                path: format!("/tmp/wallpaper-{n}.png").into(),
                label: format!("Wallpaper {n} — prueba"),
                thumb: Thumbnail {
                    width: width as u32,
                    height: height as u32,
                    rgba,
                },
            }
        })
        .collect()
}

#[test]
fn cached_frames_match_full_redraw_with_reused_and_skipped_buffers() {
    let wallpapers = sample_wallpapers();
    let font = load_ui_font().expect("font");
    let mut renderer = Rasterizer::default();
    for (w, h, scale) in [
        (1280, 560, 1.0),
        (1280, 800, 2.0),
        (640, 360, 1.0),
        (320, 200, 1.0),
    ] {
        let (pw, ph) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
        let mut buffers: Vec<_> = (0..3)
            .map(|_| (vec![137; (pw * ph * 4) as usize], None))
            .collect();
        let mut expected = vec![0; (pw * ph * 4) as usize];
        for (step, pos) in [0.0f32, 0.25, 0.9, 1.0, 5.4, 10.8, 11.0, -0.3, 0.0]
            .into_iter()
            .enumerate()
        {
            let layout = layout::compute(w, h, wallpapers.len(), pos);
            let selection = crate::render::scene::Selection {
                selected: (pos.round() as i32).rem_euclid(wallpapers.len() as i32) as usize,
                hovered: (step % 2 == 0).then_some(2),
                applied: Some(0),
            };
            let mut colors = crate::hyprcolor::DynamicColors::FALLBACK;
            if step >= 5 {
                colors.panel.r = 100;
            }
            let scene = build_scene(w, h, &layout, &wallpapers, selection, colors);
            rasterize(&mut expected, pw, ph, scale, &scene, &font);
            let frame = renderer
                .render(pw, ph, scale, &scene, &font)
                .expect("render");
            // Buffer 2 can be several frames old; damage must still cover its
            // obsolete pixels, including cards that have already left the view.
            let buffer_index = [0, 1, 2, 0, 1, 0, 1, 2, 0][step];
            let (canvas, generation) = &mut buffers[buffer_index];
            frame.copy_to(canvas, *generation != Some(frame.generation));
            *generation = Some(frame.generation);
            let mismatches = canvas.iter().zip(&expected).filter(|(a, b)| a != b).count();
            assert_eq!(mismatches, 0, "cached vs full: {w}x{h}@{scale} pos={pos}");
            assert!(frame.damage.x >= 0 && frame.damage.y >= 0);
            assert!(frame.damage.x + frame.damage.w <= pw as i32);
            assert!(frame.damage.y + frame.damage.h <= ph as i32);
            if h >= 560 {
                assert!(
                    frame.damage.h < ph as i32,
                    "ordinary navigation must not damage the whole screen"
                );
            }
        }
    }
}

#[test]
fn cache_invalidates_when_scale_changes_at_the_same_physical_size() {
    let wallpapers = sample_wallpapers();
    let font = load_ui_font().expect("font");
    let mut renderer = Rasterizer::default();
    let mut generation = 0;
    for scale in [1.0, 2.0, 1.0] {
        let (w, h) = ((1280.0 / scale) as u32, (800.0 / scale) as u32);
        let layout = layout::compute(w, h, wallpapers.len(), 0.5);
        let scene = build_scene(
            w,
            h,
            &layout,
            &wallpapers,
            crate::render::scene::Selection {
                selected: 1,
                hovered: None,
                applied: None,
            },
            crate::hyprcolor::DynamicColors::FALLBACK,
        );
        let mut expected = vec![0; 1280 * 800 * 4];
        rasterize(&mut expected, 1280, 800, scale, &scene, &font);
        let frame = renderer
            .render(1280, 800, scale, &scene, &font)
            .expect("render");
        assert_ne!(generation, frame.generation);
        generation = frame.generation;
        assert_eq!(frame.pixels, expected);
    }
}

/// Run in release mode; WALL_DIR optionally benchmarks real images.
/// cargo test --release render_scroll_benchmark -- --ignored --nocapture
#[test]
#[ignore = "manual performance measurement"]
fn render_scroll_benchmark() {
    use std::{hint::black_box, time::Instant};
    let wallpapers = std::env::var("WALL_DIR")
        .ok()
        .map_or_else(sample_wallpapers, |dir| {
            scan_wallpapers(Path::new(&dir)).expect("wallpapers")
        });
    assert!(!wallpapers.is_empty());
    let font = load_ui_font().expect("font");
    let (w, h, scale) = (1280u32, 800u32, 2.0);
    let (pw, ph) = (w * 2, h * 2);
    let mut renderer = Rasterizer::default();
    let mut buffers: Vec<_> = (0..3)
        .map(|_| (vec![0; (pw * ph * 4) as usize], None))
        .collect();
    let mut timings = Vec::new();
    for step in 0..190 {
        let start = Instant::now();
        let pos = (step % 60) as f32 / 60.0;
        let layout = layout::compute(w, h, wallpapers.len(), pos);
        let scene = build_scene(
            w,
            h,
            &layout,
            &wallpapers,
            crate::render::scene::Selection {
                selected: 1,
                hovered: None,
                applied: Some(0),
            },
            crate::hyprcolor::DynamicColors::FALLBACK,
        );
        let frame = renderer
            .render(pw, ph, scale, &scene, &font)
            .expect("render");
        let (canvas, generation) = &mut buffers[step % 3];
        frame.copy_to(canvas, *generation != Some(frame.generation));
        *generation = Some(frame.generation);
        black_box(canvas);
        if step >= 10 {
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    timings.sort_by(f64::total_cmp);
    eprintln!(
        "2560x1600 @2, {} wallpapers, median={:.3}ms p95={:.3}ms",
        wallpapers.len(),
        timings[timings.len() / 2],
        timings[timings.len() * 95 / 100]
    );
}
