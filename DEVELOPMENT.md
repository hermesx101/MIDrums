# Developing MI Drums

This guide covers local setup, the quality gate, and practical debugging. Read
[ARCHITECTURE.md](ARCHITECTURE.md) before changing domain behavior.

## Prerequisites

- A current stable Rust toolchain with Cargo. The workspace uses Rust 2021 and does not declare a
  minimum supported Rust version.
- Git to obtain and version the repository.
- Windows is the currently tested environment.
- Platform support required by eframe (window/rendering), midir (MIDI), rodio/CPAL (audio), and rfd
  (native file dialogs). Exact non-Windows system packages are not specified by this repository.
- A MIDI kit and audio output for hardware/manual testing. Automated tests do not require them.

Install Rust with rustup or another trusted distribution, then verify `rustc --version` and
`cargo --version`.

## Clone

The repository's canonical remote URL is not recorded in the source tree. Substitute the actual URL:

```sh
git clone <repository-url>
cd DRUMS
```

## Build and run

Build the workspace in development mode:

```sh
cargo build --workspace
```

Run the desktop application:

```sh
cargo run -p drums-app
```

The application package is `drums-app`; `drums-core` is a library and has no standalone executable.

## Tests and code quality

Apply formatting:

```sh
cargo fmt --all
```

Verify formatting without modifying files:

```sh
cargo fmt --check
```

Run all workspace tests and doc tests:

```sh
cargo test --workspace
```

Run strict linting for every target and enabled feature combination:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Build the optimized release executable:

```sh
cargo build --release
```

On Windows the resulting executable is `target/release/drums-app.exe`. `Cargo.lock` is committed
because this workspace produces an application; keep it synchronized with the manifests.

## Project structure

```text
Cargo.toml                         workspace and shared dependency versions
crates/drums-core/src/             hardware-independent domain library
  midi/                            device backend, parser, file import, transport, metronome
  mapping/                         logical drums, mapping store, Setup Wizard state machine
  practice.rs                     hit conversion, matching, result lifecycle
  notation.rs                     notation intermediate representation
  performance.rs                  live/attempt aggregate statistics
  session_history.rs              durable completed sessions
  progress.rs                     multi-session derived progress
crates/drums-app/src/              eframe desktop application
  main.rs                         state, startup, navigation, frame orchestration
  midi_file_ui.rs                 Track Library, Practice controls, Timeline painter
  notation_ui.rs                  Sheet Music painter
  audio_output.rs                 asynchronous device discovery and route ownership
  backing_audio.rs/click_audio.rs audio consumers
  *_ui.rs                         feature-specific views
  practice_settings.rs            persisted preferences
  track_library.rs                library model and TOML adapter
```

## Development invariants

- Keep hardware-independent rules in `drums-core`; egui, dialogs, and output devices belong in
  `drums-app`.
- Preserve MIDI callback timestamps and musical ticks. UI frames are not a timing authority.
- Resolve physical input through `ObservedMidiInput` and `DrumKitConfiguration`; never score live
  input by comparing raw hardware note numbers directly with lesson note numbers.
- Preserve aliases: several physical input identities may resolve to one logical drum. Reassignment
  of an already-owned identity must remain explicit.
- Treat Note On velocity zero as Note Off.
- Continuous hi-hat CC updates pedal state but is not a playable/scored hit.
- Timeline and Sheet Music share one transport and PracticeSession.
- Keep CPAL `output_devices()` enumeration on `AudioDeviceRefreshWorker`. Calling it from egui code
  can freeze the UI on slow audio backends.
- Never delete or overwrite source MIDI/audio files through Track Library or Reset All Data.

## Debugging guide

### MIDI input is broken

Start with `crates/drums-core/src/midi/backend.rs`, `midi/parser.rs`, and `midi/event.rs`. Open MIDI
Monitor, refresh/select/connect the port, and inspect message kind, channel, note/controller,
velocity/value, and timestamp. Monitor data is upstream of mapping and scoring. If no event appears,
the fault is at the hardware/driver/backend/parser boundary. Device disappearance is detected by
periodic port enumeration rather than a universal midir unplug callback.

### Drum mapping is broken

Inspect `crates/drums-core/src/mapping/mod.rs` for physical identity, conflict, alias, reassignment,
and resolution rules; inspect `mapping/store.rs` for per-device TOML; inspect
`crates/drums-app/src/mapping_ui.rs` for editor behavior. Confirm the exact connected device name and
compare Monitor input with the saved mapping identity.

### Setup Wizard is broken

State transitions and event acceptance live in `crates/drums-core/src/mapping/wizard.rs`; rendering
and button actions live in `crates/drums-app/src/wizard_ui.rs`; `DrumApp::forward_events_to_setup_wizard`
feeds events. Test boundary suppression, debounce, duplicate ownership, Note versus CC steps, aliases,
and preservation of a baseline mapping separately.

### Practice scoring is broken

Follow this path:

```text
DrumApp::handle_practice_midi_event
  -> ActualHit::from_midi_event_observed_at
  -> MIDI latency compensation
  -> PracticeSession::record_hit / advance_to_tick
  -> PracticeResult
```

The implementation is in `crates/drums-core/src/practice.rs`; aggregate summaries are in
`performance.rs` and durable conversion in `session_history.rs`. Check mapping identity, callback
timestamp, compensated elapsed time, transport tick, candidate ordering, and whether count-in or
transport state suppresses scoring.

### Timing, seek, or looping is broken

Inspect `crates/drums-core/src/midi/transport.rs`, then the frame orchestration in
`crates/drums-app/src/main.rs`. Tempo/time-signature conversion, practice BPM, state, and wrap logic
belong to `MusicalTransport`; the app synchronizes practice, metronome, and backing audio around it.

### Timeline is broken

Core expected-event data is created by `Timeline` in `midi/transport.rs` and
`DrumApp::rebuild_timeline`. Rendering is in `midi_file_ui.rs`, especially `rhythm_grid`, lane/window
helpers, result colours, grid headers, and playhead geometry. Check that expected and actual markers
use the same visible time window before changing painter coordinates.

### Sheet Music is broken

Inspect conversion/quantization in `crates/drums-core/src/notation.rs` and rendering/follow behavior
in `crates/drums-app/src/notation_ui.rs`. Practice-integrated controls and scroll suspension also
touch `midi_file_ui.rs`. Notation is derived; it must not rewrite source ticks used by scoring.

### Audio output is broken

Inspect `crates/drums-app/src/audio_output.rs` first. Confirm the background refresh worker produces a
snapshot, then check `planned_route`, preferred/default fallback state, and Rodio stream creation.
`click_audio.rs` handles metronome/test sound; `backing_audio.rs` handles decoded media. Cross-feature
routing and gain updates are coordinated in `main.rs` and `preferences_ui.rs`. Never move enumeration
onto the UI thread to simplify debugging.

### Persistence or Reset All Data is broken

- Mappings: `drums-core/src/mapping/store.rs`
- Sessions: `drums-core/src/session_history.rs`
- Preferences: `drums-app/src/practice_settings.rs`
- Track Library: `drums-app/src/track_library.rs`
- Reset scope/confirmation: `drums-app/src/preferences_ui.rs`

`DRUMS_CONFIG_DIR` overrides mapping storage and `DRUMS_HISTORY_PATH` overrides session history,
which is useful for isolated tests. Preferences and Track Library do not expose equivalent overrides.

### Progress or Performance Analysis is broken

Progress calculations live in `drums-core/src/progress.rs` and render in `progress_ui.rs`. Completed
session statistics live in `session_history.rs`; detailed presentation is
`performance_analysis_ui.rs`. Both features derive from the completed-session store, so validate that
history before debugging chart presentation.

## If you need to modify...

| Goal | Start here |
| --- | --- |
| Add/change a logical drum or mapping rule | `drums-core/src/mapping/mod.rs` |
| Add a MIDI device/backend feature | `drums-core/src/midi/backend.rs` |
| Change MIDI parsing | `drums-core/src/midi/parser.rs`, `midi/event.rs` |
| Change scoring | `drums-core/src/practice.rs` |
| Change timing windows | `TimingConfig` in `drums-core/src/practice.rs` |
| Change transport/BPM/loop behavior | `drums-core/src/midi/transport.rs` |
| Change Timeline rendering | `drums-app/src/midi_file_ui.rs` |
| Change Sheet Music conversion/rendering | `drums-core/src/notation.rs`, `drums-app/src/notation_ui.rs` |
| Change audio output | `drums-app/src/audio_output.rs`, then `main.rs`/`preferences_ui.rs` |
| Change Setup Wizard | `drums-core/src/mapping/wizard.rs`, then `drums-app/src/wizard_ui.rs` |
| Change persistence | the owning store/adapter listed above |
| Add a Practice preference | `drums-app/src/practice_settings.rs`, its UI, then `DrumApp` load/save |
| Change Progress/Performance Analysis | `drums-core/src/progress.rs` or `session_history.rs`, then matching UI |

Add focused domain tests before changing UI representation, then run the complete release quality
gate from [RELEASE.md](RELEASE.md).
