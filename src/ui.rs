use crate::{
    glass::{self, MUTED},
    glass_backdrop::{self, Backdrop},
    icons::{self, Icon, Icons},
    overlay::{Action, Diagnostics, Overlay},
    settings::{self, Settings},
    snowcap::SnowCap,
    weather::{self, Observation},
};
use eframe::egui::{self, Color32, RichText, Ui, ViewportCommand};
use std::{
    path::PathBuf,
    ptr::null_mut,
    sync::mpsc,
    time::{Duration, Instant},
};
use windows_sys::{
    core::w,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::*,
        UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION},
    },
};

const GREEN: Color32 = Color32::from_rgb(116, 215, 167);
const WARN: Color32 = Color32::from_rgb(227, 183, 119);
const ACCENTS: [Color32; 4] = [
    Color32::from_rgb(141, 217, 246),
    Color32::from_rgb(166, 177, 255),
    Color32::from_rgb(116, 214, 191),
    Color32::from_rgb(214, 182, 242),
];
const NAV: [(&str, Icon); 6] = [
    ("Главная", Icon::House),
    ("Эффекты", Icon::Snowflake),
    ("Погода", Icon::CloudSnow),
    ("Производительность", Icon::Gauge),
    ("Оформление", Icon::Palette),
    ("Настройки", Icon::Settings),
];

type WeatherReply = (String, Result<Observation, String>);

pub struct App {
    icons: Icons,
    backdrop: Option<Backdrop>,
    overlay: Overlay,
    settings: Settings,
    page: usize,
    paused: bool,
    hide_on_start: bool,
    hidden: bool,
    exiting: bool,
    autostart: bool,
    system_animations: bool,
    saving_at: Option<Instant>,
    notice: String,
    city_input: String,
    weather_rx: Option<mpsc::Receiver<WeatherReply>>,
    observation: Option<Observation>,
    weather_error: Option<String>,
    next_weather: Instant,
    cap_preview: Option<(SnowCap, egui::TextureHandle)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: bool) -> Result<Self, std::io::Error> {
        let settings = settings::load();
        let observation = settings
            .real_weather
            .then(|| weather::load_cache(&settings.city))
            .flatten();
        let overlay = Overlay::start(settings.clone(), observation.clone(), cc.egui_ctx.clone())
            .map_err(std::io::Error::other)?;
        let system = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let mut fonts = egui::FontDefinitions::default();
        for (name, file) in [
            ("Segoe UI Symbol", "seguisym.ttf"),
            ("Segoe UI", "segoeui.ttf"),
        ] {
            if let Ok(bytes) = std::fs::read(system.join("Fonts").join(file)) {
                fonts
                    .font_data
                    .insert(name.into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, name.into());
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_zoom_factor(settings.zoom);
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.set_style_of(egui::Theme::Dark, glass::style());
        glass::configure_window(cc);
        Ok(Self {
            icons: Icons::load(&cc.egui_ctx),
            backdrop: Backdrop::new(cc),
            hide_on_start: startup && overlay.tray_ready,
            hidden: false,
            exiting: false,
            city_input: settings.city.clone(),
            settings,
            overlay,
            page: 0,
            paused: false,
            autostart: autostart_enabled(),
            system_animations: system_animations(),
            saving_at: None,
            notice: String::new(),
            weather_rx: None,
            observation,
            weather_error: None,
            next_weather: Instant::now(),
            cap_preview: None,
        })
    }

    fn save(&mut self) {
        self.saving_at = None;
        if let Err(error) = settings::save(&self.settings) {
            self.notice = "Не удалось сохранить настройки.".into();
            settings::log(&format!("Settings: {error}"));
        }
    }

    fn hide(&mut self, ctx: &egui::Context) {
        self.hidden = true;
        if let Some(backdrop) = &self.backdrop {
            backdrop.hide();
        }
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }

    fn actions(&mut self, ctx: &egui::Context) {
        while let Ok(action) = self.overlay.actions.try_recv() {
            match action {
                Action::Open => {
                    self.hidden = false;
                    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                Action::Pause => self.paused = !self.paused,
                Action::Falling => self.settings.falling = !self.settings.falling,
                Action::Caps => self.settings.caps = !self.settings.caps,
                Action::Weather => self.settings.real_weather = !self.settings.real_weather,
                Action::Exit => self.exiting = true,
            }
        }
    }

    fn fetch_if_due(&mut self, ctx: &egui::Context, now: Instant) {
        if let Some(rx) = &self.weather_rx {
            match rx.try_recv() {
                Ok((city, result)) => {
                    self.weather_rx = None;
                    if city == self.settings.city && self.settings.real_weather {
                        match result {
                            Ok(observation) => {
                                if let Err(error) = weather::save_cache(&city, &observation) {
                                    settings::log(&format!("Weather cache: {error}"));
                                }
                                self.observation = Some(observation);
                                self.weather_error = None;
                            }
                            Err(error) => {
                                self.weather_error = Some(error);
                            }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.weather_rx = None;
                    self.weather_error = Some("Обновление прервалось.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.settings.real_weather
            && !self.settings.city.is_empty()
            && self.weather_rx.is_none()
            && now >= self.next_weather
        {
            let city = self.settings.city.clone();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.weather_rx = Some(rx);
            self.next_weather = now + weather::REFRESH;
            std::thread::spawn(move || {
                let result = weather::fetch(&city);
                let _ = tx.send((city, result));
                ctx.request_repaint();
            });
        }
        if self.settings.real_weather && self.weather_rx.is_none() && !self.settings.city.is_empty()
        {
            ctx.request_repaint_after(
                self.next_weather
                    .saturating_duration_since(now)
                    .max(Duration::from_secs(1)),
            );
        }
    }

    fn home(&mut self, ui: &mut Ui, diag: &Diagnostics) {
        ui.horizontal(|ui| {
            glass::chip(ui, |ui| {
                ui.heading(RichText::new("Зимняя атмосфера").size(30.0).strong());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (icon, label) = if self.paused {
                    (Icon::Play, "Продолжить")
                } else {
                    (Icon::Pause, "Пауза")
                };
                if ui
                    .add(
                        self.icons
                            .button(icon, label, ACCENTS[self.settings.accent])
                            .fill(Color32::from_rgba_unmultiplied(21, 38, 63, 225)),
                    )
                    .clicked()
                {
                    self.paused = !self.paused;
                }
            });
        });
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(if self.settings.real_weather {
                            "РЕАЛЬНАЯ ПОГОДА"
                        } else {
                            "РУЧНОЙ РЕЖИМ"
                        })
                        .color(ACCENTS[self.settings.accent])
                        .size(12.0)
                        .strong(),
                    );
                    let c = weather::conditions(
                        &self.settings,
                        self.observation.as_ref(),
                        Instant::now(),
                    );
                    let has_weather = !self.settings.real_weather || self.observation.is_some();
                    let title = match (&self.observation, self.settings.real_weather) {
                        (Some(obs), true) => obs.city.as_str(),
                        (None, true) => "Погода не загружена",
                        (_, false) => "Снег по вашим настройкам",
                    };
                    ui.label(RichText::new(title).size(19.0).strong());
                    if has_weather {
                        let t = self
                            .observation
                            .as_ref()
                            .filter(|_| self.settings.real_weather)
                            .map_or(c.temperature, |obs| obs.current.temp_c);
                        ui.label(RichText::new(format!("{t:+.1} °C")).size(45.0).strong());
                    }
                    if let Some(text) = &self.weather_error {
                        ui.label(RichText::new(text).color(WARN));
                    }
                    ui.label(RichText::new(diag.reason).color(MUTED));
                });
                if ui.available_width() >= 115.0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let c = weather::conditions(
                            &self.settings,
                            self.observation.as_ref(),
                            Instant::now(),
                        );
                        scene(
                            ui,
                            !self.paused && self.settings.falling && c.snow > 0.0,
                            !self.paused && self.settings.caps && c.snow > 0.0,
                            &self.settings,
                            &mut self.cap_preview,
                        );
                    });
                }
            });
        });
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Быстрое управление");
            ui.spacing_mut().item_spacing.y = 4.0;
            glass::toggle(ui, &mut self.settings.falling, "Снегопад");
            glass::toggle(ui, &mut self.settings.caps, "Снежные шапки на окнах");
            glass::toggle(
                ui,
                &mut self.settings.real_weather,
                "Синхронизация с погодой",
            );
        });
    }

    fn effects(&mut self, ui: &mut Ui) {
        heading(
            ui,
            "Эффекты",
            "Снегопад и накопление на окнах настраиваются отдельно",
        );
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Пресеты");
            ui.horizontal_wrapped(|ui| {
                for (title, intensity, size, speed) in [
                    ("Лёгкий снег", 20.0, 2.0, 38.0),
                    ("Обычная зима", 45.0, 3.0, 65.0),
                    ("Метель", 90.0, 4.5, 125.0),
                ] {
                    if ui.button(title).clicked() {
                        self.settings.intensity = intensity;
                        self.settings.size = size;
                        self.settings.speed = speed;
                    }
                }
            });
        });
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Снегопад");
            glass::toggle(ui, &mut self.settings.falling, "Падающий снег");
            ui.add(
                egui::Slider::new(&mut self.settings.intensity, 0.0..=100.0)
                    .suffix("%")
                    .text("Интенсивность"),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.size, 1.0..=8.0)
                    .suffix(" px")
                    .text("Размер"),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.opacity, 10.0..=100.0)
                    .suffix("%")
                    .text("Непрозрачность"),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.speed, 10.0..=170.0)
                    .suffix(" px/с")
                    .text("Скорость"),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.particle_limit, 50..=2000)
                    .text("Лимит частиц на дисплей"),
            );
        });
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Снежные шапки");
            glass::toggle(ui, &mut self.settings.caps, "Накопление на окнах");
            glass::toggle(
                ui,
                &mut self.settings.bottom_snow,
                "Снег у нижнего края экрана",
            );
            ui.add(
                egui::Slider::new(&mut self.settings.max_height, 5.0..=50.0)
                    .suffix(" px")
                    .text("Максимальная высота"),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.roughness, 0.0..=100.0)
                    .suffix("%")
                    .text("Неровность края"),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.accumulation, 0.1..=3.0)
                    .suffix("×")
                    .text("Накопление"),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.melt, 0.0..=3.0)
                    .suffix("×")
                    .text("Таяние"),
            );
            glass::toggle(ui, &mut self.settings.icicles, "Сосульки");
            glass::toggle(
                ui,
                &mut self.settings.shake_to_clear,
                "Стряхивать снег при встряхивании окна",
            )
            .on_hover_text(
                "Быстро потяните окно за заголовок влево, вправо и снова влево или наоборот.",
            );
            ui.separator();
            ui.heading("Снежные шары и снеговики");
            glass::toggle(
                ui,
                &mut self.settings.snowballs,
                "Лепить снежные шары мышью",
            );
            ui.label(
                RichText::new(
                    "Удерживайте Shift и левую кнопку мыши над снежной шапкой, ведите                      вдоль окна — снег скатается в шар. Не отпуская кнопку, перенесите                      шар в нижнюю часть экрана. Слепите меньший шар и положите его                      на первый; третий станет головой снеговика. Готовые шары                      также перемещаются с Shift + ЛКМ.",
                )
                .color(MUTED),
            );
        });
    }

    fn weather(&mut self, ui: &mut Ui) {
        heading(ui, "Погода", "Условия для снегопада и накопления");
        glass::chip(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.settings.real_weather, true, "Реальная погода");
                ui.selectable_value(&mut self.settings.real_weather, false, "Ручной режим");
            });
        });
        if self.settings.real_weather {
            glass::surface(ui, self.settings.glass, false, |ui| {
                ui.heading("Местоположение");
                ui.horizontal_wrapped(|ui| {
                    let city = ui.add(
                        egui::TextEdit::singleline(&mut self.city_input)
                            .hint_text("Например, Борисоглебск")
                            .char_limit(100),
                    );
                    let enter =
                        city.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let can_refresh =
                        self.weather_rx.is_none() && self.city_input.trim().chars().count() >= 2;
                    let refresh = ui.add_enabled(
                        can_refresh,
                        self.icons
                            .button(Icon::Refresh, "Обновить", ACCENTS[self.settings.accent]),
                    );
                    if refresh.clicked() || can_refresh && enter {
                        self.settings.city = self.city_input.trim().into();
                        self.next_weather = Instant::now();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.hyperlink_to("WeatherAPI", "https://www.weatherapi.com/");
                    ui.label(RichText::new("Автообновление раз в час").color(MUTED));
                });
                if self.weather_rx.is_some() {
                    ui.label("Загрузка погоды…");
                }
                if let Some(error) = &self.weather_error {
                    ui.label(RichText::new(error).color(WARN));
                }
                if let Some(obs) = &self.observation {
                    let stale =
                        obs.cached || self.weather_error.is_some() || obs.age() >= weather::REFRESH;
                    ui.separator();
                    ui.heading(&obs.city);
                    ui.label(&obs.current.condition.text);
                    ui.label(format!(
                        "{:+.1} °C     ·     Ветер {:.1} км/ч     ·     Осадки {:.1} мм",
                        obs.current.temp_c, obs.current.wind_kph, obs.current.precip_mm
                    ));
                    let mins = weather::age_minutes(obs);
                    let utc = obs.current.last_updated_epoch.rem_euclid(86400) / 60;
                    ui.label(
                        RichText::new(format!(
                            "{} · {:02}:{:02} UTC · {} мин назад",
                            if stale {
                                "Последние сохранённые данные"
                            } else {
                                "Текущие данные"
                            },
                            utc / 60,
                            utc % 60,
                            mins
                        ))
                        .color(if stale { WARN } else { MUTED }),
                    );
                    if obs.age() >= weather::MAX_AGE {
                        ui.label(
                            RichText::new("Данные устарели. Снегопад по погоде остановлен.")
                                .color(WARN),
                        );
                    }
                }
            });
        } else {
            glass::surface(ui, self.settings.glass, false, |ui| {
                ui.heading("Ручная погода");
                ui.add(
                    egui::Slider::new(&mut self.settings.manual_snow, 0.0..=100.0)
                        .suffix("%")
                        .text("Снег"),
                );
                ui.add(
                    egui::Slider::new(&mut self.settings.manual_wind, -100.0..=100.0)
                        .suffix("%")
                        .text("Боковой ветер"),
                );
                ui.add(
                    egui::Slider::new(&mut self.settings.manual_temp, -25.0..=25.0)
                        .suffix(" °C")
                        .text("Температура"),
                );
            });
        }
    }

    fn performance(&mut self, ui: &mut Ui, diag: &Diagnostics) {
        heading(
            ui,
            "Производительность",
            "Отрисовка активна, пока эффекты видны",
        );
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Автоматическая пауза");
            glass::toggle(
                ui,
                &mut self.settings.fullscreen_pause,
                "Приостанавливать эффекты на полноэкранном дисплее",
            );
            ui.horizontal_wrapped(|ui| {
                ui.label("Лимит кадров");
                for fps in [15, 30, 60, 120] {
                    ui.selectable_value(&mut self.settings.fps, fps, format!("{fps} FPS"));
                }
            });
        });
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Диагностика");
            ui.label(diag.reason);
            ui.separator();
            ui.label(format!("Отрисовка: {} FPS", diag.fps));
            ui.label(format!("Окон отслеживается: {}", diag.tracked));
            ui.label(format!("Видимых снежных шапок: {}", diag.caps));
            ui.label(format!(
                "Дисплеев со снежной кромкой: {}",
                diag.bottom_edges
            ));
            ui.label(format!("Частиц на дисплеях: {}", diag.particles));
            ui.label(format!("Снежных шаров: {}", diag.snowballs));
            ui.label(format!("Собрано снеговиков: {}", diag.snowmen));
        });
    }

    fn appearance(&mut self, ui: &mut Ui) {
        heading(ui, "Оформление", "Внешний вид панели управления");
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Frosted Glass");
            ui.label("Акцент");
            ui.horizontal_wrapped(|ui| {
                for (i, name) in ["Ледяной", "Лавандовый", "Мятный", "Серебристый"]
                    .iter()
                    .enumerate()
                {
                    let text = RichText::new(*name).color(ACCENTS[i]);
                    ui.selectable_value(&mut self.settings.accent, i, text);
                }
            });
            ui.add(
                egui::Slider::new(&mut self.settings.glass, 0.0..=100.0)
                    .suffix("%")
                    .text("Выраженность стекла"),
            );
            egui::ComboBox::from_label("Масштаб интерфейса")
                .selected_text(format!("{}%", (self.settings.zoom * 100.0).round()))
                .show_ui(ui, |ui| {
                    for (label, zoom) in
                        [("85%", 0.85), ("100%", 1.0), ("115%", 1.15), ("130%", 1.3)]
                    {
                        ui.selectable_value(&mut self.settings.zoom, zoom, label);
                    }
                });
            glass::toggle(ui, &mut self.settings.animations, "Анимации интерфейса");
        });
    }

    fn system(&mut self, ui: &mut Ui, diag: &Diagnostics) {
        heading(ui, "Настройки", "Запуск, системный трей и мониторы");
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Запуск и завершение");
            let mut enabled = self.autostart;
            if glass::toggle(ui, &mut enabled, "Автозапуск с Windows").changed() {
                match set_autostart(enabled) {
                    Ok(()) => self.autostart = enabled,
                    Err(e) => {
                        self.notice = "Не удалось изменить автозапуск.".into();
                        settings::log(&format!("Autostart: {e}"));
                    }
                }
            }
            ui.add_enabled_ui(self.overlay.tray_ready, |ui| {
                glass::toggle(
                    ui,
                    &mut self.settings.close_to_tray,
                    "Закрывать панель в системный трей",
                );
            });
            if self.overlay.tray_ready {
                ui.label(
                    RichText::new("Открыть панель можно двойным щелчком по значку в трее.")
                        .color(MUTED),
                );
            }
            if ui
                .add(
                    self.icons
                        .button(Icon::Exit, "Полностью выйти", Color32::LIGHT_RED),
                )
                .clicked()
            {
                self.exiting = true;
            }
        });
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Дисплеи");
            let label = diag
                .monitors
                .iter()
                .find(|(id, _)| id == &self.settings.monitor)
                .map_or("Все дисплеи", |(_, label)| label);
            egui::ComboBox::from_id_salt("monitor")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.settings.monitor, String::new(), "Все дисплеи");
                    for (id, label) in &diag.monitors {
                        ui.selectable_value(&mut self.settings.monitor, id.clone(), label);
                    }
                });
        });
        glass::surface(ui, self.settings.glass, false, |ui| {
            ui.heading("Snowy New Year");
            ui.label(format!(
                "Версия {} · Русский · Windows",
                env!("CARGO_PKG_VERSION")
            ));
            ui.collapsing("Лицензия иконок Lucide", |ui| {
                ui.label(icons::LICENSE);
            });
        });
    }
}

impl eframe::App for App {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let previous = self.settings.clone();
        let previous_pause = self.paused;
        self.actions(ctx);
        if self.hide_on_start {
            self.hide_on_start = false;
            self.hide(ctx);
        }
        let requested_close = ctx.input(|input| input.viewport().close_requested());
        if requested_close
            && !self.exiting
            && self.overlay.tray_ready
            && self.settings.close_to_tray
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide(ctx);
        }
        self.sync(ctx, previous, previous_pause);
    }

    fn ui(&mut self, root: &mut Ui, _: &mut eframe::Frame) {
        if self.hidden || self.exiting {
            return;
        }
        let ctx = root.ctx().clone();
        glass_backdrop::begin_frame(self.backdrop.is_some());
        let previous = self.settings.clone();
        let previous_pause = self.paused;
        let diag = self.overlay.shared.lock().unwrap().diagnostics.clone();
        let accent = ACCENTS[self.settings.accent];
        {
            let style = root.style_mut();
            style.visuals.selection.bg_fill = accent.gamma_multiply(0.25);
            style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, accent);
            style.visuals.widgets.active.fg_stroke.color = accent;
            style.animation_time = if self.settings.animations && self.system_animations {
                0.14
            } else {
                0.0
            };
        }
        let compact = root.available_width() < 710.0;
        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(if compact { 77.0 } else { 246.0 })
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 13,
                right: 6,
                top: 13,
                bottom: 13,
            }))
            .show(root, |ui| {
                glass::surface(ui, self.settings.glass, true, |ui| {
                    ui.set_min_height(ui.available_height());
                    self.navigation(ui, compact, accent, &diag);
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 13,
                right: 23,
                top: 22,
                bottom: 16,
            }))
            .show(root, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(self.page)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match self.page {
                            0 => self.home(ui, &diag),
                            1 => self.effects(ui),
                            2 => self.weather(ui),
                            3 => self.performance(ui, &diag),
                            4 => self.appearance(ui),
                            _ => self.system(ui, &diag),
                        }
                        if !self.notice.is_empty() {
                            ui.label(RichText::new(&self.notice).color(WARN));
                        }
                    });
            });
        ctx.request_repaint_after(Duration::from_secs(1));
        if let Some(backdrop) = &mut self.backdrop {
            backdrop.update(&ctx);
        }
        self.sync(&ctx, previous, previous_pause);
    }
}

impl App {
    fn navigation(&mut self, ui: &mut Ui, compact: bool, accent: Color32, diag: &Diagnostics) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(
                self.icons
                    .image(Icon::Snowflake, 19.0, accent)
                    .alt_text("Snowy New Year"),
            );
            if !compact {
                ui.label(
                    RichText::new("Snowy New Year")
                        .color(accent)
                        .size(17.0)
                        .strong(),
                );
            }
        });
        ui.add_space(12.0);
        ui.spacing_mut().item_spacing.y = 5.0;
        for (index, &(name, icon)) in NAV.iter().enumerate() {
            let selected = index == self.page;
            let tint = if selected { accent } else { MUTED };
            let button = if compact {
                let image = self.icons.image(icon, 17.0, tint).alt_text(name);
                egui::Button::new(image).image_tint_follows_text_color(false)
            } else {
                self.icons.button(icon, name, tint)
            }
            .wrap_mode(egui::TextWrapMode::Truncate)
            .selected(selected)
            .frame_when_inactive(selected)
            .min_size(egui::vec2(ui.available_width(), 36.0));
            if ui.add(button).on_hover_text(name).clicked() {
                self.page = index;
            }
        }
        if !compact {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                let error = self.weather_error.is_some() && self.settings.real_weather;
                let text = if error {
                    "Ошибка погоды"
                } else {
                    diag.reason
                };
                let color = if error || self.paused { WARN } else { GREEN };
                egui::Frame::new()
                    .fill(Color32::from_white_alpha(14))
                    .corner_radius(13)
                    .inner_margin(egui::Margin::symmetric(9, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (dot, _) = ui
                                .allocate_exact_size(egui::Vec2::splat(7.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot.center(), 3.0, color);
                            ui.label(RichText::new(text).color(color).size(12.0));
                        });
                    });
            });
        }
    }

    fn sync(&mut self, ctx: &egui::Context, previous: Settings, previous_pause: bool) {
        let now = Instant::now();
        if previous.city != self.settings.city
            || previous.real_weather != self.settings.real_weather
        {
            self.observation = self
                .settings
                .real_weather
                .then(|| weather::load_cache(&self.settings.city))
                .flatten();
            self.weather_error = None;
            self.next_weather = now;
        }
        let previous_observation = self.observation.as_ref().map(|obs| obs.received);
        let previous_error = self.weather_error.is_some();
        self.fetch_if_due(ctx, now);
        if previous != self.settings {
            self.saving_at = Some(now + Duration::from_millis(400));
            ctx.request_repaint_after(Duration::from_millis(400));
            if previous.zoom != self.settings.zoom {
                ctx.set_zoom_factor(self.settings.zoom);
            }
        }
        if previous != self.settings
            || previous_pause != self.paused
            || previous_observation != self.observation.as_ref().map(|obs| obs.received)
            || previous_error != self.weather_error.is_some()
        {
            {
                let mut shared = self.overlay.shared.lock().unwrap();
                shared.settings = self.settings.clone();
                shared.paused = self.paused;
                shared.observation = self.observation.clone();
                shared.weather_error = self.weather_error.is_some() && self.settings.real_weather;
            }
            self.overlay.wake();
        }
        if self.saving_at.is_some_and(|deadline| now >= deadline) {
            self.save();
        }
        if self.exiting {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if self.saving_at.is_some() {
            self.save();
        }
    }
}

fn heading(ui: &mut Ui, title: &str, subtitle: &str) {
    glass::chip(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.heading(RichText::new(title).size(28.0).strong());
        ui.label(RichText::new(subtitle).color(MUTED));
    });
}

fn scene(
    ui: &mut Ui,
    snow: bool,
    caps: bool,
    settings: &Settings,
    preview: &mut Option<(SnowCap, egui::TextureHandle)>,
) {
    let accent = settings.accent;
    let width = ui.available_width().min(250.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(width, 132.0), egui::Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 18, Color32::from_rgba_unmultiplied(8, 22, 45, 176));
    p.rect_stroke(
        r,
        18,
        egui::Stroke::new(1.0, Color32::from_white_alpha(36)),
        egui::StrokeKind::Inside,
    );
    let city = egui::Rect::from_center_size(
        r.center() + egui::vec2(0.0, 12.0),
        egui::vec2(width * 0.66, 55.0),
    );
    p.rect_filled(city, 5, Color32::from_rgb(31, 58, 82));
    let edge = egui::Stroke::new(1.0_f32, Color32::from_rgb(77, 114, 146));
    p.rect_stroke(city, 5, edge, egui::StrokeKind::Inside);
    p.line_segment(
        [
            city.left_top() + egui::vec2(0.0, 12.0),
            city.right_top() + egui::vec2(0.0, 12.0),
        ],
        edge,
    );
    for i in 0..3 {
        p.circle_filled(
            city.left_top() + egui::vec2(8.0 + 7.0 * i as f32, 6.0),
            1.5,
            ACCENTS[accent],
        );
    }
    if caps {
        let shape = SnowCap {
            width: city.width().round() as i32,
            depth: (settings.max_height * 0.85).min(30.0),
            roughness: settings.roughness / 100.0,
            seed: 0x75c2_ac73,
            icicles: settings.icicles,
        };
        let rows = shape.rows();
        let (cached_shape, texture) = preview.get_or_insert_with(|| {
            (
                shape,
                ui.ctx()
                    .load_texture("snow cap", cap_image(shape), egui::TextureOptions::LINEAR),
            )
        });
        if *cached_shape != shape {
            texture.set(cap_image(shape), egui::TextureOptions::LINEAR);
            *cached_shape = shape;
        }
        let image = egui::Rect::from_min_size(
            city.left_top() + egui::vec2(0.0, rows.start as f32),
            egui::vec2(city.width(), rows.len() as f32),
        );
        p.image(
            texture.id(),
            image,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if snow {
        for i in 0..29 {
            let x = ((i * 173 + 41) % 241) as f32 / 241.0;
            let y = ((i * 59 + 9) % 113) as f32 / 113.0;
            p.circle_filled(
                r.left_top() + egui::vec2(4.0 + x * (width - 8.0), 4.0 + y * (r.height() - 8.0)),
                0.7 + (i % 3) as f32 * 0.55,
                ACCENTS[accent].gamma_multiply(0.6),
            );
        }
    }
}

fn cap_image(shape: SnowCap) -> egui::ColorImage {
    let rows = shape.rows();
    let width = shape.width as usize;
    let mut pixels = vec![Color32::TRANSPARENT; width * rows.len()];
    shape.rasterize(0..shape.width, rows.clone(), |x, y, argb| {
        let [a, r, g, b] = argb.to_be_bytes();
        pixels[(y - rows.start) as usize * width + x as usize] =
            Color32::from_rgba_premultiplied(r, g, b, a);
    });
    egui::ColorImage::new([width, rows.len()], pixels)
}

const RUN_KEY: windows_sys::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: windows_sys::core::PCWSTR = w!("Snowy New Year");

fn system_animations() -> bool {
    let mut value = 1i32;
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut value as *mut i32).cast(),
            0,
        );
    }
    value != 0
}

fn autostart_enabled() -> bool {
    unsafe {
        let mut size = 0;
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_NAME,
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut size,
        ) == ERROR_SUCCESS
    }
}

fn set_autostart(enabled: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let command = if enabled {
        let path = std::env::current_exe()?;
        let quote = u16::from(b'"');
        Some(
            std::iter::once(quote)
                .chain(path.as_os_str().encode_wide())
                .chain("\" --tray\0".encode_utf16())
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    unsafe {
        let mut key = null_mut();
        let code = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut key);
        if code != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(code as i32));
        }
        let result = match command {
            Some(command) => RegSetValueExW(
                key,
                RUN_NAME,
                0,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            ),
            None => RegDeleteValueW(key, RUN_NAME),
        };
        RegCloseKey(key);
        if result != ERROR_SUCCESS && (enabled || result != ERROR_FILE_NOT_FOUND) {
            return Err(std::io::Error::from_raw_os_error(result as i32));
        }
    }
    Ok(())
}
