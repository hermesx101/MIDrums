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
| `click_audio.rs` | Synthesized metronome click playback through rodio. |
| `backing_audio.rs` | Decoded backing-audio lifecycle, transport controls, seek, volume, and mute. |

## 3. MIDI architecture

`MidiBackend` creates a `midir::MidiInput` and owns at most one connection. Its callback receives arbitrary byte chunks. A connection-local `MidiParser` turns those chunks into complete `MidiEvent` values and stamps them with `Clock::now_micros()`.

The callback sends events over `std::sync::mpsc` and requests an egui repaint. It does not render UI, access files, score hits, or use GUI frame time. `DrumApp::drain_events` non-blockingly drains the queue on the GUI thread and forwards the same events to Setup, mapping diagnostics, pedal-state handling, scoring, and the bounded monitor log.

`MidiMessage` stores raw channel values as 0-15. `channel_number()` exposes the user-facing 1-16 value used in mapping identities and persistence. Note number, velocity, controller number/value, and the monotonic timestamp remain available to downstream consumers.

`ObservedMidiInput::from_event` is the shared filter used by Setup, mapping, diagnostics, and mapping resolution. It accepts positive-velocity Note On and Control Change messages. It rejects Note Off, zero-velocity Note On, and unrelated message kinds.

The application periodically re-enumerates devices and closes a connection whose saved port name is no longer present. This is a best-effort portability mechanism because `midir` does not expose one uniform unplug notification across all backends.

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

The continuous CC path never calls `record_hit`. It therefore cannot create Correct, Extra, or any other scored result and does not suppress a separate discrete pedal Note. The current expected-note model does not use pedal position to infer open/closed articulation; that is an explicit limitation rather than hidden behavior.

Standalone notation recognizes General MIDI 42 as closed hi-hat, 44 as pedal, and 46 as open. The current Practice expected-note conversion groups 42/44/46 as general `HiHat`, so the two paths should not be assumed to have identical articulation detail.

## 6. Setup Wizard

`mapping/wizard.rs` contains the state machine and has no egui dependency. `wizard_ui.rs` renders it and translates button clicks into state transitions.

The wizard progresses through `SUPPORTED_COMPONENTS`. All playable components listen for Note events; `HiHatPedalPosition` listens for CC. It captures the first eligible input, records nearby distinct inputs as possible conflicts, requires confirmation, and applies a short boundary/debounce guard so stale or double-trigger events do not become the next component.

Each accepted component can retain multiple Note aliases. A wizard created with an existing configuration protects its inputs from accidental reuse and preserves skipped baseline components. At completion, the UI builds and validates the configuration, saves it through `MappingStore`, then marks the wizard finished and makes the saved configuration active.

## 7. Practice engine

`MidiFile::expected_notes_for_track` creates the expected side of practice. `DrumApp::rebuild_timeline` uses those same notes for `PracticeSession`, Timeline, notation, and automatic subdivision suggestion.

On the actual side, `DrumApp` first updates continuous hi-hat state for CC events. Positive-velocity Note On events are converted by `ActualHit::from_midi_event_observed_at`. This conversion resolves the event through the active mapping, subtracts measured GUI queue delay, applies the configured MIDI input-latency compensation, and calculates an effective transport tick.

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

## 9. Timeline

`Timeline` in `drums-core` is a small data model: expected events with calculated musical positions plus a final tick. `midi_file_ui.rs` owns painting.

The painter computes shared label/grid geometry, renders fixed logical-drum lanes, a subdivision-aware grid, expected notes, missed-note marks, actual result overlays, loop bounds, a playhead, and the semantic legend. It uses transport time rather than frame count. Recent results are bounded for rendering while complete result history remains in the core session.

## 10. Sheet Music

`notation.rs` converts recognized notes into `DrumNotationScore`: tempos, measures, signatures, quantized slots, rests, rhythmic values, and drum-note chords. This is an intermediate representation; `notation_ui.rs` performs the egui painting.

The standalone view can play/pause, zoom, follow playback, and enter Practice. In Sheet Music Practice, `main.rs` keeps the practice header outside the vertical scroll area. Only notation content scrolls, while transport, session feedback, audio, and metronome controls remain visible. Result overlays are derived from the same `PracticeResult` values used by the Timeline.

## 11. Persistence

All application persistence is local TOML; there is no database.

| Data | Storage | Owner |
| --- | --- | --- |
| Device mappings | `mappings/*.toml`, one file per device | `MappingStore` |
| Completed sessions | `practice-sessions.toml` | `PracticeSessionStore` |
| Track references/metadata | `track-library.toml` | `track_library.rs` |
| Practice/appearance settings | `practice-settings.toml` | `practice_settings.rs` |

Windows normally places these below the user's application-data `drums` directory. HOME-based fallbacks use `.config/drums` for mappings, sessions, and practice settings. The Track Library currently has Windows-oriented application-data resolution and otherwise falls back to the process working directory. `DRUMS_CONFIG_DIR` overrides mapping storage and `DRUMS_HISTORY_PATH` overrides session history, mainly for tests.

Track records contain source paths. They do not own the referenced files. Backing-audio selection is held in memory and is not part of persisted Track Library data.

Session writes report errors to the UI. Mapping loads/saves are validated. Some Track Library and practice-settings call sites deliberately fall back or ignore non-critical errors; this is tracked as technical debt.

## 12. Settings and Preferences

`practice_settings.rs` uses one serde/TOML model for MIDI input latency, audio sync offset, automatic backing-audio start, selected practice visualization, theme, accent color, and UI scale. `#[serde(default)]` keeps older/partial files compatible.

`preferences_ui.rs` edits the appearance fields and calls `apply_app_appearance` in `main.rs`. Theme is applied through egui's system/dark/light selection, scale through the global zoom factor, and the selected accent through shared style colors.

Reset All Data enumerates only known application-owned paths. It removes mapping TOML records plus the session, library, and settings files. On complete success, `DrumApp::restore_first_launch_after_reset` clears in-memory state, restores defaults, and routes to Setup. Deletion is deliberately scoped so source MIDI/audio paths are not targets.

## 13. Testing

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

## 14. Where to make changes

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
| Backing audio or click playback | `crates/drums-app/src/backing_audio.rs`, `click_audio.rs` |
| Navigation and cross-feature orchestration | `crates/drums-app/src/main.rs` |

When changing domain behavior, add or update a `drums-core` test before changing its UI representation. Keep MIDI callback work bounded and keep user-file paths outside application-owned deletion logic.
