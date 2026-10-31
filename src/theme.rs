//! Native egui translation of ShareOS shareos.css (.sos.acc-blue / .light.acc-blue).
use eframe::egui::{self, Color32, FontFamily, FontId, Stroke, TextStyle};
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub hover: Color32,
    pub line: Color32,
    pub accent: Color32,
    pub dim: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub error: Color32,
}
pub fn palette(dark: bool) -> Palette {
    let c = |rgb: u32| Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    if dark {
        Palette {
            bg: c(0x06090d),
            panel: c(0x0b111a),
            hover: c(0x0e1726),
            line: c(0x15253b),
            accent: c(0x3da5ff),
            dim: c(0x1d4f80),
            text: c(0xc6dff5),
            muted: c(0x6f90ad),
            error: c(0xffb3b3),
        }
    } else {
        Palette {
            bg: c(0xf3f6f9),
            panel: Color32::WHITE,
            hover: c(0xeef4fb),
            line: c(0xd6e1ee),
            accent: c(0x1f6fd1),
            dim: c(0xa9c7ee),
            text: c(0x111d2b),
            muted: c(0x53637a),
            error: c(0xa3262f),
        }
    }
}
pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = palette(dark);
    let mut style = (*ctx.style()).clone();
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = Some(p.text);
    v.panel_fill = p.bg;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.hover;
    v.window_stroke = Stroke::new(1.0_f32, p.line);
    v.window_corner_radius = egui::CornerRadius::same(12);
    v.hyperlink_color = p.accent;
    v.error_fg_color = p.error;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0_f32, p.bg);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_fill = p.panel;
        w.weak_bg_fill = p.panel;
        w.bg_stroke = Stroke::new(1.0_f32, p.line);
        w.fg_stroke = Stroke::new(1.0_f32, p.accent);
        w.corner_radius = egui::CornerRadius::same(8);
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.dim);
    v.widgets.active.bg_fill = p.hover;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(12.0, 14.0);
    style.spacing.button_padding = egui::vec2(16.0, 10.0);
    style.spacing.interact_size.y = 40.0;
    style.spacing.text_edit_width = 380.0;
    for (text, size) in [
        (TextStyle::Body, 14.0),
        (TextStyle::Button, 13.0),
        (TextStyle::Small, 12.0),
        (TextStyle::Heading, 22.0),
        (TextStyle::Monospace, 13.0),
    ] {
        style
            .text_styles
            .insert(text, FontId::new(size, FontFamily::Monospace));
    }
    ctx.set_style(style);
}
