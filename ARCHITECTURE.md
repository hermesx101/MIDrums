# MI Drums Architecture

This document explains how the current implementation is divided and how data moves through it. It is intended for developers changing the code, not as a user guide.

## 1. High-level architecture

MI Drums is a Rust workspace with a domain library and a desktop binary:

- `drums-core` owns hardware-independent MIDI parsing, mapping, transport, practice/scoring, notation, history, performance, and progress logic.
- `drums-app` owns the egui/eframe window, navigation, view state, file dialogs, local UI persistence adapters, and audio output.

The principal live-input flow is:

```text
MIDI device
  -> midir callback
  -> raw MIDI bytes
  -> MidiParser
  -> timestamped MidiEvent
  -> GUI-thread event drain
  -> ObservedMidiInput / DrumKitConfiguration
  -> logical DrumComponent
  -> ActualHit
  -> PracticeSession matching
  -> PracticeResult
  -> completed session record
  -> Performance Analysis / Progress
```

The lesson flow is:

```text
MIDI file
  -> midly Standard MIDI File parser
  -> tracks + tempo/time-signature maps + ParsedNote values
  -> likely drum-track selection
  -> normalized ExpectedNote values
  -> Timeline / DrumNotationScore
  -> PracticeSession matching and scoring
```

The GUI is the orchestrator, but scoring and timing rules stay in `drums-core`. Audio follows transport state; it is not the timing authority.

The audio-routing flow is independent of MIDI input:

```text
PracticeSettings
  -> preferred Audio Output name + global volume/mute
  -> background CPAL device snapshot
  -> AudioOutput route and shared Rodio handle
  -> ClickAudio (metronome/test sound)
  -> BackingAudio
```

The persistence/startup flow is:

```text
local TOML files
  -> preferences + mappings + Track Library loaded at startup
  -> Setup page when no usable saved mapping exists; otherwise Track Library
  -> exact device-name mapping restored on connection
  -> completed PracticeSession appended to session history
  -> Performance Analysis and Progress derived from that history
```

## 2. Project structure

```text
Cargo.toml
crates/
  drums-core/
    src/
      lib.rs
      time.rs
      audio_sync.rs
      midi/
        backend.rs
        event.rs
        parser.rs
        file.rs
        transport.rs
        metronome.rs
      mapping/
        mod.rs
        store.rs
        resolver.rs
        wizard.rs
      practice.rs
      performance.rs
      session_history.rs
      progress.rs
      notation.rs
  drums-app/
    src/
      main.rs
      monitor_ui.rs
      wizard_ui.rs
      mapping_ui.rs
      midi_file_ui.rs
      notation_ui.rs
      performance_analysis_ui.rs
      progress_ui.rs
      preferences_ui.rs
      practice_settings.rs
      track_library.rs
      audio_output.rs
      click_audio.rs
      backing_audio.rs
Files/
```

`Files/` contains sample MIDI/audio media for manual use. The automated tests generate their own fixtures and do not depend on those files.

### Core module responsibilities

| Module | Responsibility |
| --- | --- |
| `time.rs` | Shared monotonic clock abstraction used for callback timestamps and deterministic tests. |
| `midi/backend.rs` | MIDI port enumeration, one active connection, callback-thread parsing, event queue, and disconnect polling. |
| `midi/event.rs` | Parsed message/event data model and display helpers. |
| `midi/parser.rs` | Stateful streaming MIDI byte parser. |
| `midi/file.rs` | Standard MIDI File parsing, metadata, likely-drum heuristic, and expected-note conversion. |
| `midi/transport.rs` | Tempo/time-signature conversion, musical position, playback state, looping advancement, and Timeline data. |
| `midi/metronome.rs` | Subdivision suggestion, count grid, and transport-boundary click events. |
| `mapping/mod.rs` | Logical drums, physical input identities, mappings, aliases, conflicts, CC calibration, and resolution. |
| `mapping/store.rs` | Per-device TOML mapping persistence. |
| `mapping/resolver.rs` | Ordered pedal state and optional shared-note hi-hat interpretation. |
| `mapping/wizard.rs` | GUI-independent Setup Wizard state machine. |
| `practice.rs` | Actual-hit conversion, continuous pedal state, expected-target lifecycle, matching, and result creation. |
| `performance.rs` | Live/attempt-level aggregate statistics derived from a `PracticeSession`. |
| `session_history.rs` | Durable completed-session records and TOML store. |
| `progress.rs` | Read-only multi-session progress calculations and threshold-based insights. |
| `notation.rs` | MIDI/expected-note to drum-notation intermediate representation. |
| `audio_sync.rs` | Pure latency, backing-audio position, and looping conversion helpers. |

### Application module responsibilities

| Module | Responsibility |
| --- | --- |
| `main.rs` | `DrumApp` state, eframe update loop, navigation, device/session orchestration, transport advancement, session persistence, and global appearance. |
| `monitor_ui.rs` | Live MIDI Monitor view. |
| `wizard_ui.rs` | Rendering/actions for the core Setup Wizard. |
| `mapping_ui.rs` | Manual mapping editor and live reassignment workflow. |
| `midi_file_ui.rs` | Track Library and the main Practice UI, including Timeline rendering and practice controls. |
| `notation_ui.rs` | Standalone and practice-integrated Sheet Music rendering. |
| `performance_analysis_ui.rs` | Detailed persisted-session analysis. |
| `progress_ui.rs` | Multi-session charts, comparisons, and insights. |
| `preferences_ui.rs` | Dedicated Preferences page and confirmed application-data reset. |
| `practice_settings.rs` | Persisted timing, view, audio-start, theme, accent, and scale settings. |
| `track_library.rs` | Track Library record model and persistence. |
| `audio_output.rs` | Background output-device discovery, preferred/default route selection, global output gain, and shared Rodio handle ownership. |
| `click_audio.rs` | Synthesized metronome click playback through rodio. |
| `backing_audio.rs` | Decoded backing-audio lifecycle, transport controls, seek, volume, and mute. |

## 3. MIDI architecture

`MidiBackend` creates a `midir::MidiInput` and owns at most one connection. Its callback receives arbitrary byte chunks. A connection-local `MidiParser` turns those chunks into complete `MidiEvent` values and stamps them with `Clock::now_micros()`.

The callback sends events over `std::sync::mpsc` and requests an egui repaint. It does not render UI, access files, score hits, or use GUI frame time. `DrumApp::drain_events` non-blockingly drains the queue on the GUI thread and forwards the same events to Setup, mapping diagnostics, pedal-state handling, scoring, and the bounded monitor log.

`MidiMessage` stores raw channel values as 0-15. `channel_number()` exposes the user-facing 1-16 value used in mapping identities and persistence. Note number, velocity, controller number/value, and the monotonic timestamp remain available to downstream consumers.

`ObservedMidiInput::from_event` is the shared filter used by Setup, mapping, diagnostics, and mapping resolution. It accepts positive-velocity Note On and Control Change messages. It rejects Note Off, zero-velocity Note On, and unrelated message kinds.

The application periodically re-enumerates devices and closes a connection whose saved port name is no longer present. This is a best-effort portability mechanism because `midir` does not expose one uniform unplug notification across all backends.

`MessageKind` distinguishes Note On, Note Off, Control Change, program/pitch messages, system
messages, and unknown data. MIDI's Note On with velocity zero convention is normalized as Note Off
by the event helpers and ignored as a playable mapping input. Velocity and CC value remain raw
7-bit values. Monitor rows display events before mapping, making MIDI Monitor the boundary to check
before debugging logical-drum resolution.

## 4. Drum Mapping architecture

The mapping boundary separates physical device data from lesson/scoring identity:

```text
(event kind, note/controller number, MIDI channel)
  -> DrumMapping
  -> DrumComponent
```

`DrumKitConfiguration.mappings` is a vector, not a one-entry-per-drum map. This deliberately permits multiple `DrumMapping` entries with the same component. Those entries are aliases: every alias resolves to the same logical drum and reaches scoring identically.

Input ownership is unique. `add_input` rejects an identity already assigned to another component, while `reassign_input` explicitly transfers it. Validation also rejects duplicate identities, invalid channels, inverted CC ranges, and CC calibration data attached to Note mappings.

Configurations are keyed by exact MIDI device name. The storage layer sanitizes that name for the filename and adds a stable hash to avoid collisions. Reconnecting the same named device loads and validates its configuration.

## 5. Hi-hat architecture

Hi-hat behavior is represented by four logical components:

- `HiHat`: playable closed/general hi-hat Note input
- `HiHatOpen`: playable open hi-hat Note input
- `HiHatPedal`: playable discrete foot/pedal Note input
- `HiHatPedalPosition`: continuous CC input with an observed calibration range

Closed, open, and discrete pedal mappings are ordinary Note aliases and can produce `ActualHit` values. Continuous pedal position is different: Setup or Drum Mapping records the controller number, channel, and observed `CcRange`. `PracticeSession::update_hi_hat_pedal_state` normalizes a matching CC value into `0.0..=1.0` and records the latest raw value/timestamp as `HiHatPedalState`.

The continuous CC path never calls `record_hit`. It therefore cannot create Correct, Extra, or any other scored result and does not suppress a separate discrete pedal Note. For profiles with the optional `hi_hat_pedal` mode, `MidiInputResolver` holds the most recent mapped pedal CC on each channel and classifies shared stick inputs as Closed or Open using a configurable raw threshold and direction. A discrete foot input retains its own component. Missing pedal state produces an unknown stick target; held state does not expire. Reconnect, disconnect, profile save and application reset clear the resolver. The GUI drains events in callback order and resolves each one before scoring; a later CC cannot reclassify an earlier hit.

Standalone notation recognizes General MIDI 42 as closed hi-hat, 44 as pedal, and 46 as open. With `hi_hat_pedal` enabled, Practice preserves those three musical roles and makes both stick targets available from shared inputs. Legacy profiles continue grouping 42/44/46 as general `HiHat`. See [SHARED_HI_HAT.md](SHARED_HI_HAT.md) for setup and the measured V51/VH-14D example.

## 6. Setup Wizard

`mapping/wizard.rs` contains the state machine and has no egui dependency. `wizard_ui.rs` renders it and translates button clicks into state transitions.

The wizard progresses through `SUPPORTED_COMPONENTS`. All playable components listen for Note events; `HiHatPedalPosition` listens for CC. It captures the first eligible input, records nearby distinct inputs as possible conflicts, requires confirmation, and applies a short boundary/debounce guard so stale or double-trigger events do not become the next component.

Each accepted component can retain multiple Note aliases. A wizard created with an existing configuration protects its inputs from accidental reuse and preserves skipped baseline components. At completion, the UI builds and validates the configuration, saves it through `MappingStore`, then marks the wizard finished and makes the saved configuration active.

The default wizard order is Crash, Ride, Hi-Hat, Open Hi-Hat, Hi-Hat Pedal, Hi-Hat Pedal Position,
Snare, Kick, High Tom, Mid Tom, and Low Tom. In shared-note mode, stick zones are learned once at Hi-Hat and the Open Hi-Hat step is skipped; its review uses the same ordered pedal resolver. Playable components detect positive-velocity Note On;
Hi-Hat Pedal Position detects CC and accumulates its observed minimum/maximum for calibration.

```text
Intro
  -> Detecting(component)
  -> Confirming(component)
       -> accept -> Detecting(next component)
       -> accept and add another -> Detecting(same component)
       -> retry -> Detecting(same component)
       -> back -> Detecting(previous component)
       -> skip -> Detecting(next component)
  -> Completed (live test/review)
  -> validate + save -> Track Library

Any active state -> cancel -> Canceled
```

Inputs already owned by another learned or baseline component are retained as rejected diagnostics
rather than reassigned implicitly. The boundary timestamp and debounce interval prevent events from
the prior step or a flam/double trigger from being accepted for the next prompt.

## 7. Practice engine

`MidiFile::expected_notes_for_track` creates the expected side of practice. `DrumApp::rebuild_timeline` uses those same notes for `PracticeSession`, Timeline, notation, and automatic subdivision suggestion.

On the actual side, `DrumApp` first updates continuous hi-hat state for CC events. Positive-velocity Note On events are converted by `ActualHit::from_resolved_midi_event_observed_at` using the logical component from the ordered resolver. The conversion preserves callback timestamps and subtracts measured GUI queue delay; the application then applies MIDI input-latency compensation and recalculates the effective tick. The stateless `from_midi_event_observed_at` entry point remains available for ordinary mappings.

While the transport is playing and count-in is inactive, the hit is passed to `PracticeSession::record_hit`. Separately, transport advancement calls `advance_to_tick` to finalize misses after their late boundary. Stop/seek reset or reposition matching state; loop wraps finalize/rearm the loop region and retain accumulated results.

At song completion, the session is finalized and converted into one `CompletedPracticeSession`. The durable record contains metadata, individual hit/result records, timing offsets, velocity, overall statistics, and per-drum statistics. Progress and Performance Analysis both read that same history.

## 8. Scoring

`TimingConfig` contains the on-time and outer match windows. The current defaults are 50 ms and 100 ms respectively. Tests cover inclusive boundaries and use custom configurations where needed.

Matching considers only unconsumed known expected notes inside the outer window. Candidates are ordered by:

1. same logical component before a wrong component
2. smallest absolute timing difference
3. expected musical tick
4. stable index tie-breaking

A same-component candidate becomes `CorrectHit` with `TimingJudgment::Good`, `Early`, or `Late`. A different component becomes `WrongDrum` and consumes one target. No candidate becomes internal `Ignored` with an actual hit, displayed as Extra. Expected events crossing the late edge become `Missed`. Unknown expected notes are preserved as internal `Ignored` records and excluded from scoring.

`PracticePerformance` derives live counts and accuracy. `CompletedPracticeSession` creates the analysis-ready durable snapshot. Accuracy is correct divided by finalized known targets; extras do not enlarge that denominator.

## 9. MusicalTransport

`MusicalTransport` owns playback state, current/final tick, a `TempoMap`, a
`MusicalPositionMap`, and an optional practice BPM override. Its states are:

- `Stopped`: positioned at tick zero after `stop`; `play` starts advancement.
- `Playing`: `DrumApp::update` advances it from monotonic elapsed wall time.
- `Paused`: retains its current tick; `play` resumes from that position.

`seek` accepts a signed tick, clamps it into the song, and retains the current transport state.
Seeking to the final tick makes a subsequent `play` a defined no-op until repositioned. `stop`
returns to zero. `advance_by_looping` converts elapsed seconds through the active tempo model and,
for a valid loop region, wraps while preserving overshoot and playing state.

The source MIDI tempo map supports tempo changes. A practice BPM override scales musical time
without rewriting MIDI ticks; current BPM and elapsed seconds are derived from the transport.
`MusicalPositionMap` converts ticks into bar, beat, and tick positions using time-signature events.
The UI sends commands and renders transport state, but frame count never determines score timing.
On seek, stop, or loop wrap, `DrumApp` explicitly synchronizes PracticeSession, Metronome, and
backing-audio state.

## 10. Timeline

`Timeline` in `drums-core` is a small data model: expected events with calculated musical positions plus a final tick. `midi_file_ui.rs` owns painting.

The painter computes shared label/grid geometry, renders fixed logical-drum lanes, a subdivision-aware grid, expected notes, missed-note marks, actual result overlays, loop bounds, a playhead, and the semantic legend. It uses transport time rather than frame count. Recent results are bounded for rendering while complete result history remains in the core session.

`DrumApp::rebuild_timeline` creates the core `Timeline` and `PracticeSession` from the same expected
notes. `rhythm_grid` in `midi_file_ui.rs` performs egui painting. Lane and x-position helpers keep
expected notes and actual unquantized hit times aligned to one time window; the playhead comes from
`MusicalTransport`. Horizontal geometry reserves a stable label region before converting time to x
coordinates. Timeline rendering currently performs per-frame preparation and is a known profiling
candidate, but its architecture is intentionally unchanged for v1.0.

## 11. Sheet Music

`notation.rs` converts recognized notes into `DrumNotationScore`: tempos, measures, signatures, quantized slots, rests, rhythmic values, and drum-note chords. This is an intermediate representation; `notation_ui.rs` performs the egui painting.

The standalone view can play/pause, zoom, follow playback, and enter Practice. In Sheet Music Practice, `main.rs` keeps the practice header outside the vertical scroll area. Only notation content scrolls, while transport, session feedback, audio, and metronome controls remain visible. Result overlays are derived from the same `PracticeResult` values used by the Timeline.

Scrolling manually suspends automatic following for three seconds. Follow Playback scrolls the
currently active system into view; it does not own or advance playback. Zoom is bounded to 70-200%.
Notation quantization changes only the derived notation representation, never MIDI source ticks or
the timestamps used by scoring.

## 12. Audio architecture

`AudioOutput` owns the current Rodio `OutputStream`/handle, preferred device name, enumerated device
handles, active route, fallback status, global volume/mute, and `ClickAudio`. `BackingAudio` receives
a clone of the same output handle. Changing the application gain updates both consumers; backing
audio also has independent track volume and mute.

At startup, `AudioOutput` opens the system default endpoint without enumerating every device. One
persistent `AudioDeviceRefreshWorker` owns a bounded request channel and runs CPAL
`output_devices()` calls on its named background thread. It returns an `AudioDeviceSnapshot`
containing display names and device handles and requests an egui repaint. The frame update polls the
result channel without blocking and applies only completed snapshots. Manual refreshes are
coalesced while a request is active; periodic refreshes use the same worker.

This separation is an important invariant: **CPAL device enumeration must not run synchronously on
the egui/UI thread.** Some audio backends or disconnected endpoints can make enumeration slow. A
synchronous call in a frame handler freezes interaction; the current worker architecture prevents
that regression.

Route planning has three states: requested named device, system default, or system-default fallback
while retaining an unavailable named preference. Applying a route change recreates the shared
output and asks `BackingAudio` to use it. Enumeration/open errors are reported through the existing
status/notice path. Test Audio and metronome clicks are synthesized by `ClickAudio`; backing files
are decoded by Rodio and are not persisted with a Track Library entry.

## 13. Track Library and persistence

Track Library entries store source path, display name, imported MIDI metadata, added/last-practiced
time, and last accuracy. Import parses the selected file before adding it and avoids duplicate path
records. Search and sorting operate on an in-memory copy for presentation. Rename and removal change
only library metadata; source MIDI files are never renamed or deleted. Opening an entry reparses its
source path. A missing, moved, unreadable, or invalid source is reported as an import/open error.

All application persistence is local TOML; there is no database.

| Data | Storage | Owner |
| --- | --- | --- |
| Device mappings | `mappings/*.toml`, one file per device | `MappingStore` |
| Completed sessions | `practice-sessions.toml` | `PracticeSessionStore` |
| Track references/metadata | `track-library.toml` | `track_library.rs` |
| Practice/appearance settings | `practice-settings.toml` | `practice_settings.rs` |

Windows normally places these below the user's application-data `drums` directory. HOME-based fallbacks use `.config/drums` for mappings, sessions, and practice settings. The Track Library currently has Windows-oriented application-data resolution and otherwise falls back to the process working directory. `DRUMS_CONFIG_DIR` overrides mapping storage and `DRUMS_HISTORY_PATH` overrides session history, mainly for tests.

Track records contain source paths. They do not own the referenced files. Backing-audio selection is held in memory and is not part of persisted Track Library data.

Session writes and Track Library writes on import/edit/completion report failures in the status UI.
Mapping loads/saves are validated. Preferences are saved from user-facing controls, with dedicated
Preferences edits reporting failures. Practice-settings and Track Library loaders currently return
defaults/empty data if the file cannot be read or parsed; this recovery behavior can conceal a
malformed file and is documented as a limitation rather than changed for v1.0.

## 14. Settings and Preferences

`practice_settings.rs` uses one serde/TOML model for MIDI input latency, audio sync offset,
automatic backing-audio start, Timeline/Sheet Music selection, expected-drums-only Timeline
filtering, preferred output device, application volume/mute, theme, accent colour, and UI scale.
`#[serde(default)]` keeps older/partial files compatible.

`preferences_ui.rs` edits the appearance fields and calls `apply_app_appearance` in `main.rs`. Theme is applied through egui's system/dark/light selection, scale through the global zoom factor, and the selected accent through shared style colors.

Reset All Data enumerates only known application-owned paths. It removes mapping TOML records plus the session, library, and settings files. On complete success, `DrumApp::restore_first_launch_after_reset` clears in-memory state, restores defaults, and routes to Setup. Deletion is deliberately scoped so source MIDI/audio paths are not targets.

Reset requires a modal confirmation. It does not restart the process. After complete deletion the
running app disconnects MIDI, clears mapping/wizard/monitor/practice/library/history-derived state,
unloads backing audio, restores default appearance/timing/audio preferences, requests an audio
refresh, and shows Setup. If any deletion fails, it reports an incomplete reset and does not perform
the in-memory first-launch restoration. Non-TOML files in the mappings directory are left alone.

## 15. Startup and UI ownership

`main` creates the eframe window and `DrumApp::new` loads settings, enumerates MIDI ports, applies
appearance, checks saved mappings, opens initial audio, starts the background device worker, loads
Track Library, and chooses Setup or Track Library. A single discovered MIDI port is selected but
not connected automatically. On connect, the exact device-name mapping is loaded; an absent or
unusable mapping starts Setup immediately.

`DrumApp::update` is the orchestration boundary. It drains MIDI events, applies completed audio
snapshots, advances transport by elapsed monotonic time, handles count-in/loop/metronome/backing
audio synchronization, advances the PracticeSession, persists completion, and delegates rendering
to the selected view module. Avoid moving domain rules into this function: mapping, timing,
matching, notation conversion, statistics, and progress belong in `drums-core`.

## 16. Testing

Tests are colocated with modules under `#[cfg(test)]`.

- `drums-core` contains most behavioral coverage and has no GUI dependency.
- `drums-app` tests persistence adapters, first-launch selection, reset safety, backing-audio failure handling, and pure Timeline/scoring visualization helpers.
- Tests create temporary files/directories where persistence is involved and do not require the sample media directory.

Run the full quality gate:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace --no-fail-fast
```

Physical MIDI/audio behavior and rendered UI interaction still require manual validation.

## 17. Where to make changes

| Change | Start here |
| --- | --- |
| Raw MIDI parsing or message kinds | `crates/drums-core/src/midi/parser.rs`, `midi/event.rs` |
| MIDI ports, callback, or disconnect handling | `crates/drums-core/src/midi/backend.rs` |
| MIDI lesson parsing or drum-track detection | `crates/drums-core/src/midi/file.rs` |
| Drum identities, aliases, conflicts, or resolution | `crates/drums-core/src/mapping/mod.rs` |
| Mapping persistence | `crates/drums-core/src/mapping/store.rs` |
| Setup Wizard state/acceptance rules | `crates/drums-core/src/mapping/wizard.rs` |
| Setup Wizard presentation | `crates/drums-app/src/wizard_ui.rs` |
| Manual Drum Mapping UI | `crates/drums-app/src/mapping_ui.rs` |
| Transport, BPM, bars, seek, or loop mechanics | `crates/drums-core/src/midi/transport.rs` |
| Metronome grid/click generation | `crates/drums-core/src/midi/metronome.rs` |
| Hit conversion, matching, or timing judgments | `crates/drums-core/src/practice.rs` |
| Live aggregate statistics | `crates/drums-core/src/performance.rs` |
| Durable session schema/store | `crates/drums-core/src/session_history.rs` |
| Progress calculations/insights | `crates/drums-core/src/progress.rs` |
| Timeline and main Practice UI | `crates/drums-app/src/midi_file_ui.rs` |
| Notation model | `crates/drums-core/src/notation.rs` |
| Sheet Music rendering | `crates/drums-app/src/notation_ui.rs` |
| Performance or Progress screens | `crates/drums-app/src/performance_analysis_ui.rs`, `progress_ui.rs` |
| Track Library persistence | `crates/drums-app/src/track_library.rs` |
| Preferences schema/application | `crates/drums-app/src/practice_settings.rs`, `preferences_ui.rs`, `main.rs` |
| Audio-device discovery or route selection | `crates/drums-app/src/audio_output.rs` |
| Backing audio or click playback | `crates/drums-app/src/backing_audio.rs`, `click_audio.rs` |
| Navigation and cross-feature orchestration | `crates/drums-app/src/main.rs` |

When changing domain behavior, add or update a `drums-core` test before changing its UI representation. Keep MIDI callback work bounded and keep user-file paths outside application-owned deletion logic.

Also preserve these cross-module invariants:

- `MusicalTransport` and callback-derived timestamps are timing authorities; egui frame count is not.
- Physical MIDI identity must pass through `DrumKitConfiguration` before scoring.
- One logical drum may own several physical aliases, but one physical identity has one owner.
- Continuous pedal CC updates state and does not pass through `record_hit`.
- Timeline and Sheet Music are views over the same transport/session, not separate practice engines.
- Progress and Performance Analysis derive from completed-session history rather than mutable UI state.
- CPAL enumeration stays on `AudioDeviceRefreshWorker`, never in a UI callback.
