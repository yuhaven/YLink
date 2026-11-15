//! Window geometry for the host shell.
//!
//! The core never renders video; it only decides where a plugin's window
//! should sit. Placement itself happens in the desktop layer (Win32
//! `SetWindowPos`), because that is the only piece that needs the window
//! handle a plugin reports via `window.report`.

use serde::{Deserialize, Serialize};

/// Screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn center_x(&self) -> i32 {
        self.x + (self.width as i32) / 2
    }

    pub fn center_y(&self) -> i32 {
        self.y + (self.height as i32) / 2
    }
}

/// What to apply to a plugin window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSpec {
    pub rect: Rect,
    pub always_on_top: bool,
    pub borderless: bool,
    pub fullscreen: bool,
}

/// Short-drama aspect ratio, width / height.
pub const PORTRAIT_RATIO: f64 = 9.0 / 16.0;

/// Aspect ratio presets the shell offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aspect {
    /// 9:16, short drama / live / shorts.
    Portrait,
    /// 3:4, tablet-ish content.
    Classic,
    /// 16:9, video and desktop.
    Landscape,
    /// Fill the work area, whatever the content is.
    Fill,
}

impl Aspect {
    pub fn ratio(self) -> Option<f64> {
        match self {
            Aspect::Portrait => Some(9.0 / 16.0),
            Aspect::Classic => Some(3.0 / 4.0),
            Aspect::Landscape => Some(16.0 / 9.0),
            Aspect::Fill => None,
        }
    }
}

impl WindowSpec {
    /// Fit `aspect` inside `work` using 90% of the available height, centered.
    ///
    /// Height is the driver on purpose: a 9:16 window sized by width would be
    /// taller than the screen on most 1080p displays.
    pub fn fit(work: Rect, aspect: Aspect, always_on_top: bool, borderless: bool) -> Self {
        let max_height = (work.height as f64 * 0.90).round() as u32;
        let (width, height) = match aspect.ratio() {
            Some(ratio) => {
                let h = max_height;
                let w = ((h as f64 * ratio).round() as u32).min(work.width);
                // If width had to be clamped, recompute height to keep ratio.
                let h = if w == work.width {
                    ((w as f64 / ratio).round() as u32).min(max_height)
                } else {
                    h
                };
                (w, h)
            }
            None => (work.width, work.height),
        };

        let x = work.x + ((work.width as i32 - width as i32) / 2);
        let y = work.y + ((work.height as i32 - height as i32) / 2);

        Self {
            rect: Rect::new(x, y, width, height),
            always_on_top,
            borderless,
            fullscreen: false,
        }
    }

    /// Default short-drama window: portrait, always on top, no chrome.
    pub fn portrait(work: Rect) -> Self {
        Self::fit(work, Aspect::Portrait, true, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_fits_inside_1080p_work_area() {
        let work = Rect::new(0, 0, 1920, 1040);
        let spec = WindowSpec::portrait(work);
        assert_eq!(spec.rect.height, 936); // 90% of 1040
        assert_eq!(spec.rect.width, 527); // 936 * 9/16, rounded
        assert!(spec.rect.width < work.width);
        assert!(spec.always_on_top && spec.borderless);
    }

    #[test]
    fn portrait_is_centered() {
        let work = Rect::new(100, 50, 1920, 1000);
        let spec = WindowSpec::portrait(work);
        let right_gap = (work.x + work.width as i32) - (spec.rect.x + spec.rect.width as i32);
        assert!((right_gap - (spec.rect.x - work.x)).abs() <= 1);
    }

    #[test]
    fn landscape_clamps_to_work_width() {
        let work = Rect::new(0, 0, 1280, 720);
        let spec = WindowSpec::fit(work, Aspect::Landscape, false, false);
        assert!(spec.rect.width <= work.width);
        assert!(spec.rect.height <= work.height);
    }

    #[test]
    fn fill_uses_whole_area() {
        let work = Rect::new(0, 0, 1600, 900);
        let spec = WindowSpec::fit(work, Aspect::Fill, false, false);
        assert_eq!(spec.rect, work);
    }
}
