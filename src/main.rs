// src/main.rs complet

#![cfg_attr(not(windows), allow(unused))]
#[cfg(not(windows))]
compile_error!("Binaire Windows uniquement.");

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::{egui, NativeOptions};
use egui::{Color32, FontFamily, FontId, Margin, RichText, TextStyle, Vec2};
use rdev::{Event, EventType, Key};
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows::core::w;
#[cfg(windows)]
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};

// ======================= Constantes =======================

const IDLE_THRESHOLD_MS: u64 = 30_000; // apparition après 30 s d'inactivité
const SESSION_GAP_MS: u64 = 15_000;    // frappe "continue" si < 15 s
const ALERT_MS: u64 = 60_000;          // barre 0→60 s, puis alerte

const TICK_MS: u64 = 200;
const REPAINT_MS_VISIBLE: u64 = 200;
const REPAINT_MS_HIDDEN: u64 = 300;

const MIN_W_PX: f32 = 560.0;
const MAX_W_PX: f32 = 1200.0;
const MIN_H_PX: f32 = 160.0;
const MAX_H_PX: f32 = 260.0;

const TITLE: &str = "Idle HUD";

// ======================= Win32 helpers =======================

#[cfg(windows)]
fn primary_screen_px() -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

// Tolérance plein écran basée sur la fenêtre active couvrant son moniteur.
#[cfg(windows)]
fn fullscreen_active_on_any_monitor() -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }

        let mut wr = RECT::default();
        let _ = GetWindowRect(hwnd, &mut wr);

        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(hmon, &mut mi);

        let mw = mi.rcMonitor.right - mi.rcMonitor.left;
        let mh = mi.rcMonitor.bottom - mi.rcMonitor.top;
        if mw <= 0 || mh <= 0 {
            return false;
        }

        let tol: i32 = 2;
        (wr.left - mi.rcMonitor.left).abs() <= tol
            && (wr.top - mi.rcMonitor.top).abs() <= tol
            && (wr.right - mi.rcMonitor.right).abs() <= tol
            && (wr.bottom - mi.rcMonitor.bottom).abs() <= tol
    }
}
#[cfg(not(windows))]
fn fullscreen_active_on_any_monitor() -> bool { false }

#[cfg(windows)]
fn play_connect_once() {
    unsafe { let _ = PlaySoundW(w!("DeviceConnect"), None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT); }
}
#[cfg(windows)]
fn play_disconnect_alert() {
    unsafe { let _ = PlaySoundW(w!("DeviceDisconnect"), None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT); }
}
#[cfg(not(windows))]
fn play_connect_once() {}
#[cfg(not(windows))]
fn play_disconnect_alert() {}

// ======================= Messages =======================

enum Msg {
    Tick,
    KeyActivity(Instant), // frappes texte uniquement
}

// ======================= Stats =======================

struct DayStats {
    date: chrono::NaiveDate,
    writing_ms: u64,
    idle_ms: u64,
    key_presses: u64,
}
impl DayStats {
    fn new_today() -> Self {
        Self {
            date: chrono::Local::now().date_naive(),
            writing_ms: 0,
            idle_ms: 0,
            key_presses: 0,
        }
    }
    fn maybe_rollover(&mut self) {
        let today = chrono::Local::now().date_naive();
        if self.date != today {
            *self = Self::new_today();
        }
    }
}

// ======================= App =======================

struct IdleApp {
    rx: Receiver<Msg>,
    last_tick: Instant,
    last_key: Instant,
    stats: DayStats,

    window_visible: bool,
    initialized: bool,
    visible_since: Option<Instant>,
    alert_fired: bool,
    pending_connect_sound: bool,
}

impl IdleApp {
    fn new(rx: Receiver<Msg>) -> Self {
        let now = Instant::now();
        Self {
            rx,
            last_tick: now,
            last_key: now,
            stats: DayStats::new_today(),
            window_visible: false,
            initialized: false,
            visible_since: None,
            alert_fired: false,
            pending_connect_sound: false,
        }
    }

    fn ms_since_last_key(&self) -> u64 {
        Instant::now().saturating_duration_since(self.last_key).as_millis() as u64
    }
    fn visible_elapsed_ms(&self) -> u64 {
        self.visible_since
            .map(|t| Instant::now().saturating_duration_since(t).as_millis() as u64)
            .unwrap_or(0)
    }

    fn compute_window_size_pts(&self, ctx: &egui::Context) -> Vec2 {
        #[cfg(windows)]
        let (sw, sh) = primary_screen_px();
        #[cfg(not(windows))]
        let (sw, sh) = (1920, 1080);
        let sw = sw.max(800) as f32;
        let sh = sh.max(600) as f32;
        let w_px = (sw * 0.48).clamp(MIN_W_PX, MAX_W_PX);
        let h_px = (sh * 0.18).clamp(MIN_H_PX, MAX_H_PX);
        let ppp = ctx.pixels_per_point();
        Vec2::new(w_px / ppp, h_px / ppp)
    }
    fn ui_scale_factor(&self, ctx: &egui::Context) -> f32 {
        let ppp = ctx.pixels_per_point();
        let size = self.compute_window_size_pts(ctx) * ppp;
        (size.y / 170.0).clamp(1.0, 1.7)
    }
    fn apply_text_style(&self, ctx: &egui::Context) {
        // Simule du gras via .strong() + tailles plus grandes
        let scale = self.ui_scale_factor(ctx);
        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (TextStyle::Heading,   FontId::new(30.0 * scale, FontFamily::Proportional)),
            (TextStyle::Body,      FontId::new(24.0 * scale, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(24.0 * scale, FontFamily::Monospace)),
            (TextStyle::Button,    FontId::new(24.0 * scale, FontFamily::Proportional)),
            (TextStyle::Small,     FontId::new(20.0 * scale, FontFamily::Proportional)),
        ].into();
        ctx.set_style(style);
    }

    fn place_center(&self, ctx: &egui::Context) {
        #[cfg(windows)]
        {
            let size = self.compute_window_size_pts(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            let (sw, sh) = primary_screen_px();
            let ppp = ctx.pixels_per_point();
            let x_pts = ((sw as f32 / ppp) - size.x) * 0.5;
            let y_pts = ((sh as f32 / ppp) - size.y) * 0.5;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x_pts.max(0.0), y_pts.max(0.0))));
        }
    }
    fn place_offscreen(&self, ctx: &egui::Context) {
        #[cfg(windows)]
        {
            let size = self.compute_window_size_pts(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            let (sw, _sh) = primary_screen_px();
            let ppp = ctx.pixels_per_point();
            let x_pts = ((sw as f32 / ppp) - size.x) * 0.5;
            let y_pts = -size.y - 200.0;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x_pts.max(0.0), y_pts)));
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.place_center(ctx);
        self.visible_since = Some(Instant::now());
        self.alert_fired = false;
        self.pending_connect_sound = true; // une seule fois à la reprise
        self.window_visible = true;
    }
    fn hide_window(&mut self, ctx: &egui::Context) {
        self.place_offscreen(ctx);
        self.window_visible = false;
        self.visible_since = None;
        self.alert_fired = false;
        self.pending_connect_sound = false;
    }

    fn handle_tick(&mut self, ctx: &egui::Context) {
        self.stats.maybe_rollover();

        let now = Instant::now();
        let dt_ms = now.saturating_duration_since(self.last_tick).as_millis() as u64;
        self.last_tick = now;

        let since_key = self.ms_since_last_key();
        let typing_now = since_key <= SESSION_GAP_MS;

        if typing_now {
            self.stats.writing_ms = self.stats.writing_ms.saturating_add(dt_ms);
        } else {
            self.stats.idle_ms = self.stats.idle_ms.saturating_add(dt_ms);
        }

        let fullscreen = fullscreen_active_on_any_monitor();

        if fullscreen && self.window_visible {
            self.hide_window(ctx);
        }

        if self.window_visible && !fullscreen {
            let vms = self.visible_elapsed_ms();
            if vms >= ALERT_MS && !self.alert_fired {
                self.alert_fired = true;
                play_disconnect_alert();
            }
        }

        if since_key >= IDLE_THRESHOLD_MS {
            if !self.window_visible && !fullscreen {
                self.show_window(ctx);
            }
        } else if self.window_visible {
            self.hide_window(ctx);
        }
    }
}

// ======================= Impl eframe::App =======================

impl eframe::App for IdleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.initialized {
            // Démarre "minimisée": dès le 1er frame on place hors-écran.
            let visuals = if ctx.style().visuals.dark_mode { egui::Visuals::dark() } else { egui::Visuals::light() };
            ctx.set_visuals(visuals);
            self.apply_text_style(ctx);
            self.place_offscreen(ctx);
            self.initialized = true;
        }

        if self.window_visible {
            ctx.request_repaint_after(Duration::from_millis(REPAINT_MS_VISIBLE));
        } else {
            ctx.request_repaint_after(Duration::from_millis(REPAINT_MS_HIDDEN));
        }

        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Tick => self.handle_tick(ctx),
                Msg::KeyActivity(t) => {
                    let fullscreen = fullscreen_active_on_any_monitor();
                    self.last_key = t;
                    self.stats.key_presses = self.stats.key_presses.saturating_add(1);

                    if self.window_visible && self.pending_connect_sound && !fullscreen {
                        self.pending_connect_sound = false;
                        play_connect_once();
                    }

                    if self.window_visible {
                        self.hide_window(ctx);
                    }
                }
            }
        }

        // UI: ÉCRITURE, IDLE, KEYS, CPS + barre 0→60 s en vert (puis rouge après alerte).
        let scale = self.ui_scale_factor(ctx);
        let green = Color32::from_rgb(0, 190, 120);
        let red = Color32::from_rgb(220, 70, 70);
        let accent = if self.alert_fired { red } else { green };

        let cps_mean = if self.stats.writing_ms > 0 {
            (self.stats.key_presses as f32) / (self.stats.writing_ms as f32 / 1000.0)
        } else {
            0.0
        };

        let elapsed = self.visible_elapsed_ms().min(ALERT_MS);
        let progress = if self.window_visible { elapsed as f32 / ALERT_MS as f32 } else { 0.0 };

        egui::CentralPanel::default()
            .frame(egui::Frame::default().inner_margin(Margin::symmetric(12.0 * scale, 12.0 * scale)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(format!("ÉCRITURE: {}", fmt_hms(self.stats.writing_ms))).strong().color(accent));
                    ui.label(RichText::new(format!("IDLE: {}", fmt_hms(self.stats.idle_ms))).strong().color(accent));
                    ui.label(RichText::new(format!("KEYS: {}", self.stats.key_presses)).strong().color(accent));
                    ui.label(RichText::new(format!("CPS: {:.2}", cps_mean)).strong().color(accent));

                    ui.add_space(8.0 * scale);
                    draw_bar(ui, progress, green, 20.0 * scale); // toujours verte pendant la progression
                });
            });
    }
}

// ======================= UI utils =======================

fn draw_bar(ui: &mut egui::Ui, fraction: f32, fill: Color32, height: f32) {
    let desired = egui::vec2(ui.available_width() * 0.9, height);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let centered = egui::Rect::from_min_max(
        egui::pos2(rect.center().x - desired.x * 0.5, rect.min.y),
        egui::pos2(rect.center().x + desired.x * 0.5, rect.max.y),
    );
    let painter = ui.painter();
    let bg = ui.visuals().widgets.inactive.bg_fill;
    painter.rect_filled(centered, 6.0, bg);
    if fraction > 0.0 {
        let w = centered.width() * fraction.clamp(0.0, 1.0);
        let filled = egui::Rect::from_min_size(centered.min, egui::vec2(w, centered.height()));
        painter.rect_filled(filled, 6.0, fill);
    }
}

// ======================= Divers =======================

fn is_character_key(k: Key) -> bool {
    use Key::*;
    matches!(
        k,
        KeyA | KeyB | KeyC | KeyD | KeyE | KeyF | KeyG | KeyH | KeyI | KeyJ | KeyK | KeyL | KeyM
            | KeyN | KeyO | KeyP | KeyQ | KeyR | KeyS | KeyT | KeyU | KeyV | KeyW | KeyX | KeyY
            | KeyZ | Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9
    )
}

fn fmt_hms(ms: u64) -> String {
    let total_sec = ms / 1000;
    let h = total_sec / 3600;
    let m = (total_sec % 3600) / 60;
    let s = total_sec % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

// ======================= Threads =======================

fn spawn_tick_thread(tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let period = Duration::from_millis(TICK_MS);
        loop {
            let _ = tx.send(Msg::Tick);
            std::thread::sleep(period);
        }
    });
}

fn spawn_keyboard_thread(tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let callback = move |event: Event| {
            if let EventType::KeyPress(k) = event.event_type {
                if is_character_key(k) {
                    let _ = tx.send(Msg::KeyActivity(Instant::now()));
                }
            }
        };
        let _ = rdev::listen(callback);
    });
}

// ======================= Entrée =======================

fn main() -> eframe::Result<()> {
    let (tx, rx) = unbounded();
    spawn_tick_thread(tx.clone());
    spawn_keyboard_thread(tx);

    // Démarre "minimisée": fenêtre décorée, topmost, mais placée hors-écran au 1er frame.
    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_decorations(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_inner_size([820.0, 190.0])
        .with_visible(true);

    let native_options = NativeOptions { viewport, ..Default::default() };

    eframe::run_native(
        TITLE,
        native_options,
        Box::new(move |_cc| Ok(Box::new(IdleApp::new(rx)))),
    )
}
