//! DRUMS desktop GUI (Phase 1).
//!
//! This crate is deliberately tiny: it owns the egui/eframe front-end and
//! delegates all MIDI work to `drums-core`. The MIDI backend runs its own
//! real-time thread (inside the midir callback); the GUI thread only drains a
//! thread-safe event channel each frame and displays the results. The GUI
//! frame rate is never a timing source — every event carries the
//! high-resolution timestamp captured at the moment it was received.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use drums_core::time::{SharedClock, SystemClock};
use drums_core::{MidiBackend, MidiEvent, PortInfo};
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("MIDI Drum Practice — Phase 1"),
        ..Default::default()
    };
    eframe::run_native(
        "MIDI Drum Practice",
        options,
        Box::new(|cc| Ok(Box::new(DrumApp::new(cc)))),
    )
}

/// Application state. GUI-only; no MIDI processing happens here.
struct DrumApp {
    backend: MidiBackend,
    ports: Vec<PortInfo>,
    selected: Option<usize>,
    status: String,
    connected: bool,
    /// Received MIDI events, newest first.
    events: Vec<MidiEvent>,
    frames: u64,
}

impl DrumApp {
    fn new(ctx: &eframe::CreationContext<'_>) -> Self {
        let clock: SharedClock = Arc::new(SystemClock::new());
        let mut backend = MidiBackend::new(clock);

        // As soon as the MIDI thread queues events, wake eframe so the UI
        // repaints immediately instead of waiting for mouse/keyboard input.
        // `egui::Context::request_repaint` is thread-safe and explicitly meant
        // to be called from background threads; it schedules a single repaint,
        // so the GUI still goes idle (no polling) when no MIDI is arriving.
        let repaint_ctx = ctx.egui_ctx.clone();
        backend.set_on_event(Some(Arc::new(move || repaint_ctx.request_repaint())));

        // Initial device enumeration; any failure lands in the status line
        // instead of crashing so the app still opens (e.g. no MIDI hardware).
        let (ports, status) = match backend.list_ports() {
            Ok(ps) => (ps, String::new()),
            Err(e) => (Vec::new(), format!("MIDI unavailable: {e}")),
        };

        set_app_style(&ctx.egui_ctx);

        Self {
            backend,
            ports,
            selected: None,
            status,
            connected: false,
            events: Vec::new(),
            frames: 0,
        }
    }

    fn drain_events(&mut self) {
        let new_events = self.backend.poll_events();
        for ev in new_events {
            self.events.insert(0, ev);
        }
        if self.events.len() > 400 {
            self.events.truncate(400);
        }
    }

    fn refresh_devices(&mut self) {
        match self.backend.list_ports() {
            Ok(ps) => self.ports = ps,
            Err(e) => self.status = format!("{e}"),
        }
    }

    fn connect_clicked(&mut self) {
        let Some(index) = self.selected else {
            self.status = "Select a MIDI input device first.".to_string();
            return;
        };
        match self.backend.connect(index) {
            Ok(name) => {
                self.connected = true;
                self.events.clear();
                self.status = format!("Connected to “{name}”.");
            }
            Err(e) => {
                self.connected = false;
                self.status = format!("Connection failed: {e}");
            }
        }
    }

    fn disconnect_clicked(&mut self) {
        self.backend.disconnect();
        self.connected = false;
        self.events.clear();
        self.status = "Disconnected.".to_string();
    }
}
impl eframe::App for DrumApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames = self.frames.wrapping_add(1);

        // Periodic (~1s) disconnect detection. Cheap re-enumeration only.
        if self.frames % 60 == 0 {
            let was_connected = self.connected;
            let alive = self.backend.refresh_connection();
            if was_connected && !alive {
                self.connected = false;
                self.events.clear();
                self.status =
                    "Device disconnected — re-connect or check the cable/port.".to_string();
            }
        }

        self.drain_events();

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.heading("MIDI Drum Practice");
            ui.label("Phase 1 — MIDI Foundation (device monitoring)");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("⟳ Refresh devices").clicked() {
                    self.refresh_devices();
                }

                let selected_text = self
                    .ports
                    .get(self.selected.unwrap_or(usize::MAX))
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "Select a MIDI input…".to_string());
                egui::ComboBox::from_id_salt("device_combo")
                    .width(320.0)
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        if self.ports.is_empty() {
                            ui.add_enabled(false, egui::Label::new("No MIDI devices found"));
                        }
                        for p in &self.ports {
                            ui.selectable_value(&mut self.selected, Some(p.index), &p.name);
                        }
                    });

                if self.connected {
                    if ui.button("Disconnect").clicked() {
                        self.disconnect_clicked();
                    }
                } else {
                    let can_connect = self.selected.is_some() && !self.ports.is_empty();
                    if ui
                        .add_enabled(can_connect, egui::Button::new("⏵ Connect"))
                        .clicked()
                    {
                        self.connect_clicked();
                    }
                }
            });
            ui.add_space(2.0);
            ui.label(format!(
                "Available MIDI input devices: {}   ·   {}",
                self.ports.len(),
                if self.status.is_empty() {
                    if self.connected {
                        format!(
                            "Listening on “{}”",
                            self.backend.connected_port_name().unwrap_or("?")
                        )
                    } else {
                        "Idle — select a device and press Connect.".to_string()
                    }
                } else {
                    self.status.clone()
                }
            ));
            ui.add_space(4.0);
            ui.separator();
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            ui.heading("Latest event");
            ui.separator();

            if let Some(latest) = self.events.first() {
                egui::Grid::new("latest_grid")
                    .num_columns(2)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Message type");
                        ui.monospace(latest.message.kind.name());
                        ui.end_row();
                        ui.label("MIDI note number");
                        ui.monospace(fmt_opt(latest.message.note()));
                        ui.end_row();
                        ui.label("Velocity");
                        ui.monospace(fmt_opt(latest.message.velocity()));
                        ui.end_row();
                        ui.label("MIDI channel");
                        ui.monospace(fmt_opt(latest.message.channel_number()));
                        ui.end_row();
                        ui.label("Timestamp");
                        ui.monospace(timestamp_text(latest.timestamp_micros));
                        ui.end_row();
                    });
            } else {
                ui.weak("No MIDI events yet. Connect a device and hit a pad.");
            }

            ui.add_space(12.0);
            ui.heading(format!("Event log ({})", self.events.len()));
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for ev in &self.events {
                        ui.monospace(fmt_event(ev));
                    }
                });
        });
    }
}

/// Applies a dark theme for a serious, low-distraction look.
fn set_app_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    style.visuals.override_text_color = Some(egui::Color32::from_rgb(220, 222, 218));
    ctx.set_style(style);
}

fn fmt_opt(value: Option<u8>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => "— (n/a)".to_string(),
    }
}

fn timestamp_text(us: u64) -> String {
    format!("{} µs  (≈ {:.3} s)", us, us as f64 / 1_000_000.0)
}

fn fmt_event(ev: &MidiEvent) -> String {
    format!(
        "{:>12} µs  {:<17} ch={:<4} note={:<5} vel={:<4}",
        ev.timestamp_micros,
        ev.message.kind.name(),
        fmt_opt(ev.message.channel_number()),
        fmt_opt(ev.message.note()),
        fmt_opt(ev.message.velocity()),
    )
}