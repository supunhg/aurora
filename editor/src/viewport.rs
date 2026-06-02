//! Viewport management for virtual scrolling.
//!
//! The viewport determines which lines of the buffer are visible in the
//! editor window. Only these lines are rendered; non-visible lines are
//! skipped. The viewport tracks:
//!
//! - The first visible line
//! - The number of visible lines
//! - The scroll offset (in lines)
//! - The visual line height (in pixels, configurable)
//!
//! ## Virtual Scrolling Strategy
//!
//! We render only the visible lines + a buffer zone (2x viewport height)
//! above and below to handle smooth scrolling without flickering.

use serde::{Deserialize, Serialize};

/// Configuration for viewport rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportConfig {
    /// Height of a single line in pixels (including padding).
    pub line_height_px: f32,
    /// Extra lines to render above and below the visible area (smooth scroll buffer).
    pub scroll_buffer_lines: usize,
    /// Maximum number of lines that can be rendered in a frame.
    pub max_render_lines: usize,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        ViewportConfig {
            line_height_px: 22.0,
            scroll_buffer_lines: 50,
            max_render_lines: 5000,
        }
    }
}

/// The visible region of the buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    /// The first visible line (0-based).
    pub first_line: usize,
    /// The number of visible lines (height of viewport in lines).
    pub visible_lines: usize,
    /// Total number of lines in the buffer (used to clamp scroll position).
    pub total_lines: usize,
    /// Current scroll offset in lines (may be fractional for smooth scrolling).
    pub scroll_offset_lines: f64,
    /// Configuration for rendering.
    config: ViewportConfig,
}

impl Viewport {
    /// Create a new viewport with the given height in lines.
    pub fn new(visible_lines: usize, total_lines: usize) -> Self {
        Viewport {
            first_line: 0,
            visible_lines,
            total_lines,
            scroll_offset_lines: 0.0,
            config: ViewportConfig::default(),
        }
    }

    /// Create a viewport with a custom configuration.
    pub fn with_config(visible_lines: usize, total_lines: usize, config: ViewportConfig) -> Self {
        Viewport {
            first_line: 0,
            visible_lines,
            total_lines,
            scroll_offset_lines: 0.0,
            config,
        }
    }

    // ------------------------------------------------------------------
    // Resize & content changes
    // ------------------------------------------------------------------

    /// Update the visible line count (e.g., on window resize).
    pub fn resize(&mut self, new_visible_lines: usize) {
        self.visible_lines = new_visible_lines;
        self.clamp_scroll();
    }

    /// Update the total number of lines in the buffer.
    pub fn set_total_lines(&mut self, total: usize) {
        let old_total = self.total_lines;
        self.total_lines = total;
        if total < old_total && self.first_line > 0 {
            // Lines were removed; adjust scroll
            self.clamp_scroll();
        }
    }

    // ------------------------------------------------------------------
    // Scroll operations
    // ------------------------------------------------------------------

    /// Scroll up by `delta` lines. `delta` can be fractional for smooth scrolling.
    pub fn scroll_up(&mut self, delta: f64) {
        self.scroll_offset_lines = (self.scroll_offset_lines - delta).max(0.0);
        self.first_line = self.scroll_offset_lines.floor() as usize;
        self.clamp_scroll();
    }

    /// Scroll down by `delta` lines.
    pub fn scroll_down(&mut self, delta: f64) {
        self.scroll_offset_lines = (self.scroll_offset_lines + delta)
            .min((self.total_lines.saturating_sub(self.visible_lines)) as f64);
        self.first_line = self.scroll_offset_lines.floor() as usize;
        self.clamp_scroll();
    }

    /// Scroll to a specific line (making it the first visible line).
    pub fn scroll_to_line(&mut self, line: usize) {
        let max_first = self.total_lines.saturating_sub(self.visible_lines);
        self.first_line = line.min(max_first);
        self.scroll_offset_lines = self.first_line as f64;
    }

    /// Ensure that a given line is visible. If it's outside the visible
    /// range, the viewport is scrolled minimally to bring it into view.
    pub fn ensure_visible(&mut self, line: usize) {
        if line < self.first_line {
            self.scroll_to_line(line);
        } else if line >= self.first_line + self.visible_lines {
            let scroll_to = line.saturating_sub(self.visible_lines) + 1;
            self.scroll_to_line(scroll_to);
        }
    }

    /// Scroll by one page up.
    pub fn page_up(&mut self) {
        let delta = self.visible_lines.saturating_sub(1) as f64;
        self.scroll_up(delta);
    }

    /// Scroll by one page down.
    pub fn page_down(&mut self) {
        let delta = self.visible_lines.saturating_sub(1) as f64;
        self.scroll_down(delta);
    }

    /// Scroll to the top of the buffer.
    pub fn scroll_to_top(&mut self) {
        self.scroll_to_line(0);
    }

    /// Scroll to the bottom of the buffer.
    pub fn scroll_to_bottom(&mut self) {
        let last_line = self.total_lines.saturating_sub(1);
        self.scroll_to_line(last_line.saturating_sub(self.visible_lines.saturating_sub(1)));
    }

    // ------------------------------------------------------------------
    // Query methods
    // ------------------------------------------------------------------

    /// Get the range of lines that should be rendered.
    /// Returns `(start_line, end_line_exclusive)`.
    pub fn render_range(&self) -> (usize, usize) {
        let start = self
            .first_line
            .saturating_sub(self.config.scroll_buffer_lines);
        let end = (self.first_line + self.visible_lines + self.config.scroll_buffer_lines)
            .min(self.total_lines);
        (start, end)
    }

    /// Get the number of lines to render in the current frame.
    pub fn render_line_count(&self) -> usize {
        let (start, end) = self.render_range();
        (end - start).min(self.config.max_render_lines)
    }

    /// Get the pixel offset for the first visible line from the top of the viewport.
    /// This accounts for smooth scrolling (fractional scroll offset).
    pub fn pixel_offset_y(&self) -> f32 {
        let fractional = self.scroll_offset_lines - self.first_line as f64;
        -(fractional as f32) * self.config.line_height_px
    }

    /// Get the line at the given pixel y-coordinate within the viewport.
    pub fn line_at_y(&self, y: f32) -> usize {
        let line_f = y / self.config.line_height_px;
        let line = line_f.floor() as isize + self.first_line as isize;
        let max_line = self.total_lines.saturating_sub(1) as isize;
        line.max(0).min(max_line) as usize
    }

    /// Get the maximum scroll value (in lines).
    pub fn max_scroll(&self) -> f64 {
        self.total_lines.saturating_sub(self.visible_lines) as f64
    }

    /// Returns the scroll progress as a value between 0.0 and 1.0.
    pub fn scroll_progress(&self) -> f64 {
        let max = self.max_scroll();
        if max <= 0.0 {
            return 0.0;
        }
        (self.scroll_offset_lines / max).clamp(0.0, 1.0)
    }

    /// Returns `true` if the viewport is at the top of the buffer.
    pub fn is_at_top(&self) -> bool {
        self.first_line == 0
    }

    /// Returns `true` if the viewport is at the bottom of the buffer.
    pub fn is_at_bottom(&self) -> bool {
        self.first_line + self.visible_lines >= self.total_lines
    }

    // ------------------------------------------------------------------
    // Internal
    // ------------------------------------------------------------------

    fn clamp_scroll(&mut self) {
        let max_first = self.total_lines.saturating_sub(self.visible_lines);
        if self.first_line > max_first {
            self.first_line = max_first;
            self.scroll_offset_lines = max_first as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_viewport() {
        let vp = Viewport::new(30, 100);
        assert_eq!(vp.first_line, 0);
        assert_eq!(vp.visible_lines, 30);
        assert_eq!(vp.total_lines, 100);
    }

    #[test]
    fn test_scroll_down() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_down(10.0);
        assert_eq!(vp.first_line, 10);
    }

    #[test]
    fn test_scroll_up() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_down(50.0);
        vp.scroll_up(10.0);
        assert_eq!(vp.first_line, 40);
    }

    #[test]
    fn test_scroll_clamped_to_bottom() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_line(80); // max first line = 70
        assert_eq!(vp.first_line, 70);
    }

    #[test]
    fn test_scroll_to_bottom() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_bottom();
        assert_eq!(vp.first_line, 70);
    }

    #[test]
    fn test_ensure_visible_already_visible() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_line(10);
        vp.ensure_visible(15);
        assert_eq!(vp.first_line, 10); // unchanged
    }

    #[test]
    fn test_ensure_visible_below_viewport() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_line(10);
        vp.ensure_visible(50); // 10..40, 50 > 39
        assert_eq!(vp.first_line, 21); // 50 - 30 + 1
    }

    #[test]
    fn test_ensure_visible_above_viewport() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_line(20);
        vp.ensure_visible(5);
        assert_eq!(vp.first_line, 5);
    }

    #[test]
    fn test_page_down() {
        let mut vp = Viewport::new(30, 100);
        vp.page_down();
        assert_eq!(vp.first_line, 29);
    }

    #[test]
    fn test_render_range() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_line(10);
        let (start, end) = vp.render_range();
        assert_eq!(start, 0); // 10 - 50 = 0 (clamped)
        assert_eq!(end, 90); // 10 + 30 + 50 = 90
    }

    #[test]
    fn test_scroll_progress() {
        let vp = Viewport::new(30, 100);
        assert_eq!(vp.scroll_progress(), 0.0);
    }

    #[test]
    fn test_is_at_top_bottom() {
        let mut vp = Viewport::new(30, 100);
        assert!(vp.is_at_top());
        assert!(!vp.is_at_bottom());
        vp.scroll_to_bottom();
        assert!(!vp.is_at_top());
        assert!(vp.is_at_bottom());
    }

    #[test]
    fn test_line_at_y() {
        let vp = Viewport::new(30, 100);
        assert_eq!(vp.line_at_y(44.0), 2); // 44 / 22 = 2
    }

    #[test]
    fn test_resize() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_line(80);
        assert_eq!(vp.first_line, 70);
        vp.resize(50); // now max first line = 50
        assert_eq!(vp.first_line, 50);
    }

    #[test]
    fn test_set_total_lines_shrink() {
        let mut vp = Viewport::new(30, 100);
        vp.scroll_to_line(60);
        vp.set_total_lines(50);
        assert_eq!(vp.first_line, 20); // clamped: 50 - 30 = 20
    }
}
