use eframe::egui::{
    self,
    epaint::{Mesh, Vertex, WHITE_UV},
    Color32, CornerRadius, Rect, Response, Role, Shape, Stroke, StrokeKind, Ui, WidgetInfo,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Graphics::Dwm::*;

const TEXT: Color32 = Color32::from_rgb(244, 248, 255);
pub const MUTED: Color32 = Color32::from_rgb(170, 191, 214);

pub fn configure_window(cc: &eframe::CreationContext<'_>) {
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let window = handle.hwnd.get() as _;
    unsafe {
        let dark = 1i32;
        DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&dark as *const i32).cast(),
            4,
        );
        let caption_color = 0x0029_1e17u32;
        DwmSetWindowAttribute(
            window,
            DWMWA_CAPTION_COLOR as u32,
            (&caption_color as *const u32).cast(),
            4,
        );
        let material = DWMSBT_NONE;
        DwmSetWindowAttribute(
            window,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&material as *const i32).cast(),
            4,
        );
    }
}

pub fn style() -> egui::Style {
    let mut style = egui::Style::default();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(14, 23, 43);
    style.visuals.window_fill = Color32::from_rgb(22, 33, 55);
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(9, 16, 34, 195);
    for (widget, fill, rim) in [
        (&mut style.visuals.widgets.inactive, 21, 34),
        (&mut style.visuals.widgets.hovered, 37, 78),
        (&mut style.visuals.widgets.active, 48, 110),
        (&mut style.visuals.widgets.open, 39, 76),
    ] {
        let color = Color32::from_white_alpha(fill);
        widget.bg_fill = color;
        widget.weak_bg_fill = color;
        widget.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(rim));
        widget.corner_radius = CornerRadius::same(14);
    }
    style.visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0, Color32::from_white_alpha(30));
    style.spacing.item_spacing = egui::vec2(11.0, 12.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.button_padding = egui::vec2(11.0, 6.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
}

pub fn chip(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    let opacity = if crate::glass_backdrop::has_material() {
        166
    } else {
        222
    };
    let response = egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(14, 25, 43, opacity))
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(42)))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(12, 7))
        .show(ui, content)
        .response;
    crate::glass_backdrop::record(response.rect, ui.clip_rect(), 16);
}

pub fn surface(
    ui: &mut Ui,
    strength: f32,
    floating: bool,
    content: impl FnOnce(&mut Ui),
) -> Response {
    let painter = ui.painter().clone();
    let layer = painter.add(Shape::Noop);
    let radius = if floating { 25 } else { 23 };
    let response = egui::Frame::new()
        .inner_margin(if floating { 11 } else { 18 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            content(ui);
        })
        .response;
    let rect = response.rect;
    crate::glass_backdrop::record(rect, ui.clip_rect(), radius);
    let amount = (strength / 100.0).clamp(0.0, 1.0);
    let transmission = if crate::glass_backdrop::has_material() {
        0.62
    } else {
        1.0
    };
    let fill = |rgb: [u8; 3], opacity: f32| {
        Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], (opacity * transmission) as u8)
    };
    let (top, middle, bottom) = if floating {
        (
            fill([41, 64, 89], 195.0 + 34.0 * amount),
            fill([22, 35, 60], 178.0 + 40.0 * amount),
            fill([29, 39, 67], 178.0 + 42.0 * amount),
        )
    } else {
        (
            fill([40, 59, 83], 190.0 + 40.0 * amount),
            fill([29, 46, 72], 172.0 + 47.0 * amount),
            fill([23, 35, 63], 177.0 + 44.0 * amount),
        )
    };
    let shadow = egui::epaint::Shadow {
        offset: [0, if floating { 8 } else { 4 }],
        blur: if floating { 20 } else { 15 },
        spread: 0,
        color: Color32::from_black_alpha(if floating { 83 } else { 50 }),
    };
    let shell = Shape::rect_stroke(
        rect,
        radius,
        Stroke::new(
            1.0,
            Color32::from_white_alpha(if floating { 60 } else { 37 }),
        ),
        StrokeKind::Inside,
    );
    let rim = Shape::rect_stroke(
        rect.shrink(1.0),
        radius - 1,
        Stroke::new(
            1.0,
            Color32::from_white_alpha(if floating { 15 } else { 9 }),
        ),
        StrokeKind::Inside,
    );
    painter.set(
        layer,
        Shape::Vec(vec![
            shadow.as_shape(rect, radius).into(),
            lens(rect, f32::from(radius), top, middle, bottom),
            shell,
            rim,
            glint(rect, f32::from(radius), true),
            glint(rect, f32::from(radius), false),
        ]),
    );
    response
}

fn glint(rect: Rect, radius: f32, top_left: bool) -> Shape {
    let r = (radius - 1.5)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);
    let (center, sign) = if top_left {
        (rect.left_top() + egui::vec2(radius, radius), 1.0)
    } else {
        (rect.right_bottom() - egui::vec2(radius, radius), -1.0)
    };
    let mut points = (0..=10)
        .map(|i| {
            let angle = std::f32::consts::PI + i as f32 * std::f32::consts::FRAC_PI_2 / 10.0;
            center + sign * r * egui::vec2(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    points.push(center + sign * egui::vec2(rect.width().min(270.0) * 0.28, -r));
    Shape::line(
        points,
        Stroke::new(
            1.0,
            Color32::from_white_alpha(if top_left { 76 } else { 45 }),
        ),
    )
}

fn lens(rect: Rect, radius: f32, top: Color32, middle: Color32, bottom: Color32) -> Shape {
    let radius = radius
        .min(rect.height() / 2.0)
        .min(rect.width() / 2.0)
        .max(1.0);
    let mut mesh = Mesh::default();
    // Horizontal strips interpolate the tint across the rounded surface.
    for i in 0..=24 {
        let y = if i <= 10 {
            rect.top() + radius * i as f32 / 10.0
        } else if i <= 14 {
            egui::lerp(
                (rect.top() + radius)..=(rect.bottom() - radius),
                (i - 10) as f32 / 4.0,
            )
        } else {
            rect.bottom() - radius * (24 - i) as f32 / 10.0
        };
        let v = (y - rect.top()) / rect.height().max(1.0);
        let d = (radius - (y - rect.top()).min(rect.bottom() - y)).max(0.0);
        let inset = radius - (radius * radius - d * d).max(0.0).sqrt();
        let color = if v <= 0.42 {
            top.lerp_to_gamma(middle, v / 0.42)
        } else {
            middle.lerp_to_gamma(bottom, (v - 0.42) / 0.58)
        };
        for x in [rect.left() + inset, rect.right() - inset] {
            mesh.vertices.push(Vertex {
                pos: egui::pos2(x, y),
                uv: WHITE_UV,
                color,
            });
        }
        if i > 0 {
            let k = i * 2;
            mesh.indices.extend([k - 2, k - 1, k, k, k - 1, k + 1]);
        }
    }
    Shape::mesh(mesh)
}

pub fn toggle(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let width = ui.available_width();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout(label.into(), font, TEXT, (width - 62.0).max(40.0));
    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(width, (galley.size().y + 12.0).max(34.0)),
        egui::Sense::click(),
    );
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, ui.is_enabled(), *checked, label));
    if ui.is_rect_visible(rect) {
        let paint = ui.painter();
        if response.hovered() || response.has_focus() {
            paint.rect_filled(rect.expand(4.0), 12, Color32::from_white_alpha(12));
        }
        let text = egui::pos2(rect.left() + 2.0, rect.center().y - galley.size().y / 2.0);
        paint.galley(text, galley, ui.visuals().text_color());
        let track = Rect::from_center_size(
            egui::pos2(rect.right() - 23.0, rect.center().y),
            egui::vec2(46.0, 26.0),
        );
        let t = ui
            .ctx()
            .animate_bool_with_time(response.id, *checked, ui.style().animation_time);
        let accent = ui.visuals().selection.stroke.color;
        let tint = Color32::from_white_alpha(27).lerp_to_gamma(accent.gamma_multiply(0.57), t);
        paint.rect_filled(track, 13, tint);
        paint.rect_stroke(
            track,
            13,
            Stroke::new(
                1.0,
                Color32::from_white_alpha(if response.has_focus() { 176 } else { 65 }),
            ),
            StrokeKind::Inside,
        );
        let knob = egui::pos2(
            egui::lerp((track.left() + 13.0)..=(track.right() - 13.0), t),
            track.center().y,
        );
        paint.circle_filled(
            knob + egui::vec2(0.0, 1.5),
            10.0,
            Color32::from_black_alpha(65),
        );
        paint.circle_filled(knob, 9.5, Color32::from_rgb(236, 248, 255));
        paint.circle_stroke(knob, 9.5, Stroke::new(1.0, Color32::WHITE));
    }
    response
}
