//! Aurora Theme — VS Code-inspired visual styling with Aurora's unique identity.
//!
//! Provides color palette, frame builders, and theme setup for the Aurora editor.

use eframe::egui::{self, Color32, Frame, Margin, Rounding, Stroke, Vec2};

// ---------------------------------------------------------------------------
// Aurora Color Palette — VS Code Dark+ inspired with Aurora's purple-blue tones
// ---------------------------------------------------------------------------

// Core backgrounds
pub const BG_BASE: Color32 = Color32::from_rgb(26, 27, 38);   // Editor background
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(31, 34, 51); // Sidebar/panel background
pub const BG_ACTIVE: Color32 = Color32::from_rgb(36, 40, 59);  // Active/hover item
pub const BG_MUTED: Color32 = Color32::from_rgb(42, 46, 66);   // Subtle contrast

// Structural bars (VS Code-inspired)
pub const ACTIVITY_BAR_BG: Color32 = Color32::from_rgb(21, 22, 33); // Far left strip
pub const TITLE_BAR_BG: Color32 = Color32::from_rgb(30, 32, 48);    // Top bar
pub const STATUS_BAR_BG: Color32 = Color32::from_rgb(30, 32, 48);   // Bottom bar
pub const TAB_ACTIVE_BG: Color32 = BG_BASE;                          // Active tab = editor bg
pub const TAB_INACTIVE_BG: Color32 = Color32::from_rgb(34, 37, 54); // Inactive tab
pub const TAB_BAR_BG: Color32 = Color32::from_rgb(28, 30, 44);      // Tab strip background
pub const CURSOR_LINE_BG: Color32 = Color32::from_rgb(32, 34, 50);  // Current line highlight

// Borders
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(48, 52, 73);
pub const BORDER_SECTION: Color32 = Color32::from_rgb(38, 42, 60);  // Section dividers

// Text
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(169, 177, 214);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(120, 128, 170);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(86, 94, 120);
pub const TEXT_ACCENT: Color32 = Color32::from_rgb(137, 180, 250);
pub const TEXT_ACTIVITY_BAR: Color32 = Color32::from_rgb(255, 255, 255); // White icons
pub const TEXT_ACTIVITY_INACTIVE: Color32 = Color32::from_rgb(85, 89, 110);

// Syntax highlighting (Catppuccin Mocha inspired)
pub const SYNTAX_KEYWORD: Color32 = Color32::from_rgb(187, 154, 247);
pub const SYNTAX_STRING: Color32 = Color32::from_rgb(158, 206, 106);
pub const SYNTAX_NUMBER: Color32 = Color32::from_rgb(255, 158, 100);
pub const SYNTAX_FUNCTION: Color32 = Color32::from_rgb(125, 207, 255);
pub const SYNTAX_TYPE: Color32 = Color32::from_rgb(42, 195, 222);
pub const SYNTAX_COMMENT: Color32 = Color32::from_rgb(86, 94, 120);
pub const SYNTAX_OPERATOR: Color32 = Color32::from_rgb(137, 220, 235);
pub const SYNTAX_CONSTANT: Color32 = Color32::from_rgb(250, 179, 135);
pub const SYNTAX_VARIABLE: Color32 = Color32::from_rgb(169, 177, 214);

// Accents
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(137, 180, 250);
pub const ACCENT_GREEN: Color32 = Color32::from_rgb(166, 227, 161);
pub const ACCENT_YELLOW: Color32 = Color32::from_rgb(249, 226, 175);
pub const ACCENT_RED: Color32 = Color32::from_rgb(243, 139, 168);
pub const ACCENT_PURPLE: Color32 = Color32::from_rgb(203, 166, 247);
pub const ACCENT_CYAN: Color32 = Color32::from_rgb(42, 195, 222);
pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(250, 179, 135);

// Status indicators
pub const STATUS_MODIFIED: Color32 = ACCENT_YELLOW;
pub const STATUS_AI_READY: Color32 = ACCENT_GREEN;
pub const STATUS_AI_THINKING: Color32 = ACCENT_YELLOW;
pub const STATUS_AI_ACTIVE: Color32 = ACCENT_BLUE;

// Gutter
pub const GUTTER_LINE_NUM: Color32 = Color32::from_rgb(59, 66, 97);
pub const GUTTER_ACTIVE_LINE: Color32 = Color32::from_rgb(120, 128, 170);
pub const GUTTER_BG: Color32 = Color32::from_rgb(24, 25, 36); // Slightly different from editor
pub const GUTTER_ACTIVE_LINE_BG: Color32 = Color32::from_rgb(28, 30, 44);

// Selection & cursor
pub const SELECTION_BG: Color32 = Color32::from_rgb(54, 74, 130);
pub const CURSOR_COLOR: Color32 = Color32::from_rgb(192, 202, 245);

// File tree specific
pub const INDENT_GUIDE: Color32 = Color32::from_rgb(42, 46, 66);
pub const TREE_SELECTED_BG: Color32 = Color32::from_rgb(28, 30, 46);
pub const TREE_HOVER_BG: Color32 = Color32::from_rgb(35, 38, 56);

// Activity bar specific
pub const ACTIVITY_BAR_ACTIVE_INDICATOR: Color32 = ACCENT_BLUE;

// ---------------------------------------------------------------------------
// Frame builders
// ---------------------------------------------------------------------------

pub fn editor_frame() -> Frame {
    Frame {
        fill: BG_BASE,
        inner_margin: Margin::symmetric(0.0, 0.0),
        ..Frame::none()
    }
}

pub fn sidebar_frame() -> Frame {
    Frame {
        fill: BG_SIDEBAR,
        inner_margin: Margin::symmetric(0.0, 0.0),
        ..Frame::none()
    }
}

pub fn activity_bar_frame() -> Frame {
    Frame {
        fill: ACTIVITY_BAR_BG,
        inner_margin: Margin::symmetric(0.0, 0.0),
        ..Frame::none()
    }
}

pub fn status_bar_frame() -> Frame {
    Frame {
        fill: STATUS_BAR_BG,
        inner_margin: Margin::symmetric(0.0, 0.0),
        ..Frame::none()
    }
}

pub fn panel_frame() -> Frame {
    Frame {
        fill: BG_SIDEBAR,
        inner_margin: Margin::symmetric(0.0, 0.0),
        ..Frame::none()
    }
}

pub fn title_bar_frame() -> Frame {
    Frame {
        fill: TITLE_BAR_BG,
        inner_margin: Margin::symmetric(8.0, 0.0),
        ..Frame::none()
    }
}

pub fn popup_frame() -> Frame {
    Frame {
        fill: BG_ACTIVE,
        rounding: Rounding::same(6.0),
        stroke: Stroke::new(1.0, BORDER_SUBTLE),
        ..Frame::none()
    }
}

// ---------------------------------------------------------------------------
// Theme setup
// ---------------------------------------------------------------------------

pub fn setup_aurora_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    style.spacing.item_spacing = Vec2::new(6.0, 2.0);
    style.spacing.button_padding = Vec2::new(6.0, 2.0);
    style.spacing.indent = 12.0;
    style.spacing.menu_margin = Margin::symmetric(1.0, 1.0);
    style.spacing.scroll = egui::style::ScrollStyle::solid();

    let visuals = &mut style.visuals;
    visuals.dark_mode = true;
    visuals.window_fill = BG_BASE;
    visuals.panel_fill = BG_BASE;
    visuals.faint_bg_color = BG_MUTED;
    visuals.extreme_bg_color = BG_SIDEBAR;
    visuals.code_bg_color = BG_MUTED;
    visuals.warn_fg_color = ACCENT_YELLOW;
    visuals.error_fg_color = ACCENT_RED;
    visuals.hyperlink_color = ACCENT_BLUE;
    visuals.selection = egui::style::Selection {
        bg_fill: SELECTION_BG,
        stroke: Stroke::new(0.0, Color32::TRANSPARENT),
    };

    visuals.window_rounding = Rounding::same(0.0);
    visuals.window_shadow = egui::epaint::Shadow {
        extrusion: 0.0,
        color: Color32::TRANSPARENT,
    };

    visuals.widgets.noninteractive.bg_fill = BG_MUTED;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(0.0, Color32::TRANSPARENT);
    visuals.widgets.noninteractive.rounding = Rounding::same(3.0);
    visuals.widgets.noninteractive.weak_bg_fill = BG_ACTIVE;

    visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    visuals.widgets.inactive.bg_stroke = Stroke::new(0.0, Color32::TRANSPARENT);
    visuals.widgets.inactive.rounding = Rounding::same(3.0);
    visuals.widgets.inactive.weak_bg_fill = BG_MUTED;

    visuals.widgets.hovered.bg_fill = BG_ACTIVE;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT_PRIMARY);
    visuals.widgets.hovered.bg_stroke = Stroke::new(0.0, Color32::TRANSPARENT);
    visuals.widgets.hovered.rounding = Rounding::same(3.0);
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = BG_MUTED;
    visuals.widgets.active.fg_stroke = Stroke::new(2.0, TEXT_PRIMARY);
    visuals.widgets.active.bg_stroke = Stroke::new(0.0, Color32::TRANSPARENT);
    visuals.widgets.active.rounding = Rounding::same(3.0);

    visuals.widgets.open.bg_fill = BG_ACTIVE;
    visuals.widgets.open.fg_stroke = Stroke::new(2.0, ACCENT_BLUE);

    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.button_frame = false;
    visuals.collapsing_header_frame = false;

    style.text_styles = [
        (
            egui::TextStyle::Heading,
            egui::FontId::new(16.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Name("heading".into()),
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::new(13.0, egui::FontFamily::Monospace),
        ),
        (
            egui::TextStyle::Button,
            egui::FontId::new(12.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Small,
            egui::FontId::new(11.0, egui::FontFamily::Proportional),
        ),
    ]
    .into();

    ctx.set_style(style);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn scope_to_color(scope: &str) -> Color32 {
    match scope {
        "keyword" => SYNTAX_KEYWORD,
        "string" => SYNTAX_STRING,
        "number" => SYNTAX_NUMBER,
        "function" | "function.method" | "function.builtin" => SYNTAX_FUNCTION,
        "type" | "type.builtin" | "interface" => SYNTAX_TYPE,
        "comment" | "comment.line" | "comment.block" => SYNTAX_COMMENT,
        "operator" => SYNTAX_OPERATOR,
        "constant" | "constant.builtin" => SYNTAX_CONSTANT,
        "variable" | "variable.other" => SYNTAX_VARIABLE,
        "parameter" => TEXT_PRIMARY,
        "property" => TEXT_ACCENT,
        "punctuation" => TEXT_SECONDARY,
        "keyword.control.import" => SYNTAX_KEYWORD,
        "keyword.control.return" => SYNTAX_KEYWORD,
        "storage.type" => SYNTAX_TYPE,
        _ => TEXT_PRIMARY,
    }
}
