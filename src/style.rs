#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Full-screen backdrop: keeps every pixel non-transparent so the
    /// compositor blur (layerrule) covers the whole output, and dims it.
    pub const SCRIM: Self = Self::rgba(0, 0, 0, 110);
    pub const PANEL: Self = Self::rgba(28, 32, 40, 105);
    /// Neutral stand-in when the hyprcolor palette is unavailable.
    pub const ACCENT_FALLBACK: Self = Self::rgba(230, 230, 240, 255);
    pub const CARD_BG: Self = Self::rgba(16, 18, 21, 255);
    pub const CARD_DIM: Self = Self::rgba(0, 0, 0, 68);
    pub const CARD_HAIRLINE: Self = Self::rgba(255, 255, 255, 26);
    pub const TEXT_ON_SELECTED: Self = Self::rgba(255, 255, 255, 255);

    pub const TEXT_SHADOW: Self = Self::rgba(0, 0, 0, 190);
    pub const HINT_TEXT: Self = Self::rgba(255, 255, 255, 92);

    pub const fn with_alpha(self, a: u8) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

pub mod surface {
    pub const WIDTH_HINT: u32 = 1280;
    pub const HEIGHT_HINT: u32 = 560;
}

pub mod panel {
    pub const HEIGHT: i32 = 392;
    pub const RADIUS: i32 = 30;
    pub const HORIZONTAL_PADDING: i32 = 56;
    pub const MIN_SCREEN_MARGIN: i32 = 56;
}

pub mod card {
    pub const WIDTH: i32 = 200;
    pub const HEIGHT: i32 = 284;

    pub const THUMB_DECODE_SCALE: u32 = 2;

    pub const INACTIVE_WIDTH: i32 = 176;
    pub const INACTIVE_HEIGHT: i32 = 250;

    pub const INACTIVE_SCALE_STEP_PERCENT: i32 = 12;
    pub const INACTIVE_MIN_SCALE_PERCENT: i32 = 76;

    pub const INACTIVE_DIM_BASE_ALPHA: u8 = 76;
    pub const INACTIVE_DIM_STEP_ALPHA: u8 = 24;
    pub const INACTIVE_DIM_MAX_ALPHA: u8 = 150;

    pub const GAP: i32 = 40;
    pub const RADIUS: i32 = 16;
}

pub mod selection {
    /// Ring around the selected card, drawn with the dynamic accent.
    pub const RING_OUTSET: i32 = 2;
    pub const RING_WIDTH: f32 = 2.0;
    pub const RING_ALPHA: u8 = 210;

    pub const GLOW_OUTSET: i32 = 10;
    pub const GLOW_ALPHA: u8 = 18;

    pub const HOVER_OUTSET: i32 = 3;
    pub const HOVER_WIDTH: f32 = 2.0;
    pub const HOVER_ALPHA: u8 = 120;

    pub const BADGE_MARGIN: i32 = 8;
    pub const BADGE_OUTER: i32 = 14;
    pub const BADGE_INNER: i32 = 8;
}

pub mod hints {
    pub const TEXT: &str = "‹ › elegir · Enter aplicar · Esc salir";
    pub const FONT_SIZE: f32 = 10.5;
    pub const STRIP_HEIGHT: i32 = 26;
}

pub mod label {
    pub const SELECTED_STRIP_HEIGHT: i32 = 42;
    pub const FONT_SIZE: f32 = 13.5;
    pub const TOP_GAP: i32 = 6;
    pub const LETTER_SPACING: f32 = 0.0;

    pub const GRADIENT_LIFT: i32 = 36;
    pub const GRADIENT_BOTTOM_ALPHA: u8 = 220;

    pub const TEXT_SHADOW_OFFSET_X: i32 = 0;
    pub const TEXT_SHADOW_OFFSET_Y: i32 = 1;
}
