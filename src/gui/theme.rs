use eframe::egui;

/// Modern glassy theme colors
pub struct ThemeColors;

impl ThemeColors {
    // Primary accent - vibrant blue
    pub fn accent() -> egui::Color32 {
        egui::Color32::from_rgb(64, 156, 255)
    }

    pub fn accent_hover() -> egui::Color32 {
        egui::Color32::from_rgb(100, 175, 255)
    }

    pub fn accent_dim() -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(64, 156, 255, 40)
    }

    // Background layers (dark mode)
    pub fn bg_dark() -> egui::Color32 {
        egui::Color32::from_rgb(15, 17, 23)
    }

    pub fn bg_panel() -> egui::Color32 {
        egui::Color32::from_rgb(22, 25, 35)
    }

    pub fn bg_elevated() -> egui::Color32 {
        egui::Color32::from_rgb(30, 34, 46)
    }

    pub fn bg_glass() -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(40, 45, 60, 180)
    }

    pub fn bg_glass_hover() -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(55, 60, 80, 200)
    }

    // Sidebar
    pub fn sidebar_bg() -> egui::Color32 {
        egui::Color32::from_rgb(18, 20, 28)
    }

    pub fn sidebar_selected() -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(64, 156, 255, 30)
    }

    // Text colors
    pub fn text_primary() -> egui::Color32 {
        egui::Color32::from_rgb(240, 242, 248)
    }

    pub fn text_secondary() -> egui::Color32 {
        egui::Color32::from_rgb(160, 165, 180)
    }

    pub fn text_muted() -> egui::Color32 {
        egui::Color32::from_rgb(100, 105, 120)
    }

    // Borders
    pub fn border() -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(100, 110, 130, 60)
    }

    pub fn border_light() -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8)
    }

    // Status colors
    pub fn success() -> egui::Color32 {
        egui::Color32::from_rgb(80, 200, 120)
    }

    #[allow(dead_code)]
    pub fn warning() -> egui::Color32 {
        egui::Color32::from_rgb(245, 180, 60)
    }

    #[allow(dead_code)]
    pub fn error() -> egui::Color32 {
        egui::Color32::from_rgb(240, 80, 90)
    }

    #[allow(dead_code)]
    pub fn info() -> egui::Color32 {
        egui::Color32::from_rgb(80, 160, 240)
    }
}

pub fn setup_custom_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Modern spacing
    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(16.0);
    style.spacing.menu_margin = egui::Margin::same(8.0);

    // Smooth rounded corners for glassy feel
    let smooth_rounding = egui::Rounding::same(10.0);
    let button_rounding = egui::Rounding::same(8.0);

    style.visuals.window_rounding = smooth_rounding;
    style.visuals.menu_rounding = button_rounding;
    style.visuals.popup_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 4.0),
        blur: 16.0,
        spread: 0.0,
        color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 80),
    };

    // Widget styling
    style.visuals.widgets.noninteractive.rounding = button_rounding;
    style.visuals.widgets.inactive.rounding = button_rounding;
    style.visuals.widgets.hovered.rounding = button_rounding;
    style.visuals.widgets.active.rounding = button_rounding;
    style.visuals.widgets.open.rounding = button_rounding;

    // Glassy inactive widgets
    style.visuals.widgets.inactive.bg_fill = ThemeColors::bg_glass();
    style.visuals.widgets.inactive.weak_bg_fill = ThemeColors::bg_elevated();
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, ThemeColors::border());
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, ThemeColors::text_secondary());

    // Hovered state with glow effect
    style.visuals.widgets.hovered.bg_fill = ThemeColors::bg_glass_hover();
    style.visuals.widgets.hovered.weak_bg_fill = ThemeColors::bg_glass_hover();
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ThemeColors::accent());
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ThemeColors::text_primary());

    // Active/pressed state
    style.visuals.widgets.active.bg_fill = ThemeColors::accent();
    style.visuals.widgets.active.weak_bg_fill = ThemeColors::accent_dim();
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, ThemeColors::accent_hover());
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, ThemeColors::text_primary());

    // Noninteractive (labels, etc.)
    style.visuals.widgets.noninteractive.bg_fill = ThemeColors::bg_elevated();
    style.visuals.widgets.noninteractive.weak_bg_fill = ThemeColors::bg_panel();
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, ThemeColors::text_primary());

    // Selection
    style.visuals.selection.bg_fill = ThemeColors::accent_dim();
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ThemeColors::accent());

    // Hyperlinks
    style.visuals.hyperlink_color = ThemeColors::accent();

    // Striped backgrounds for tables
    style.visuals.striped = true;

    // Extreme dark background
    style.visuals.extreme_bg_color = ThemeColors::bg_dark();
    style.visuals.faint_bg_color = ThemeColors::bg_elevated();
    style.visuals.code_bg_color = ThemeColors::bg_elevated();

    // Window styling
    style.visuals.window_fill = ThemeColors::bg_panel();
    style.visuals.window_stroke = egui::Stroke::new(1.0, ThemeColors::border());
    style.visuals.panel_fill = ThemeColors::bg_panel();

    // Text colors
    style.visuals.override_text_color = Some(ThemeColors::text_primary());

    // Separator color
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, ThemeColors::border());

    ctx.set_style(style);
}

/// Creates a glassy frame for panels
pub fn glass_frame(_ui: &egui::Ui) -> egui::Frame {
    egui::Frame::none()
        .fill(ThemeColors::bg_glass())
        .stroke(egui::Stroke::new(1.0, ThemeColors::border_light()))
        .rounding(12.0)
        .inner_margin(16.0)
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 2.0),
            blur: 8.0,
            spread: 0.0,
            color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 40),
        })
}

/// Creates an elevated card frame
pub fn card_frame(_ui: &egui::Ui) -> egui::Frame {
    egui::Frame::none()
        .fill(ThemeColors::bg_elevated())
        .stroke(egui::Stroke::new(1.0, ThemeColors::border()))
        .rounding(12.0)
        .inner_margin(20.0)
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 12.0,
            spread: 0.0,
            color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 50),
        })
}

/// Creates a subtle accent card
#[allow(dead_code)]
pub fn accent_card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(ThemeColors::accent_dim())
        .stroke(egui::Stroke::new(1.0, ThemeColors::accent()))
        .rounding(10.0)
        .inner_margin(16.0)
}

/// Status indicator frame
#[allow(dead_code)]
pub fn status_frame(good: bool) -> egui::Frame {
    let color = if good { ThemeColors::success() } else { ThemeColors::error() };
    egui::Frame::none()
        .fill(egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 20))
        .stroke(egui::Stroke::new(1.0, color))
        .rounding(8.0)
        .inner_margin(12.0)
}

// Utility color functions for consistent theming across panels
#[allow(dead_code)]
pub fn status_color(good: bool) -> egui::Color32 {
    if good {
        ThemeColors::success()
    } else {
        ThemeColors::error()
    }
}

#[allow(dead_code)]
pub fn warning_color() -> egui::Color32 {
    ThemeColors::warning()
}

#[allow(dead_code)]
pub fn info_color() -> egui::Color32 {
    ThemeColors::info()
}

/// Style for section headings
pub fn heading_style(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(18.0)
        .strong()
        .color(ThemeColors::text_primary())
}

/// Style for subheadings
pub fn subheading_style(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(14.0)
        .strong()
        .color(ThemeColors::text_secondary())
}

/// Style for muted/hint text
#[allow(dead_code)]
pub fn muted_style(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(12.0)
        .color(ThemeColors::text_muted())
}

/// Style for metric values
#[allow(dead_code)]
pub fn metric_style(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(28.0)
        .strong()
        .color(ThemeColors::text_primary())
}

/// Style for metric labels
pub fn metric_label_style(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(11.0)
        .color(ThemeColors::text_muted())
}
