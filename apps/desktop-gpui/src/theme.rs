//! Theme colors and visual constants for the ApiArk GPUI application.
//!
//! Light palette aligned with the original ApiArk desktop UI.
//! Note: `gpui::hsla` takes a normalized hue in `0..=1`, not degrees.

use gpui_kit::{hsla, Hsla};

/// Convert a degree hue (0..360) to GPUI's normalized hue (0..1).
pub(crate) fn hue(degrees: f32) -> f32 {
    degrees / 360.0
}

/// Background of the main application window.
pub fn bg_base() -> Hsla {
    hsla(hue(225.0), 0.33, 0.973, 1.0) // #f7f8fb
}

/// Slightly raised background (panels, sidebars).
pub fn bg_panel() -> Hsla {
    hsla(0.0, 0.0, 1.0, 1.0) // #ffffff
}

/// Elevated surface (inputs, cards, menus).
pub fn bg_surface() -> Hsla {
    hsla(hue(225.0), 0.40, 0.965, 1.0) // #f1f3f9
}

/// Hovered surface.
pub fn bg_hover() -> Hsla {
    hsla(hue(225.0), 0.40, 0.925, 1.0) // #e6e9f2
}

/// Primary border color.
pub fn border() -> Hsla {
    hsla(hue(222.0), 0.31, 0.87, 1.0) // #d5dae6
}

/// Subtle border color.
pub fn border_subtle() -> Hsla {
    hsla(hue(225.0), 0.33, 0.92, 1.0) // #e5e8f0
}

/// Primary text color.
pub fn text_primary() -> Hsla {
    hsla(hue(225.0), 0.21, 0.157, 1.0) // #1f2430
}

/// Secondary/muted text color.
pub fn text_muted() -> Hsla {
    hsla(hue(220.0), 0.09, 0.46, 1.0) // #6b7280
}

/// Accent color (indigo, matching the original Send button).
pub fn accent() -> Hsla {
    hsla(hue(240.0), 0.62, 0.60, 1.0) // #6366f1-ish
}

/// Accent color when hovered.
pub fn accent_hover() -> Hsla {
    hsla(hue(243.0), 0.75, 0.58, 1.0)
}

/// Success / HTTP 2xx green.
pub fn success() -> Hsla {
    hsla(hue(142.0), 0.71, 0.36, 1.0) // #16a34a
}

/// Warning / HTTP 3xx yellow.
pub fn warning() -> Hsla {
    hsla(hue(38.0), 0.92, 0.44, 1.0) // #d97706
}

/// Error / HTTP 4xx-5xx red.
pub fn error() -> Hsla {
    hsla(0.0, 0.84, 0.51, 1.0) // #dc2626
}

/// HTTP method colors (same assignments as the original Tauri version).
pub mod method {
    use crate::theme::hue;
    use gpui_kit::Hsla;

    pub fn get() -> Hsla {
        gpui_kit::hsla(hue(142.0), 0.71, 0.36, 1.0)
    }
    pub fn post() -> Hsla {
        gpui_kit::hsla(hue(38.0), 0.92, 0.44, 1.0)
    }
    pub fn put() -> Hsla {
        gpui_kit::hsla(hue(217.0), 0.83, 0.53, 1.0)
    }
    pub fn patch() -> Hsla {
        gpui_kit::hsla(hue(263.0), 0.70, 0.58, 1.0)
    }
    pub fn delete() -> Hsla {
        gpui_kit::hsla(0.0, 0.84, 0.51, 1.0)
    }
}
