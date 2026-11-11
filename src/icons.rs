use eframe::egui::{self, Color32, TextureHandle};

const SIZE: usize = 48;
pub const BADGE_SIZE: u32 = 32;
pub const LICENSE: &str = include_str!("../assets/lucide/LICENSE");

macro_rules! icon_masks {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub enum Icon { $($variant),+ }

        const MASKS: &[(&str, &[u8; SIZE * SIZE])] = &[
            $(($name, include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".alpha")))),+
        ];
    };
}

icon_masks! {
    House => "house",
    Snowflake => "snowflake",
    CloudSnow => "cloud-snow",
    Gauge => "gauge",
    Palette => "palette",
    Settings => "settings",
    Pause => "pause",
    Play => "play",
    Refresh => "refresh-cw",
    Exit => "log-out",
}

pub struct Icons(Vec<TextureHandle>);

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        Self(
            MASKS
                .iter()
                .map(|(name, mask)| {
                    let pixels = mask
                        .iter()
                        .map(|&alpha| Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha))
                        .collect();
                    ctx.load_texture(
                        format!("lucide/{name}"),
                        egui::ColorImage::new([SIZE, SIZE], pixels),
                        egui::TextureOptions::LINEAR,
                    )
                })
                .collect(),
        )
    }

    pub fn image(&self, icon: Icon, size: f32, tint: Color32) -> egui::Image<'static> {
        egui::Image::new((self.0[icon as usize].id(), egui::Vec2::splat(size))).tint(tint)
    }

    pub fn button(&self, icon: Icon, label: &str, tint: Color32) -> egui::Button<'static> {
        egui::Button::new((self.image(icon, 16.0, tint), label.to_owned()))
            .image_tint_follows_text_color(false)
    }
}

pub fn tray_badge(state: usize) -> &'static [u8; (BADGE_SIZE * BADGE_SIZE * 4) as usize] {
    const BADGES: [&[u8; (BADGE_SIZE * BADGE_SIZE * 4) as usize]; 3] = [
        include_bytes!(concat!(env!("OUT_DIR"), "/active.rgba")),
        include_bytes!(concat!(env!("OUT_DIR"), "/paused.rgba")),
        include_bytes!(concat!(env!("OUT_DIR"), "/offline.rgba")),
    ];
    BADGES[state]
}

pub fn window_icon() -> egui::IconData {
    let mut rgba = tray_badge(0).to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha != 0 {
            for channel in &mut pixel[..3] {
                *channel = (u16::from(*channel) * 255 / alpha) as u8;
            }
        }
    }
    egui::IconData {
        rgba,
        width: BADGE_SIZE,
        height: BADGE_SIZE,
    }
}
