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

mod click_audio;
mod backing_audio;
mod mapping_ui;
mod midi_file_ui;
mod performance_analysis_ui;
mod progress_ui;
mod practice_settings;
mod track_library;
mod wizard_ui;

use std::sync::Arc;
use std::time::Instant;

use drums_core::mapping::{DrumKitConfiguration, MappingStore, SUPPORTED_COMPONENTS, SetupWizard};
use drums_core::time::{SharedClock, SystemClock};
use drums_core::{
    ActualHit, CompletedPracticeSession, MatchingConfig, Metronome, MidiBackend, MidiEvent,
    MidiFile, MusicalTransport, PortInfo, PracticeResult, PracticeSession, PracticeSessionStore,
    SessionBuildMetadata, Subdivision, SubdivisionSuggestion, Timeline, new_session_id,
    suggest_subdivision,
};
use eframe::egui;
use track_library::{LibraryEntry, LibrarySort};

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
            .with_title("MIDI Drum Practice - Phase 5A"),
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
    /// Setup wizard state, present while a setup run is in progress.
    wizard: Option<SetupWizard>,
    /// Name of the device the running wizard was started against.
    wizard_device: Option<String>,
    wizard_last_input: Option<MidiEvent>,
    /// Working copy edited in the manual mapping view.
    mapping_draft: Option<DrumKitConfiguration>,
    mapping_assign_component: drums_core::DrumComponent,
    mapping_reassign_component: Option<drums_core::DrumComponent>,
    mapping_reassign_after_micros: u64,
    mapping_reassign_candidate: Option<MidiEvent>,
    mapping_reassign_started: Option<Instant>,
    /// Last meaningful hit shown by Drum Mapping; Note Off never replaces it.
    mapping_last_input: Option<MidiEvent>,
    mapping_last_detected_component: Option<drums_core::DrumComponent>,
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
    click_audio: click_audio::ClickAudio,
    backing_audio: backing_audio::BackingAudio,
    audio_sync_offset_ms: f64,
    midi_input_latency_ms: f64,
    start_audio_with_practice: bool,
}

impl DrumApp {
    fn save_practice_settings(&self) {
        let _ = practice_settings::save(&practice_settings::PracticeSettings { midi_input_latency_ms:self.midi_input_latency_ms, audio_sync_offset_ms:self.audio_sync_offset_ms, start_audio_with_practice:self.start_audio_with_practice });
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

        set_app_style(&ctx.egui_ctx);

        Self {
            clock,
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
            wizard_last_input: None,
            mapping_draft: None,
            mapping_assign_component: drums_core::DrumComponent::HiHatPedal,
            mapping_reassign_component: None,
            mapping_reassign_after_micros: 0,
            mapping_reassign_candidate: None,
            mapping_reassign_started: None,
            mapping_last_input: None,
            mapping_last_detected_component: None,
            view: View::TrackLibrary,
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
            practice_lookahead_seconds: 4.0,
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
            click_audio: click_audio::ClickAudio::new(),
            backing_audio: backing_audio::BackingAudio::new(),
            audio_sync_offset_ms: practice_settings.audio_sync_offset_ms,
            midi_input_latency_ms: practice_settings.midi_input_latency_ms,
            start_audio_with_practice: practice_settings.start_audio_with_practice,
        }
    }

    fn drain_events(&mut self) {
        let new_events = self.backend.poll_events();
        let observed_at_micros = self.clock.now_micros();
        // While a setup run is active, every incoming MIDI event is also pushed
        // into the wizard so it can spot the note the user hits. The wizard
        // handles Note On/Off filtering and confirmation itself.
        if self.wizard.is_some() {
            for ev in &new_events {
                if self
                    .wizard
                    .as_ref()
                    .is_some_and(|wizard| wizard.is_current_step_event(ev))
                {
                    self.wizard_last_input = Some(ev.clone());
                }
                if let Some(wizard) = self.wizard.as_mut() {
                    wizard.handle_event(ev);
                }
            }
        }
        for ev in new_events {
            if (ev.message.kind == drums_core::MessageKind::NoteOn
                && ev.message.velocity().unwrap_or(0) > 0)
                || ev.message.kind == drums_core::MessageKind::ControlChange
            {
                self.mapping_last_input = Some(ev.clone());
                self.mapping_last_detected_component = self.active_config.as_ref().and_then(|config| {
                    let channel = ev.message.channel_number()?;
                    let (kind, number) = match ev.message.kind {
                        drums_core::MessageKind::NoteOn => (
                            drums_core::mapping::MappingEventKind::Note,
                            ev.message.note()?,
                        ),
                        drums_core::MessageKind::ControlChange => (
                            drums_core::mapping::MappingEventKind::ControlChange,
                            ev.message.data1?,
                        ),
                        _ => return None,
                    };
                    config.mappings.iter().find_map(|mapping| {
                        (mapping.event_kind == kind
                            && mapping.midi_note == number
                            && mapping.channel == channel)
                            .then_some(mapping.component)
                    })
                });
            }
            if let (Some(config), Some(transport)) = (&self.active_config, &self.transport) {
                if let Some(mut hit) = ActualHit::from_midi_event_observed_at(
                    &ev,
                    config,
                    transport,
                    observed_at_micros,
                ) {
                    hit.elapsed_seconds = drums_core::compensate_midi_input(hit.elapsed_seconds, self.midi_input_latency_ms);
                    hit.transport_tick = transport.effective_tick_at_seconds(hit.elapsed_seconds);
                    self.last_live_hit = Some(hit.clone());
                    if transport.is_playing() && !self.count_in_active {
                        if let Some(session) = self.practice_session.as_mut() {
                            session.record_hit(hit, transport);
                        }
                    }
                }
            }
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
                let stored = self
                    .store
                    .load(&name)
                    .unwrap_or(None)
                    .filter(|config| !config.is_empty());
                self.active_config = stored.clone();
                self.mapping_draft = None;
                self.status = match &stored {
                    Some(cfg) => format!(
                        "Connected to {name}. Restored mapping: {} of {} drums configured.",
                        cfg.mapped_count(),
                        SUPPORTED_COMPONENTS.len()
                    ),
                    None => {
                        format!("Connected to {name}. No saved mapping yet - run the Setup Wizard.")
                    }
                };
                self.view = View::TrackLibrary;
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
                    let _ = track_library::save(&self.track_library);
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
            self.displayed_practice_result = None;
            self.displayed_result_since = None;
            self.observed_practice_result_count = 0;
            return;
        };
        let notes = file.expected_notes_for_track(track, self.active_config.as_ref());
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
            if self.count_in_active { self.backing_audio.pause(); } else { let p=drums_core::audio_position(0.0,self.audio_sync_offset_ms,self.backing_audio.duration());let _=self.backing_audio.seek(p);self.backing_audio.play(); }
        } else { self.backing_audio.stop(); }
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
            self.status = "Completed practice was not stored because no active session was recorded.".into();
            self.completion_saved = true;
            return;
        };
        let (Some(session), Some(transport)) = (&self.practice_session, &self.transport) else {
            self.status = "Completed practice was not stored because session data was unavailable.".into();
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
                    let _ = track_library::save(&self.track_library);
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
                    "Device disconnected - re-connect or check the cable/port.".to_string();
            }
        }

        let now = Instant::now();
        self.begin_session_recording_if_needed();
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
                    let position = drums_core::audio_position(transport.elapsed_seconds(), self.audio_sync_offset_ms, self.backing_audio.duration());
                    let _ = self.backing_audio.seek(position);
                    self.backing_audio.play();
                }
            }
            reached_song_end = was_playing
                && !transport.is_playing()
                && transport.current_tick() == transport.final_tick();
            if loop_wraps > 0 {
                if self.start_audio_with_practice && self.backing_audio.is_loaded() {
                    let position = drums_core::audio_position(transport.elapsed_seconds(), self.audio_sync_offset_ms, self.backing_audio.duration());
                    let _ = self.backing_audio.seek(position);
                }
                self.metronome.reset();
                if self.metronome_enabled {
                    for event in self.metronome.observe(transport) {
                        self.click_audio.play(event.kind);
                    }
                } else {
                    self.metronome.sync_to_transport(transport);
                }
            } else if self.metronome_enabled || self.count_in_active {
                for event in self.metronome.observe(transport) {
                    self.click_audio.play(event.kind);
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
        if !self.count_in_active {
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
        if reached_song_end {
            self.backing_audio.stop();
            self.persist_completed_session();
        }

        if false {
            egui::TopBottomPanel::top("legacy_header").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.heading("MIDI Drum Practice");
                ui.label("Phase 2 - Setup wizard & drum mapping");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Refresh devices").clicked() {
                        self.refresh_devices();
                    }

                    let selected_text = self
                        .ports
                        .get(self.selected.unwrap_or(usize::MAX))
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "Select a MIDI input...".to_string());
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
                            .add_enabled(can_connect, egui::Button::new("Connect"))
                            .clicked()
                        {
                            self.connect_clicked();
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.view, View::Monitor, "Monitor");
                    ui.selectable_value(&mut self.view, View::Setup, "Setup Wizard");
                    ui.selectable_value(&mut self.view, View::TrackLibrary, "Import MIDI");
                });
                ui.add_space(4.0);
                if self.connected {
                    let mapping_line = match &self.active_config {
                        Some(cfg) => format!(
                            "Drum mapping for {}: {}/{} components configured.",
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
                    "Available MIDI input devices: {} | {}",
                    self.ports.len(),
                    if self.status.is_empty() {
                        if self.connected {
                            format!(
                                "Listening on {}",
                                self.backend.connected_port_name().unwrap_or("?")
                            )
                        } else {
                            "Idle - select a device and press Connect.".to_string()
                        }
                    } else {
                        self.status.clone()
                    }
                ));
                ui.add_space(4.0);
                ui.separator();
            });
        }

        let mut import_midi = false;
        let mut restart = false;
        let mut start_resume = false;
        let mut stop = false;
        let mut go_tracks = false;
        let mut refresh_midi_devices = false;
        let mut connect_midi_device = false;
        let mut disconnect_midi_device = false;
        egui::TopBottomPanel::top("application_menu").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("MIDI Drum Practice")
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
                    ui.strong("Practice / Timing");
                    let mut timing_settings_changed=false;
                    ui.horizontal(|ui| { ui.label("MIDI Input Latency").on_hover_text("Compensates for delay between a physical drum hit and MIDI reception."); timing_settings_changed|=ui.add(egui::DragValue::new(&mut self.midi_input_latency_ms).range(0.0..=500.0).suffix(" ms")).changed(); });
                    ui.horizontal(|ui| { ui.label("Audio Sync Offset").on_hover_text("Shifts backing-track playback relative to the MIDI timeline."); timing_settings_changed|=ui.add(egui::DragValue::new(&mut self.audio_sync_offset_ms).range(-1000.0..=1000.0).suffix(" ms")).changed(); });
                    if timing_settings_changed { self.save_practice_settings(); if let Some(transport)=self.transport.as_ref(){let p=drums_core::audio_position(transport.elapsed_seconds(),self.audio_sync_offset_ms,self.backing_audio.duration());let _=self.backing_audio.seek(p);} }
                    ui.weak("Timing tolerance remains the note-matching window; these values do not change it.");
                    ui.separator();
                    if ui.button("MIDI Monitor").clicked() {
                        if let Some(transport) = self.transport.as_mut() {
                            transport.pause();
                            self.metronome.sync_to_transport(transport);
                        }
                        self.view = View::Monitor;
                        self.backing_audio.stop();
                        ui.close_menu();
                    }
                    if ui.button("Setup Wizard").clicked() {
                        if let Some(transport) = self.transport.as_mut() {
                            transport.pause();
                            self.metronome.sync_to_transport(transport);
                        }
                        self.view = View::Setup;
                        self.backing_audio.stop();
                        ui.close_menu();
                    }
                    if ui.button("Drum Mapping").clicked() {
                        if let Some(transport) = self.transport.as_mut() {
                            transport.pause();
                            self.metronome.sync_to_transport(transport);
                        }
                        self.view = View::Mapping;
                        self.backing_audio.stop();
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
                if self.start_audio_with_practice && self.backing_audio.is_loaded() { let p=drums_core::audio_position(transport.elapsed_seconds(),self.audio_sync_offset_ms,self.backing_audio.duration());let _=self.backing_audio.seek(p);self.backing_audio.play(); }
            }
            self.view = View::Practice;
        }
        if stop {
            self.stop_practice();
        }
        if go_tracks {
            if let Some(transport) = self.transport.as_mut() {
                transport.pause();
                self.metronome.sync_to_transport(transport);
            }
            self.view = View::TrackLibrary;
            self.backing_audio.stop();
        }
        if refresh_midi_devices {
            self.refresh_devices();
        }
        if disconnect_midi_device {
            self.disconnect_clicked();
        } else if connect_midi_device {
            self.connect_clicked();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.view == View::Practice {
                egui::ScrollArea::vertical()
                    .id_salt("practice_page_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| midi_file_ui::show_practice(self, ui));
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if matches!(self.view, View::Monitor | View::Setup | View::Mapping)
                            && settings_back_button(self, ui)
                        {
                            return;
                        }
                        match self.view {
                            View::Monitor => show_monitor_ui(self, ui),
                            View::Setup => wizard_ui::show(self, ui),
                            View::Mapping => mapping_ui::show(self, ui),
                            View::TrackLibrary => midi_file_ui::show(self, ui),
                            View::PerformanceAnalysis => performance_analysis_ui::show(self, ui),
                            View::Progress => progress_ui::show(self, ui),
                            View::Practice => unreachable!(),
                        }
                    });
            }
        });

        // This second check covers a Play click made during the UI pass above.
        // The transport itself remains driven solely by elapsed monotonic time.
        if self.backing_audio.is_playing() || self
            .transport
            .as_ref()
            .is_some_and(|transport| transport.is_playing())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
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

/// Phase 1 live MIDI monitor view (unchanged behaviour, moved into the tab).
fn show_monitor_ui(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.heading("MIDI Monitor");
    ui.horizontal_wrapped(|ui| {
        if ui.button("Refresh devices").clicked() {
            app.refresh_devices();
        }
        let selected_text = app
            .ports
            .get(app.selected.unwrap_or(usize::MAX))
            .map(|port| port.name.clone())
            .unwrap_or_else(|| "Select a MIDI input...".to_string());
        egui::ComboBox::from_id_salt("monitor_device_combo")
            .width(320.0)
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                if app.ports.is_empty() {
                    ui.add_enabled(false, egui::Label::new("No MIDI devices found"));
                }
                for port in &app.ports {
                    ui.selectable_value(&mut app.selected, Some(port.index), &port.name);
                }
            });
        if app.connected {
            if ui.button("Disconnect").clicked() {
                app.disconnect_clicked();
            }
        } else if ui
            .add_enabled(
                app.selected.is_some() && !app.ports.is_empty(),
                egui::Button::new("Connect"),
            )
            .clicked()
        {
            app.connect_clicked();
        }
    });
    if !app.status.is_empty() {
        ui.weak(&app.status);
    }
    ui.separator();
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
    style.spacing.interact_size = egui::vec2(44.0, 30.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
    ctx.set_style(style);
}

fn fmt_opt(value: Option<u8>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => "N/A".to_string(),
    }
}

fn timestamp_text(us: u64) -> String {
    format!("{} us (~{:.3} s)", us, us as f64 / 1_000_000.0)
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn fmt_event(ev: &MidiEvent) -> String {
    format!(
        "{:>12} us  {:<17} ch={:<4} note={:<5} vel={:<4}",
        ev.timestamp_micros,
        ev.message.kind.name(),
        fmt_opt(ev.message.channel_number()),
        fmt_opt(ev.message.note()),
        fmt_opt(ev.message.velocity()),
    )
}
