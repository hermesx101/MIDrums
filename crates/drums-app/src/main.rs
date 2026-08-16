//! DRUMS desktop GUI (Phase 1 + Phase 2).
//!
//! This crate is deliberately tiny: it owns the egui/eframe front-end and
//! delegates all MIDI work to `drums-core`. The MIDI backend runs its own
//! real-time thread (inside the midir callback); the GUI thread only drains a
//! thread-safe event channel each frame and displays the results. The GUI
//! frame rate is never a timing source — every event carries the
//! high-resolution timestamp captured at the moment it was received.
//!
//! Phase 2 adds two focused flows on top of the Phase 1 monitor:
//! - the **setup wizard** (`wizard_ui`), which learns each drum's MIDI note by
//!   waiting for, and asking the user to confirm, real MIDI hits, and
//! - the **manual mapping** view (`mapping_ui`) for inspecting/editing a
//!   device's saved configuration.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mapping_ui;
mod midi_file_ui;
mod click_audio;
mod wizard_ui;

use std::sync::Arc;
use std::time::Instant;

use drums_core::mapping::{DrumKitConfiguration, MappingStore, SetupWizard, SUPPORTED_COMPONENTS};
use drums_core::time::{SharedClock, SystemClock};
use drums_core::{Metronome, MidiBackend, MidiEvent, MidiFile, MusicalTransport, PortInfo, Subdivision, Timeline};
use eframe::egui;

/// Top-level views the app can show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    /// Phase 1 live MIDI monitor.
    Monitor,
    /// Phase 2 setup wizard.
    Setup,
    MidiInspection,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("MIDI Drum Practice — Phase 2"),
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
    /// Per-device mapping persistence.
    store: MappingStore,
    /// The configuration currently active for the connected device.
    active_config: Option<DrumKitConfiguration>,
    /// Setup wizard state, present while a setup run is in progress.
    wizard: Option<SetupWizard>,
    /// Name of the device the running wizard was started against.
    wizard_device: Option<String>,
    /// Working copy edited in the manual mapping view.
    mapping_draft: Option<DrumKitConfiguration>,
    /// Currently selected view/tab.
    view: View,
    imported_midi: Option<MidiFile>,
    selected_drum_track: Option<usize>,
    midi_import_error: Option<String>,
    transport: Option<MusicalTransport>,
    timeline: Option<Timeline>,
    last_transport_update: Instant,
    metronome: Metronome,
    metronome_enabled: bool,
    click_audio: click_audio::ClickAudio,
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
            store: MappingStore::default(),
            active_config: None,
            wizard: None,
            wizard_device: None,
            mapping_draft: None,
            view: View::Monitor,
            imported_midi: None,
            selected_drum_track: None,
            midi_import_error: None,
            transport: None,
            timeline: None,
            last_transport_update: Instant::now(),
            metronome: Metronome::new(Subdivision::Eighth),
            metronome_enabled: false,
            click_audio: click_audio::ClickAudio::new(),
        }
    }

    fn drain_events(&mut self) {
        let new_events = self.backend.poll_events();
        // While a setup run is active, every incoming MIDI event is also pushed
        // into the wizard so it can spot the note the user hits. The wizard
        // handles Note On/Off filtering and confirmation itself.
        if self.wizard.is_some() {
            for ev in &new_events {
                if let Some(wizard) = self.wizard.as_mut() {
                    wizard.handle_event(ev);
                }
            }
        }
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
                // Restore a previously saved mapping for this device, so the
                // same kit reconnects to the configuration the user built.
                let stored = self.store.load(&name).unwrap_or(None);
                self.active_config = stored.clone();
                self.mapping_draft = None;
                self.status = match &stored {
                    Some(cfg) => format!(
                        "Connected to “{name}”. Restored mapping: {} of {} drums configured.",
                        cfg.mapped_count(),
                        SUPPORTED_COMPONENTS.len()
                    ),
                    None => format!(
                        "Connected to “{name}”. No saved mapping yet — run the Setup Wizard."
                    ),
                };
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
        self.active_config = None;
        self.wizard = None;
        self.wizard_device = None;
        self.mapping_draft = None;
        self.status = "Disconnected.".to_string();
    }

    fn import_midi_clicked(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("MIDI files", &["mid", "midi"]).set_title("Import MIDI file").pick_file() else { return; };
        match MidiFile::from_path(&path) {
            Ok(file) => {
                self.selected_drum_track = file.suggested_drum_track();
                self.transport = Some(MusicalTransport::from_midi_file(&file));
                self.imported_midi = Some(file);
                self.rebuild_timeline();
                self.metronome.reset();
                self.midi_import_error = None;
                self.last_transport_update = Instant::now();
                self.view = View::MidiInspection;
            }
            Err(error) => { self.midi_import_error = Some(error.to_string()); self.view = View::MidiInspection; }
        }
    }

    fn rebuild_timeline(&mut self) {
        let (Some(file), Some(track), Some(transport)) = (&self.imported_midi, self.selected_drum_track, &self.transport) else {
            self.timeline = None;
            return;
        };
        let notes = file.expected_notes_for_track(track, self.active_config.as_ref());
        self.timeline = Some(Timeline::from_expected_notes(notes, transport.position_map(), file.duration_ticks));
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
                self.active_config = None;
                self.wizard = None;
                self.wizard_device = None;
                self.mapping_draft = None;
                self.status =
                    "Device disconnected — re-connect or check the cable/port.".to_string();
            }
        }

        self.drain_events();

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_transport_update);
        self.last_transport_update = now;
        if let Some(transport) = self.transport.as_mut() {
            transport.advance_by(elapsed);
            if self.metronome_enabled {
                for event in self.metronome.observe(transport) { self.click_audio.play(event.kind); }
            } else {
                self.metronome.sync_to_transport(transport);
            }
            if transport.is_playing() {
                // Rendering is not the clock: this only schedules a future
                // frame so the elapsed monotonic-time transport can be shown.
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.heading("MIDI Drum Practice");
            ui.label("Phase 2 — Setup wizard & drum mapping");
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
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view, View::Monitor, "📡 Monitor");
                ui.selectable_value(&mut self.view, View::Setup, "🎛 Setup Wizard");
                ui.selectable_value(&mut self.view, View::MidiInspection, "Import MIDI");
            });
            ui.add_space(4.0);
            if self.connected {
                let mapping_line = match &self.active_config {
                    Some(cfg) => format!(
                        "Drum mapping for “{}”: {}/{} components configured.",
                        cfg.device_name,
                        cfg.mapped_count(),
                        SUPPORTED_COMPONENTS.len()
                    ),
                    None => "No drum mapping loaded for this device yet.".to_string(),
                };
                ui.weak(mapping_line);
            }
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
            // A vertically scrollable content area is shared by all views so
            // the wizard/mapping tables never get clipped on small windows.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.view {
                    View::Monitor => show_monitor_ui(self, ui),
                    View::Setup => wizard_ui::show(self, ui),
                    View::MidiInspection => midi_file_ui::show(self, ui),
                                                        });
        });

        // This second check covers a Play click made during the UI pass above.
        // The transport itself remains driven solely by elapsed monotonic time.
        if self.transport.as_ref().is_some_and(|transport| transport.is_playing()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

/// Phase 1 live MIDI monitor view (unchanged behaviour, moved into the tab).
fn show_monitor_ui(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.heading("Latest event");
    ui.separator();

    if let Some(latest) = app.events.first() {
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
    ui.heading(format!("Event log ({})", app.events.len()));
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for ev in &app.events {
                ui.monospace(fmt_event(ev));
            }
        });
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
