//! Egui/eframe application shell for MI Drums.
//!
//! This crate is deliberately tiny: it owns the egui/eframe front-end and
//! delegates all MIDI work to `drums-core`. The MIDI backend runs its own
//! real-time thread (inside the midir callback); the GUI thread only drains a
//! thread-safe event channel each frame and displays the results. The GUI
//! frame rate is never a timing source — every event carries the
//! high-resolution timestamp captured at the moment it was received.
//!
//! Two focused configuration flows complement the practice UI:
//! - the **setup wizard** (`wizard_ui`), which learns each drum's MIDI note by
//!   waiting for, and asking the user to confirm, real MIDI hits, and
//! - the **manual mapping** view (`mapping_ui`) for inspecting/editing a
//!   device's saved configuration. Mapping, timing, and scoring behavior stays
//!   in `drums-core` so the front-end does not duplicate domain logic.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_output;
mod backing_audio;
mod click_audio;
mod mapping_ui;
mod midi_file_ui;
mod monitor_ui;
mod notation_ui;
mod performance_analysis_ui;
mod practice_settings;
mod preferences_ui;
mod progress_ui;
mod track_library;
mod wizard_ui;

use std::sync::Arc;
use std::time::Instant;

use drums_core::mapping::{
    DrumKitConfiguration, MappingStore, MidiInputResolver, ObservedMidiInput, SetupWizard,
    SUPPORTED_COMPONENTS,
};
use drums_core::time::{SharedClock, SystemClock};
use drums_core::{
    new_session_id, suggest_subdivision, ActualHit, CompletedPracticeSession, MatchingConfig,
    Metronome, MidiBackend, MidiEvent, MidiFile, MusicalTransport, PortInfo, PracticeResult,
    PracticeSession, PracticeSessionStore, SessionBuildMetadata, Subdivision,
    SubdivisionSuggestion, Timeline,
};
use eframe::egui;
use practice_settings::{AccentColour, ThemePreference, UiScale};
use track_library::{LibraryEntry, LibrarySort};

const MIDI_CONNECTION_REFRESH_FRAMES: u64 = 60;
const MAX_MIDI_EVENT_LOG_ENTRIES: usize = 400;
const DEFAULT_PRACTICE_LOOKAHEAD_SECONDS: f64 = 4.0;
const AUDIO_OUTPUT_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Top-level views the app can show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    /// Phase 1 live MIDI monitor.
    Monitor,
    /// Phase 2 setup wizard.
    Setup,
    Mapping,
    TrackLibrary,
    Practice,
    PerformanceAnalysis,
    Progress,
    SheetMusic,
    Preferences,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PracticeVisual {
    Timeline,
    SheetMusic,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PracticeLoopMode {
    Off,
    CurrentBar,
    TwoBars,
    FourBars,
    EightBars,
    Custom,
}

#[derive(Clone, Debug)]
struct ActiveSessionRecording {
    id: String,
    track_id: String,
    started_at_unix_ms: u64,
    practice_bpm: f64,
    time_signature: String,
    subdivision: Subdivision,
}

impl PracticeLoopMode {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::CurrentBar => "Current Bar",
            Self::TwoBars => "2 Bars",
            Self::FourBars => "4 Bars",
            Self::EightBars => "8 Bars",
            Self::Custom => "Custom",
        }
    }
    fn bars(self) -> Option<u64> {
        match self {
            Self::CurrentBar => Some(1),
            Self::TwoBars => Some(2),
            Self::FourBars => Some(4),
            Self::EightBars => Some(8),
            _ => None,
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("MI Drums"),
        ..Default::default()
    };
    eframe::run_native(
        "MI Drums",
        options,
        Box::new(|cc| Ok(Box::new(DrumApp::new(cc)))),
    )
}

/// Top-level desktop state and cross-feature orchestration boundary.
///
/// Hardware-independent parsing, mapping, transport, and scoring rules live in
/// `drums-core`. This type owns their active instances, drains callback events
/// on the UI thread, coordinates audio/session persistence, and delegates each
/// view to its UI module.
struct DrumApp {
    clock: SharedClock,
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
    midi_input_resolver: MidiInputResolver,
    /// Setup wizard state, present while a setup run is in progress.
    wizard: Option<SetupWizard>,
    /// Name of the device the running wizard was started against.
    wizard_device: Option<String>,
    wizard_last_input: Option<MidiEvent>,
    /// Working copy edited in the manual mapping view.
    mapping_draft: Option<DrumKitConfiguration>,
    mapping_assign_component: drums_core::DrumComponent,
    mapping_reassign_component: Option<drums_core::DrumComponent>,
    mapping_replace_component: bool,
    mapping_reassign_after_micros: u64,
    mapping_reassign_candidate: Option<MidiEvent>,
    mapping_reassign_started: Option<Instant>,
    /// Last meaningful hit shown by Drum Mapping; Note Off never replaces it.
    mapping_last_input: Option<MidiEvent>,
    mapping_last_detected_component: Option<drums_core::DrumComponent>,
    /// Last played drum, retained when pedal/controller updates follow a hit.
    mapping_last_note_component: Option<drums_core::DrumComponent>,
    /// Currently selected view/tab.
    view: View,
    imported_midi: Option<MidiFile>,
    selected_drum_track: Option<usize>,
    midi_import_error: Option<String>,
    transport: Option<MusicalTransport>,
    timeline: Option<Timeline>,
    practice_session: Option<PracticeSession>,
    session_store: PracticeSessionStore,
    active_session_recording: Option<ActiveSessionRecording>,
    last_live_hit: Option<ActualHit>,
    displayed_practice_result: Option<PracticeResult>,
    displayed_result_since: Option<Instant>,
    observed_practice_result_count: usize,
    last_transport_update: Instant,
    metronome: Metronome,
    subdivision_auto: bool,
    subdivision_suggestion: Option<SubdivisionSuggestion>,
    metronome_enabled: bool,
    /// Persisted presentation-only filter for the Timeline practice view.
    expected_drums_only: bool,
    /// Logical drum roles cached when the active lesson context changes.
    expected_timeline_drums: midi_file_ui::ExpectedTimelineDrums,
    /// UI-only future window for the Phase 5A rhythm chart.
    practice_lookahead_seconds: f64,
    practice_loop_mode: PracticeLoopMode,
    practice_loop_start_tick: u64,
    practice_loop_end_tick: u64,
    practice_loop_start_bar: u64,
    practice_loop_end_bar: u64,
    practice_loop_iteration: u64,
    count_in_bars: u8,
    count_in_active: bool,
    count_in_end_tick: u64,
    count_in_target_tick: u64,
    track_library: Vec<LibraryEntry>,
    library_search: String,
    library_sort: LibrarySort,
    library_rename_path: Option<String>,
    library_rename_buffer: String,
    library_remove_path: Option<String>,
    current_library_path: Option<String>,
    library_selected_path: Option<String>,
    completion_saved: bool,
    analysis_track_path: Option<String>,
    analysis_session_id: Option<String>,
    progress_track_path: Option<String>,
    progress_data: Option<drums_core::TrackProgress>,
    notation_score: Option<drums_core::DrumNotationScore>,
    notation_zoom: f32,
    notation_follow_playback: bool,
    notation_follow_suspended_until: Option<Instant>,
    practice_visual: PracticeVisual,
    audio_output: audio_output::AudioOutput,
    audio_device_refresh: audio_output::AudioDeviceRefreshWorker,
    manual_audio_refresh_pending: bool,
    last_audio_output_refresh: Instant,
    backing_audio: backing_audio::BackingAudio,
    audio_sync_offset_ms: f64,
    midi_input_latency_ms: f64,
    start_audio_with_practice: bool,
    theme_preference: ThemePreference,
    accent_colour: AccentColour,
    ui_scale: UiScale,
    reset_confirmation: preferences_ui::ResetConfirmation,
}

impl DrumApp {
    fn save_practice_settings(&self) -> std::io::Result<()> {
        practice_settings::save(&practice_settings::PracticeSettings {
            midi_input_latency_ms: self.midi_input_latency_ms,
            audio_sync_offset_ms: self.audio_sync_offset_ms,
            start_audio_with_practice: self.start_audio_with_practice,
            practice_view: match self.practice_visual {
                PracticeVisual::Timeline => "timeline",
                PracticeVisual::SheetMusic => "sheet_music",
            }
            .into(),
            expected_drums_only: self.expected_drums_only,
            audio_output_device: self.audio_output.preferred_device().map(str::to_string),
            output_volume: self.audio_output.volume(),
            output_muted: self.audio_output.muted(),
            theme: self.theme_preference,
            accent_colour: self.accent_colour,
            ui_scale: self.ui_scale,
        })
    }

    fn accent_color(&self) -> egui::Color32 {
        accent_color(self.accent_colour)
    }

    fn restore_first_launch_after_reset(&mut self, ctx: &egui::Context) {
        let defaults = practice_settings::PracticeSettings::default();
        self.backend.disconnect();
        self.connected = false;
        self.events.clear();
        self.active_config = None;
        self.midi_input_resolver.reset();
        self.mapping_last_note_component = None;
        self.wizard = None;
        self.wizard_device = None;
        self.wizard_last_input = None;
        self.mapping_draft = None;
        self.mapping_reassign_component = None;
        self.mapping_replace_component = false;
        self.mapping_reassign_after_micros = 0;
        self.mapping_reassign_candidate = None;
        self.mapping_reassign_started = None;
        self.mapping_last_input = None;
        self.mapping_last_detected_component = None;

        self.imported_midi = None;
        self.selected_drum_track = None;
        self.midi_import_error = None;
        self.transport = None;
        self.timeline = None;
        self.practice_session = None;
        self.active_session_recording = None;
        self.last_live_hit = None;
        self.displayed_practice_result = None;
        self.displayed_result_since = None;
        self.observed_practice_result_count = 0;
        self.last_transport_update = Instant::now();
        self.metronome = Metronome::new(Subdivision::Eighth);
        self.subdivision_auto = true;
        self.subdivision_suggestion = None;
        self.metronome_enabled = false;
        self.expected_drums_only = defaults.expected_drums_only;
        self.expected_timeline_drums = midi_file_ui::ExpectedTimelineDrums::default();
        self.practice_lookahead_seconds = DEFAULT_PRACTICE_LOOKAHEAD_SECONDS;
        self.practice_loop_mode = PracticeLoopMode::Off;
        self.practice_loop_start_tick = 0;
        self.practice_loop_end_tick = 0;
        self.practice_loop_start_bar = 1;
        self.practice_loop_end_bar = 2;
        self.practice_loop_iteration = 1;
        self.count_in_bars = 0;
        self.count_in_active = false;
        self.count_in_end_tick = 0;
        self.count_in_target_tick = 0;

        self.track_library.clear();
        self.library_search.clear();
        self.library_sort = LibrarySort::RecentlyAdded;
        self.library_rename_path = None;
        self.library_rename_buffer.clear();
        self.library_remove_path = None;
        self.current_library_path = None;
        self.library_selected_path = None;
        self.completion_saved = false;
        self.analysis_track_path = None;
        self.analysis_session_id = None;
        self.progress_track_path = None;
        self.progress_data = None;
        self.notation_score = None;
        self.notation_zoom = 1.0;
        self.notation_follow_playback = false;
        self.notation_follow_suspended_until = None;
        self.practice_visual = PracticeVisual::Timeline;
        self.backing_audio.stop();
        self.backing_audio.unload();
        self.audio_output.set_volume(defaults.output_volume);
        self.audio_output.set_muted(defaults.output_muted);
        self.audio_output
            .select_device(defaults.audio_output_device.clone());
        let _ = self.sync_backing_audio_output();
        self.manual_audio_refresh_pending = false;
        self.request_audio_output_refresh(false);
        self.last_audio_output_refresh = Instant::now();

        self.midi_input_latency_ms = defaults.midi_input_latency_ms;
        self.audio_sync_offset_ms = defaults.audio_sync_offset_ms;
        self.start_audio_with_practice = defaults.start_audio_with_practice;
        self.theme_preference = defaults.theme;
        self.accent_colour = defaults.accent_colour;
        self.ui_scale = defaults.ui_scale;
        apply_app_appearance(
            ctx,
            self.theme_preference,
            self.accent_colour,
            self.ui_scale,
        );

        self.view = View::Setup;
        self.status =
            "All application data was reset. Connect a MIDI device to begin setup.".to_string();
    }

    fn navigate_away_from_practice(&mut self, view: View) {
        if let Some(transport) = self.transport.as_mut() {
            transport.pause();
            self.metronome.sync_to_transport(transport);
        }
        self.view = view;
        self.backing_audio.stop();
    }

    fn resync_backing_audio(&mut self) {
        let Some(transport) = self.transport.as_ref() else {
            return;
        };
        let position = drums_core::audio_position(
            transport.elapsed_seconds(),
            self.audio_sync_offset_ms,
            self.backing_audio.duration(),
        );
        let _ = self.backing_audio.seek(position);
    }

    fn sync_backing_audio_output(&mut self) -> Result<(), String> {
        self.backing_audio.set_output(
            self.audio_output.handle(),
            self.audio_output.effective_gain(),
        )
    }

    fn save_track_library(&mut self) -> bool {
        match track_library::save(&self.track_library) {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("Could not save Track Library: {error}");
                false
            }
        }
    }

    fn request_audio_output_refresh(&mut self, manual: bool) -> bool {
        let started = self.audio_device_refresh.request_refresh();
        if manual && (started || self.audio_device_refresh.is_refreshing()) {
            self.manual_audio_refresh_pending = true;
        }
        started
    }

    fn audio_output_refresh_in_flight(&self) -> bool {
        self.audio_device_refresh.is_refreshing()
    }

    fn apply_completed_audio_device_refresh(&mut self) {
        let Some(result) = self.audio_device_refresh.try_take_latest() else {
            return;
        };
        let manual = std::mem::take(&mut self.manual_audio_refresh_pending);
        let update = match result {
            Ok(snapshot) => self.audio_output.apply_device_snapshot(snapshot),
            Err(error) => self.audio_output.apply_refresh_error(error),
        };
        if update.route_changed {
            self.status = match self.sync_backing_audio_output() {
                Err(error) => {
                    format!("Audio output changed, but backing audio could not be routed: {error}")
                }
                Ok(()) => update.notice.unwrap_or_else(|| {
                    format!(
                        "Audio output changed to {}.",
                        self.audio_output
                            .active_device()
                            .unwrap_or(audio_output::DEFAULT_SYSTEM_DEVICE)
                    )
                }),
            };
        } else if let Some(notice) = update.notice {
            self.status = notice;
        } else if manual {
            self.status = "Audio output devices refreshed.".into();
        }
    }

    fn apply_audio_output_gain(&mut self) {
        self.backing_audio
            .set_output_gain(self.audio_output.effective_gain());
    }

    fn new(ctx: &eframe::CreationContext<'_>) -> Self {
        let practice_settings = practice_settings::load();
        let clock: SharedClock = Arc::new(SystemClock::new());
        let mut backend = MidiBackend::new(clock.clone());

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
        let selected = sole_port_index(&ports);

        apply_app_appearance(
            &ctx.egui_ctx,
            practice_settings.theme,
            practice_settings.accent_colour,
            practice_settings.ui_scale,
        );

        let store = MappingStore::default();
        let needs_initial_setup = store
            .load_all()
            .map(|configs| needs_initial_setup(&configs))
            .unwrap_or(true);
        let audio_output = audio_output::AudioOutput::new(
            practice_settings.audio_output_device.clone(),
            practice_settings.output_volume,
            practice_settings.output_muted,
        );
        let backing_audio =
            backing_audio::BackingAudio::new(audio_output.handle(), audio_output.effective_gain());
        let audio_refresh_ctx = ctx.egui_ctx.clone();
        let mut audio_device_refresh =
            audio_output::AudioDeviceRefreshWorker::new(Arc::new(move || {
                audio_refresh_ctx.request_repaint()
            }));
        audio_device_refresh.request_refresh();
        let status = match (status.is_empty(), audio_output.notice()) {
            (true, Some(notice)) => notice.to_string(),
            (false, Some(notice)) => format!("{status} {notice}"),
            _ => status,
        };

        Self {
            clock,
            backend,
            ports,
            selected,
            status,
            connected: false,
            events: Vec::new(),
            frames: 0,
            store,
            active_config: None,
            midi_input_resolver: MidiInputResolver::default(),
            wizard: None,
            wizard_device: None,
            wizard_last_input: None,
            mapping_draft: None,
            mapping_assign_component: drums_core::DrumComponent::HiHatPedal,
            mapping_reassign_component: None,
            mapping_replace_component: false,
            mapping_reassign_after_micros: 0,
            mapping_reassign_candidate: None,
            mapping_reassign_started: None,
            mapping_last_input: None,
            mapping_last_detected_component: None,
            mapping_last_note_component: None,
            view: if needs_initial_setup {
                View::Setup
            } else {
                View::TrackLibrary
            },
            imported_midi: None,
            selected_drum_track: None,
            midi_import_error: None,
            transport: None,
            timeline: None,
            practice_session: None,
            session_store: PracticeSessionStore::default(),
            active_session_recording: None,
            last_live_hit: None,
            displayed_practice_result: None,
            displayed_result_since: None,
            observed_practice_result_count: 0,
            last_transport_update: Instant::now(),
            metronome: Metronome::new(Subdivision::Eighth),
            subdivision_auto: true,
            subdivision_suggestion: None,
            metronome_enabled: false,
            expected_drums_only: practice_settings.expected_drums_only,
            expected_timeline_drums: midi_file_ui::ExpectedTimelineDrums::default(),
            practice_lookahead_seconds: DEFAULT_PRACTICE_LOOKAHEAD_SECONDS,
            practice_loop_mode: PracticeLoopMode::Off,
            practice_loop_start_tick: 0,
            practice_loop_end_tick: 0,
            practice_loop_start_bar: 1,
            practice_loop_end_bar: 2,
            practice_loop_iteration: 1,
            count_in_bars: 0,
            count_in_active: false,
            count_in_end_tick: 0,
            count_in_target_tick: 0,
            track_library: track_library::load(),
            library_search: String::new(),
            library_sort: LibrarySort::RecentlyAdded,
            library_rename_path: None,
            library_rename_buffer: String::new(),
            library_remove_path: None,
            current_library_path: None,
            library_selected_path: None,
            completion_saved: false,
            analysis_track_path: None,
            analysis_session_id: None,
            progress_track_path: None,
            progress_data: None,
            notation_score: None,
            notation_zoom: 1.0,
            notation_follow_playback: false,
            notation_follow_suspended_until: None,
            practice_visual: if practice_settings.practice_view == "sheet_music" {
                PracticeVisual::SheetMusic
            } else {
                PracticeVisual::Timeline
            },
            audio_output,
            audio_device_refresh,
            manual_audio_refresh_pending: false,
            last_audio_output_refresh: Instant::now(),
            backing_audio,
            audio_sync_offset_ms: practice_settings.audio_sync_offset_ms,
            midi_input_latency_ms: practice_settings.midi_input_latency_ms,
            start_audio_with_practice: practice_settings.start_audio_with_practice,
            theme_preference: practice_settings.theme,
            accent_colour: practice_settings.accent_colour,
            ui_scale: practice_settings.ui_scale,
            reset_confirmation: preferences_ui::ResetConfirmation::default(),
        }
    }

    fn drain_events(&mut self) {
        let new_events = self.backend.poll_events();
        let observed_at_micros = self.clock.now_micros();
        self.forward_events_to_setup_wizard(&new_events);
        for event in new_events {
            if let Some(config) = self.active_config.as_ref().or(self.mapping_draft.as_ref()) {
                self.midi_input_resolver.observe(&event, config);
            }
            self.update_mapping_diagnostics(&event);
            self.handle_practice_midi_event(&event, observed_at_micros);
            self.events.insert(0, event);
        }
        self.events.truncate(MAX_MIDI_EVENT_LOG_ENTRIES);
    }

    fn forward_events_to_setup_wizard(&mut self, events: &[MidiEvent]) {
        // While a setup run is active, every incoming MIDI event is also pushed
        // into the wizard so it can spot the note the user hits. The wizard
        // handles Note On/Off filtering and confirmation itself.
        let Some(wizard) = self.wizard.as_mut() else {
            return;
        };
        for event in events {
            if wizard.is_current_step_event(event) {
                self.wizard_last_input = Some(event.clone());
            }
            wizard.handle_event(event);
        }
    }

    fn update_mapping_diagnostics(&mut self, event: &MidiEvent) {
        let Some(_) = ObservedMidiInput::from_event(event) else {
            return;
        };
        self.mapping_last_input = Some(event.clone());
        self.mapping_last_detected_component = self
            .active_config
            .as_ref()
            .and_then(|config| self.midi_input_resolver.resolve(event, config));
        if event.message.kind == drums_core::MessageKind::NoteOn {
            self.mapping_last_note_component = self.mapping_last_detected_component;
        }
    }

    fn handle_practice_midi_event(&mut self, event: &MidiEvent, observed_at_micros: u64) {
        let Some(config) = self.active_config.as_ref() else {
            return;
        };
        if let Some(session) = self.practice_session.as_mut() {
            session.update_hi_hat_pedal_state(event, config);
        }
        let Some(transport) = self.transport.as_ref() else {
            return;
        };
        let Some(mut hit) = ActualHit::from_resolved_midi_event_observed_at(
            event,
            self.midi_input_resolver.resolve(event, config),
            transport,
            observed_at_micros,
        ) else {
            return;
        };
        hit.elapsed_seconds =
            drums_core::compensate_midi_input(hit.elapsed_seconds, self.midi_input_latency_ms);
        hit.transport_tick = transport.effective_tick_at_seconds(hit.elapsed_seconds);
        self.last_live_hit = Some(hit.clone());
        if transport.is_playing() && !self.count_in_active {
            if let Some(session) = self.practice_session.as_mut() {
                session.record_hit(hit, transport);
            }
        }
    }

    fn refresh_devices(&mut self) {
        match self.backend.list_ports() {
            Ok(ps) => {
                self.ports = ps;
                if self.ports.len() == 1 {
                    self.selected = Some(self.ports[0].index);
                } else if self
                    .selected
                    .is_some_and(|selected| !self.ports.iter().any(|port| port.index == selected))
                {
                    self.selected = None;
                }
            }
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
                let (stored, mapping_load_error) = match self.store.load(&name) {
                    Ok(config) => (config.filter(DrumKitConfiguration::is_usable), None),
                    Err(error) => (None, Some(error)),
                };
                self.active_config = stored.clone();
                self.midi_input_resolver.reset();
                self.mapping_last_note_component = None;
                self.mapping_draft = None;
                self.status = if let Some(error) = mapping_load_error {
                    format!(
                        "Connected to {name}, but its saved drum mapping could not be loaded: {error}"
                    )
                } else {
                    match &stored {
                        Some(cfg) => format!(
                            "Connected to {name}. Restored mapping: {} of {} drums configured.",
                            cfg.mapped_component_count(),
                            SUPPORTED_COMPONENTS.len()
                        ),
                        None => format!(
                            "Connected to {name}. No saved mapping yet - run the Setup Wizard."
                        ),
                    }
                };
                if stored.is_none() {
                    let mut wizard = SetupWizard::new();
                    wizard.start_after(self.clock.now_micros());
                    self.wizard = Some(wizard);
                    self.wizard_device = Some(name);
                    self.wizard_last_input = None;
                    self.view = View::Setup;
                }
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
        self.midi_input_resolver.reset();
        self.mapping_last_note_component = None;
        self.wizard = None;
        self.wizard_device = None;
        self.wizard_last_input = None;
        self.mapping_draft = None;
        self.mapping_last_input = None;
        self.mapping_last_detected_component = None;
        self.last_live_hit = None;
        self.status = "Disconnected.".to_string();
    }

    fn import_midi_clicked(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MIDI files", &["mid", "midi"])
            .set_title("Import MIDI file")
            .pick_file()
        else {
            return;
        };
        match MidiFile::from_path(&path) {
            Ok(file) => {
                let library_path = path.to_string_lossy().into_owned();
                if !self
                    .track_library
                    .iter()
                    .any(|entry| entry.path == library_path)
                {
                    self.track_library.push(LibraryEntry {
                        path: library_path,
                        display_name: file.filename.clone(),
                        bpm: file.tempo_events.first().map(|tempo| tempo.bpm()),
                        time_signature: file
                            .time_signature_events
                            .first()
                            .map(|value| format!("{}/{}", value.numerator, value.denominator())),
                        duration_seconds: file.duration_seconds,
                        note_count: file.tracks.iter().map(|track| track.note_event_count).sum(),
                        likely_drums: file.tracks.iter().any(|track| track.likely_drum),
                        added_at: track_library::now_seconds(),
                        last_practiced_at: None,
                        last_accuracy: None,
                    });
                    self.save_track_library();
                }
                self.library_selected_path = Some(path.to_string_lossy().into_owned());
                self.midi_import_error = None;
                self.view = View::TrackLibrary;
            }
            Err(error) => {
                self.midi_import_error = Some(error.to_string());
                self.view = View::TrackLibrary;
            }
        }
    }

    fn rebuild_timeline(&mut self) {
        self.active_session_recording = None;
        let (Some(file), Some(track), Some(transport)) = (
            &self.imported_midi,
            self.selected_drum_track,
            &self.transport,
        ) else {
            self.timeline = None;
            self.practice_session = None;
            self.expected_timeline_drums = midi_file_ui::ExpectedTimelineDrums::default();
            self.displayed_practice_result = None;
            self.displayed_result_since = None;
            self.observed_practice_result_count = 0;
            return;
        };
        let notes = file.expected_notes_for_track(track, self.active_config.as_ref());
        self.expected_timeline_drums = midi_file_ui::ExpectedTimelineDrums::from_expected(&notes);
        self.notation_score = drums_core::DrumNotationScore::from_expected_notes(file, &notes);
        let suggestion = suggest_subdivision(file.timing, &notes);
        self.subdivision_suggestion = Some(suggestion);
        if self.subdivision_auto {
            self.metronome.set_subdivision(suggestion.subdivision);
            self.metronome.sync_to_transport(transport);
        }
        self.practice_session = Some(PracticeSession::new(
            notes.clone(),
            MatchingConfig::default(),
        ));
        self.displayed_practice_result = None;
        self.displayed_result_since = None;
        self.observed_practice_result_count = 0;
        self.timeline = Some(Timeline::from_expected_notes(
            notes,
            transport.position_map(),
            file.duration_ticks,
        ));
    }

    fn open_library_entry(&mut self, path: &str) -> Result<(), String> {
        self.backing_audio.unload();
        let file = MidiFile::from_path(path).map_err(|error| error.to_string())?;
        self.selected_drum_track = file.suggested_drum_track();
        self.transport = Some(MusicalTransport::from_midi_file(&file));
        self.imported_midi = Some(file);
        self.current_library_path = Some(path.to_string());
        self.library_selected_path = Some(path.to_string());
        self.completion_saved = false;
        self.active_session_recording = None;
        self.rebuild_timeline();
        self.metronome.reset();
        self.last_transport_update = Instant::now();
        Ok(())
    }

    fn open_sheet_music(&mut self, path: &str) -> Result<(), String> {
        self.open_library_entry(path)?;
        let (Some(file), Some(track)) = (&self.imported_midi, self.selected_drum_track) else {
            return Err("No drum track was found in this MIDI file.".into());
        };
        self.notation_score = drums_core::DrumNotationScore::from_midi(file, track);
        self.view = View::SheetMusic;
        Ok(())
    }

    fn restart_practice(&mut self, play: bool) {
        self.completion_saved = false;
        self.active_session_recording = None;
        self.count_in_active = false;
        let Some(transport) = self.transport.as_mut() else {
            return;
        };
        transport.seek(0);
        self.metronome.reset();
        if let Some(session) = self.practice_session.as_mut() {
            session.on_seek_to_tick(0, transport);
        }
        self.displayed_practice_result = None;
        self.displayed_result_since = None;
        self.observed_practice_result_count = 0;
        if play {
            if self.count_in_bars > 0 {
                self.count_in_active = true;
                self.count_in_target_tick = 0;
                self.count_in_end_tick = transport
                    .position_map()
                    .tick_at_measure(self.count_in_bars as u64 + 1, transport.final_tick());
            }
            transport.play();
        }
        self.last_transport_update = Instant::now();
        if play && self.start_audio_with_practice && self.backing_audio.is_loaded() {
            if self.count_in_active {
                self.backing_audio.pause();
            } else {
                let p = drums_core::audio_position(
                    0.0,
                    self.audio_sync_offset_ms,
                    self.backing_audio.duration(),
                );
                let _ = self.backing_audio.seek(p);
                self.backing_audio.play();
            }
        } else {
            self.backing_audio.stop();
        }
    }

    fn stop_practice(&mut self) {
        let Some(transport) = self.transport.as_mut() else {
            return;
        };
        transport.stop();
        self.metronome.reset();
        if let Some(session) = self.practice_session.as_mut() {
            session.on_stop();
        }
        self.displayed_practice_result = None;
        self.displayed_result_since = None;
        self.observed_practice_result_count = 0;
        self.active_session_recording = None;
        self.backing_audio.stop();
    }

    fn begin_session_recording_if_needed(&mut self) {
        if self.active_session_recording.is_some() || self.count_in_active {
            return;
        }
        let (Some(transport), Some(track_id)) =
            (self.transport.as_ref(), self.current_library_path.clone())
        else {
            return;
        };
        if !transport.is_playing() {
            return;
        }
        let started_at_unix_ms = unix_time_millis();
        let (numerator, denominator) = transport.position_map().time_signature_at_tick(0);
        self.active_session_recording = Some(ActiveSessionRecording {
            id: new_session_id(&track_id, started_at_unix_ms),
            track_id,
            started_at_unix_ms,
            practice_bpm: transport.practice_starting_bpm(),
            time_signature: format!("{numerator}/{denominator}"),
            subdivision: self.metronome.subdivision(),
        });
    }

    fn persist_completed_session(&mut self) {
        if self.completion_saved {
            return;
        }
        let Some(recording) = self.active_session_recording.take() else {
            self.status =
                "Completed practice was not stored because no active session was recorded.".into();
            self.completion_saved = true;
            return;
        };
        let (Some(session), Some(transport)) = (&self.practice_session, &self.transport) else {
            self.status =
                "Completed practice was not stored because session data was unavailable.".into();
            self.completion_saved = true;
            return;
        };
        let completed_at_unix_ms = unix_time_millis();
        let completed = CompletedPracticeSession::from_practice_session(
            session,
            transport,
            SessionBuildMetadata {
                id: recording.id,
                track_id: recording.track_id.clone(),
                started_at_unix_ms: recording.started_at_unix_ms,
                completed_at_unix_ms,
                practice_bpm: recording.practice_bpm,
                time_signature: recording.time_signature,
                subdivision: recording.subdivision,
            },
        );
        let accuracy = completed.statistics.accuracy;
        match self.session_store.append(completed) {
            Ok(()) => {
                if let Some(entry) = self
                    .track_library
                    .iter_mut()
                    .find(|entry| entry.path == recording.track_id)
                {
                    entry.last_accuracy = Some(accuracy);
                    entry.last_practiced_at = Some(completed_at_unix_ms / 1_000);
                    self.save_track_library();
                }
                self.status = format!("Practice session saved ({accuracy:.1}% accuracy).");
            }
            Err(error) => {
                self.status = format!("Could not save completed practice session: {error}");
            }
        }
        self.completion_saved = true;
    }

    fn active_loop_region(&self) -> Option<(u64, u64)> {
        (self.practice_loop_mode != PracticeLoopMode::Off
            && self.practice_loop_start_tick < self.practice_loop_end_tick)
            .then_some((self.practice_loop_start_tick, self.practice_loop_end_tick))
    }

    fn configure_loop(&mut self, mode: PracticeLoopMode) {
        self.practice_loop_mode = mode;
        self.practice_loop_iteration = 1;
        let Some(transport) = self.transport.as_ref() else {
            return;
        };
        let current_bar = transport.musical_position().measure;
        let final_tick = transport.final_tick();
        let last_bar = transport
            .position_map()
            .position_at_tick(final_tick)
            .measure;
        if let Some(bars) = mode.bars() {
            self.practice_loop_start_bar = current_bar;
            self.practice_loop_end_bar = (current_bar + bars).min(last_bar + 1);
        } else if mode == PracticeLoopMode::Custom {
            self.practice_loop_start_bar = self.practice_loop_start_bar.clamp(1, last_bar);
            self.practice_loop_end_bar = self
                .practice_loop_end_bar
                .clamp(self.practice_loop_start_bar + 1, last_bar + 1);
        } else {
            return;
        }
        self.practice_loop_start_tick = transport
            .position_map()
            .tick_at_measure(self.practice_loop_start_bar, final_tick);
        self.practice_loop_end_tick = transport
            .position_map()
            .tick_at_measure(self.practice_loop_end_bar, final_tick);
    }
}
impl eframe::App for DrumApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames = self.frames.wrapping_add(1);

        // Periodic (~1s) disconnect detection. Cheap re-enumeration only.
        if self.frames.is_multiple_of(MIDI_CONNECTION_REFRESH_FRAMES) {
            let was_connected = self.connected;
            let alive = self.backend.refresh_connection();
            if was_connected && !alive {
                self.connected = false;
                self.events.clear();
                self.active_config = None;
                self.midi_input_resolver.reset();
                self.mapping_last_note_component = None;
                self.wizard = None;
                self.wizard_device = None;
                self.mapping_draft = None;
                self.status =
                    "Device disconnected - re-connect or check the cable/port.".to_string();
            }
        }

        self.apply_completed_audio_device_refresh();
        let now = Instant::now();
        if now.duration_since(self.last_audio_output_refresh) >= AUDIO_OUTPUT_REFRESH_INTERVAL {
            self.last_audio_output_refresh = now;
            self.request_audio_output_refresh(false);
        }
        if self.view == View::Practice {
            self.begin_session_recording_if_needed();
        }
        let elapsed = now.duration_since(self.last_transport_update);
        self.last_transport_update = now;
        let mut reached_song_end = false;
        let loop_region = if self.count_in_active {
            None
        } else {
            self.active_loop_region()
        };
        let mut loop_wraps = 0;
        if let Some(transport) = self.transport.as_mut() {
            let was_playing = transport.is_playing();
            loop_wraps = transport.advance_by_looping(elapsed, loop_region);
            if self.count_in_active && transport.current_tick() >= self.count_in_end_tick {
                transport.seek(self.count_in_target_tick.min(i64::MAX as u64) as i64);
                transport.play();
                self.count_in_active = false;
                self.metronome.reset();
                if let Some(session) = self.practice_session.as_mut() {
                    session.reset();
                }
                self.observed_practice_result_count = 0;
                if self.start_audio_with_practice && self.backing_audio.is_loaded() {
                    let position = drums_core::audio_position(
                        transport.elapsed_seconds(),
                        self.audio_sync_offset_ms,
                        self.backing_audio.duration(),
                    );
                    let _ = self.backing_audio.seek(position);
                    self.backing_audio.play();
                }
            }
            reached_song_end = was_playing
                && !transport.is_playing()
                && transport.current_tick() == transport.final_tick();
            if loop_wraps > 0 {
                if self.start_audio_with_practice && self.backing_audio.is_loaded() {
                    let position = drums_core::audio_position(
                        transport.elapsed_seconds(),
                        self.audio_sync_offset_ms,
                        self.backing_audio.duration(),
                    );
                    let _ = self.backing_audio.seek(position);
                }
                self.metronome.reset();
                if self.metronome_enabled {
                    for event in self.metronome.observe(transport) {
                        let _ = self.audio_output.play_click(event.kind);
                    }
                } else {
                    self.metronome.sync_to_transport(transport);
                }
            } else if self.metronome_enabled || self.count_in_active {
                for event in self.metronome.observe(transport) {
                    let _ = self.audio_output.play_click(event.kind);
                }
            } else {
                self.metronome.sync_to_transport(transport);
            }
            if transport.is_playing() {
                // Rendering is not the clock: this only schedules a future
                // frame so the elapsed monotonic-time transport can be shown.
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }

        // Sample live hits only after bringing the single musical transport up
        // to date for this frame, avoiding a one-frame-old transport tick.
        if loop_wraps > 0 {
            if let (Some(session), Some((start, end))) = (&mut self.practice_session, loop_region) {
                session.on_loop_wrap(start, end);
                self.practice_loop_iteration += loop_wraps;
            }
        }
        self.drain_events();
        if self.view == View::Practice && !self.count_in_active {
            if let (Some(session), Some(transport)) = (&mut self.practice_session, &self.transport)
            {
                session.advance_to_tick(transport.current_tick(), transport);
                if reached_song_end {
                    session.finish();
                }
            }
        }
        if let Some(session) = &self.practice_session {
            let result_count = session.results().len();
            if result_count < self.observed_practice_result_count {
                self.displayed_practice_result = None;
                self.displayed_result_since = None;
            } else if result_count > self.observed_practice_result_count {
                self.displayed_practice_result = session.results().last().cloned();
                self.displayed_result_since = Some(now);
            }
            self.observed_practice_result_count = result_count;
        }
        if reached_song_end && self.view == View::Practice {
            self.backing_audio.stop();
            self.persist_completed_session();
        }

        let mut import_midi = false;
        let mut restart = false;
        let mut start_resume = false;
        let mut stop = false;
        let mut go_tracks = false;
        let mut refresh_midi_devices = false;
        let mut connect_midi_device = false;
        let mut disconnect_midi_device = false;
        egui::TopBottomPanel::top("application_menu")
            .frame(egui::Frame::default()
                .fill(ctx.style().visuals.window_fill)
                .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_gray(55)))
                .inner_margin(egui::Margin::symmetric(10, 5)))
            .show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("MI Drums")
                        .strong()
                        .size(20.0),
                );
                ui.separator();
                ui.menu_button("File", |ui| {
                    if ui.button("Import MIDI...").clicked() {
                        import_midi = true;
                        ui.close_menu();
                    }
                    if ui.button("Track Library").clicked() {
                        go_tracks = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Practice", |ui| {
                    let available = self.transport.is_some() && self.practice_session.is_some();
                    if ui
                        .add_enabled(available, egui::Button::new("Practice Timeline"))
                        .clicked()
                    {
                        self.practice_visual = PracticeVisual::Timeline;
                        let _ = self.save_practice_settings();
                        self.view = View::Practice;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.current_library_path.is_some(),
                            egui::Button::new("Performance Analysis"),
                        )
                        .clicked()
                    {
                        self.analysis_track_path = self.current_library_path.clone();
                        self.analysis_session_id = None;
                        self.view = View::PerformanceAnalysis;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.current_library_path.is_some(),
                            egui::Button::new("Progress"),
                        )
                        .clicked()
                    {
                        if let Some(track) = self.current_library_path.clone() {
                            match self.session_store.load_for_track(&track) {
                                Ok(sessions) => {
                                    self.progress_data = Some(
                                        drums_core::TrackProgress::from_sessions(&sessions),
                                    );
                                    self.progress_track_path = Some(track);
                                    self.view = View::Progress;
                                }
                                Err(error) => {
                                    self.status = format!("Could not load progress: {error}")
                                }
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(available, egui::Button::new("Restart"))
                        .clicked()
                    {
                        restart = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(available, egui::Button::new("Start / Resume"))
                        .clicked()
                    {
                        start_resume = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(available, egui::Button::new("Stop"))
                        .clicked()
                    {
                        stop = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Track Library").clicked() {
                        go_tracks = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Settings", |ui| {
                    ui.strong("View");
                    let previous_view = self.practice_visual;
                    ui.selectable_value(
                        &mut self.practice_visual,
                        PracticeVisual::Timeline,
                        "Timeline",
                    );
                    ui.selectable_value(
                        &mut self.practice_visual,
                        PracticeVisual::SheetMusic,
                        "Sheet Music",
                    );
                    if self.practice_visual != previous_view {
                        let _ = self.save_practice_settings();
                    }
                    ui.separator();
                    ui.strong("Practice / Timing");
                    let mut timing_settings_changed = false;
                    ui.horizontal(|ui| {
                        ui.label("MIDI Input Latency").on_hover_text(
                            "Compensates for delay between a physical drum hit and MIDI reception.",
                        );
                        timing_settings_changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.midi_input_latency_ms)
                                    .range(0.0..=500.0)
                                    .suffix(" ms"),
                            )
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Audio Sync Offset").on_hover_text(
                            "Shifts backing-track playback relative to the MIDI timeline.",
                        );
                        timing_settings_changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.audio_sync_offset_ms)
                                    .range(-1000.0..=1000.0)
                                    .suffix(" ms"),
                            )
                            .changed();
                    });
                    if timing_settings_changed {
                        let _ = self.save_practice_settings();
                        self.resync_backing_audio();
                    }
                    ui.weak("Timing tolerance remains the note-matching window; these values do not change it.");
                    ui.separator();
                    if ui.button("MIDI Monitor").clicked() {
                        self.navigate_away_from_practice(View::Monitor);
                        ui.close_menu();
                    }
                    if ui.button("Setup Wizard").clicked() {
                        self.navigate_away_from_practice(View::Setup);
                        ui.close_menu();
                    }
                    if ui.button("Drum Mapping").clicked() {
                        self.navigate_away_from_practice(View::Mapping);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Preferences").clicked() {
                        self.navigate_away_from_practice(View::Preferences);
                        ui.close_menu();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.connected {
                        if ui.button("Disconnect").clicked() {
                            disconnect_midi_device = true;
                        }
                        ui.colored_label(
                            egui::Color32::from_rgb(90, 210, 135),
                            egui::RichText::new("Connected").strong(),
                        );
                    } else {
                        if ui
                            .add_enabled(
                                self.selected.is_some() && !self.ports.is_empty(),
                                egui::Button::new("Connect"),
                            )
                            .clicked()
                        {
                            connect_midi_device = true;
                        }
                        ui.strong("No device connected");
                    }
                    let selected_text = if self.connected {
                        self.backend
                            .connected_port_name()
                            .unwrap_or("MIDI device")
                            .to_string()
                    } else {
                        self.ports
                            .iter()
                            .find(|port| Some(port.index) == self.selected)
                            .map(|port| port.name.clone())
                            .unwrap_or_else(|| {
                                if self.ports.is_empty() {
                                    "No MIDI devices found".to_string()
                                } else {
                                    "Select MIDI device".to_string()
                                }
                            })
                    };
                    egui::ComboBox::from_id_salt("header_midi_device")
                        .width(220.0)
                        .selected_text(format!("MIDI: {selected_text}"))
                        .show_ui(ui, |ui| {
                            if self.ports.is_empty() {
                                ui.add_enabled(false, egui::Label::new("No MIDI devices found"));
                            }
                            for port in &self.ports {
                                ui.selectable_value(
                                    &mut self.selected,
                                    Some(port.index),
                                    &port.name,
                                );
                            }
                            ui.separator();
                            if ui.button("Refresh Devices").clicked() {
                                refresh_midi_devices = true;
                                ui.close_menu();
                            }
                        });
                });
            });
            ui.add_space(5.0);
        });
        if !self.connected {
            egui::TopBottomPanel::top("midi_disconnected_warning")
                .frame(
                    egui::Frame::default()
                        .fill(egui::Color32::from_rgb(38, 33, 24))
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            egui::Color32::from_rgb(105, 86, 48),
                        ))
                        .inner_margin(egui::Margin::symmetric(10, 4)),
                )
                .show(ctx, |ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 190, 105),
                        "MIDI DEVICE DISCONNECTED — connect a device for live drum input.",
                    );
                });
        }
        if import_midi {
            self.import_midi_clicked();
        }
        if restart {
            self.restart_practice(true);
            self.view = View::Practice;
        }
        if start_resume {
            if let Some(transport) = self.transport.as_mut() {
                transport.play();
                if self.start_audio_with_practice && self.backing_audio.is_loaded() {
                    let p = drums_core::audio_position(
                        transport.elapsed_seconds(),
                        self.audio_sync_offset_ms,
                        self.backing_audio.duration(),
                    );
                    let _ = self.backing_audio.seek(p);
                    self.backing_audio.play();
                }
            }
            self.view = View::Practice;
        }
        if stop {
            self.stop_practice();
        }
        if go_tracks {
            self.navigate_away_from_practice(View::TrackLibrary);
        }
        if refresh_midi_devices {
            self.refresh_devices();
        }
        if disconnect_midi_device {
            self.disconnect_clicked();
        } else if connect_midi_device {
            self.connect_clicked();
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                if self.view == View::Practice {
                    if self.practice_visual == PracticeVisual::SheetMusic {
                        midi_file_ui::show_practice_persistent_header(self, ui);
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt("sheet_music_practice_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| midi_file_ui::show_practice_sheet_content(self, ui));
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("practice_page_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| midi_file_ui::show_practice(self, ui));
                    }
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if matches!(
                                self.view,
                                View::Monitor | View::Setup | View::Mapping | View::Preferences
                            ) && settings_back_button(self, ui)
                            {
                                return;
                            }
                            match self.view {
                                View::Monitor => monitor_ui::show(self, ui),
                                View::Setup => wizard_ui::show(self, ui),
                                View::Mapping => mapping_ui::show(self, ui),
                                View::TrackLibrary => midi_file_ui::show(self, ui),
                                View::PerformanceAnalysis => {
                                    performance_analysis_ui::show(self, ui)
                                }
                                View::Progress => progress_ui::show(self, ui),
                                View::SheetMusic => notation_ui::show(self, ui),
                                View::Preferences => preferences_ui::show(self, ui),
                                View::Practice => unreachable!(),
                            }
                        });
                }
            });

        // This second check covers a Play click made during the UI pass above.
        // The transport itself remains driven solely by elapsed monotonic time.
        if self.backing_audio.is_playing()
            || self
                .transport
                .as_ref()
                .is_some_and(|transport| transport.is_playing())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        ctx.request_repaint_after(AUDIO_OUTPUT_REFRESH_INTERVAL);
    }
}

fn settings_back_button(app: &mut DrumApp, ui: &mut egui::Ui) -> bool {
    let has_practice = app.transport.is_some() && app.practice_session.is_some();
    let label = if has_practice {
        "Back to Practice"
    } else {
        "Your Tracks"
    };
    if ui.button(label).clicked() {
        app.view = if has_practice {
            View::Practice
        } else {
            View::TrackLibrary
        };
        true
    } else {
        ui.add_space(8.0);
        false
    }
}

fn accent_color(preference: AccentColour) -> egui::Color32 {
    let (red, green, blue) = preference.rgb();
    egui::Color32::from_rgb(red, green, blue)
}

fn apply_app_appearance(
    ctx: &egui::Context,
    theme: ThemePreference,
    accent: AccentColour,
    ui_scale: UiScale,
) {
    ctx.set_theme(match theme {
        ThemePreference::System => egui::ThemePreference::System,
        ThemePreference::Dark => egui::ThemePreference::Dark,
        ThemePreference::Light => egui::ThemePreference::Light,
    });
    ctx.set_zoom_factor(ui_scale.zoom_factor());
    let accent = accent_color(accent);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(220, 222, 218));
        style.visuals.panel_fill = egui::Color32::from_rgb(27, 27, 27);
        style.visuals.window_fill = egui::Color32::from_rgb(24, 24, 24);
        style.visuals.faint_bg_color = egui::Color32::from_rgb(22, 25, 29);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(17, 22, 29);
        style.visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(55));
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(55, 55, 55);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(68, 68, 68);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(76, 76, 76);
        configure_shared_style(style, accent);
    });
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = egui::Color32::from_rgb(242, 243, 245);
        style.visuals.window_fill = egui::Color32::from_rgb(248, 248, 249);
        style.visuals.faint_bg_color = egui::Color32::from_rgb(232, 235, 239);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(220, 224, 230);
        configure_shared_style(style, accent);
    });
}

fn configure_shared_style(style: &mut egui::Style, accent: egui::Color32) {
    style.spacing.interact_size = egui::vec2(40.0, 28.0);
    style.spacing.button_padding = egui::vec2(9.0, 5.0);
    style.spacing.item_spacing = egui::vec2(7.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(10);
    style.visuals.selection.bg_fill = accent;
    style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, accent);
    style.visuals.hyperlink_color = accent;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, accent);
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, accent);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(21.0));
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn sole_port_index(ports: &[PortInfo]) -> Option<usize> {
    match ports {
        [only_port] => Some(only_port.index),
        _ => None,
    }
}

fn needs_initial_setup(configs: &[DrumKitConfiguration]) -> bool {
    !configs.iter().any(DrumKitConfiguration::is_usable)
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use drums_core::mapping::{DrumComponent, DrumMapping};

    #[test]
    fn midi_port_auto_selection_is_safe_for_empty_and_ambiguous_lists() {
        let port = |index, name: &str| PortInfo {
            index,
            name: name.to_string(),
        };

        assert_eq!(sole_port_index(&[]), None);
        assert_eq!(sole_port_index(&[port(7, "only")]), Some(7));
        assert_eq!(
            sole_port_index(&[port(1, "first"), port(2, "second")]),
            None
        );
    }

    #[test]
    fn first_launch_requires_setup_until_a_playable_mapping_exists() {
        assert!(needs_initial_setup(&[]));

        let mut cc_only = DrumKitConfiguration::new("controller only");
        cc_only
            .add_input(DrumMapping {
                component: DrumComponent::HiHatPedalPosition,
                event_kind: drums_core::MappingEventKind::ControlChange,
                midi_note: 19,
                channel: 10,
                articulation: None,
                cc_range: Some(drums_core::mapping::CcRange::new(0, 127)),
            })
            .unwrap();
        assert!(needs_initial_setup(&[cc_only]));

        let mut playable = DrumKitConfiguration::new("kit");
        playable
            .add_input(DrumMapping {
                component: DrumComponent::Snare,
                event_kind: drums_core::MappingEventKind::Note,
                midi_note: 38,
                channel: 10,
                articulation: None,
                cc_range: None,
            })
            .unwrap();
        assert!(!needs_initial_setup(&[playable]));
    }
}
