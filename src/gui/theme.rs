use eframe::egui;

pub fn setup_custom_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Customize spacing
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);

    // Customize visuals
    style.visuals.window_rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(4.0);

    ctx.set_style(style);
}

pub fn status_color(good: bool) -> egui::Color32 {
    if good {
        egui::Color32::from_rgb(100, 200, 100)
    } else {
        egui::Color32::from_rgb(200, 100, 100)
    }
}

pub fn warning_color() -> egui::Color32 {
    egui::Color32::from_rgb(200, 180, 100)
}

pub fn info_color() -> egui::Color32 {
    egui::Color32::from_rgb(100, 150, 200)
}
