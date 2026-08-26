# MI Drums project status

## Release status

MI Drums v1.0.0 is functionally complete and is being prepared for public release. Current
application behavior is frozen for v1.0. The intended NUX DM-1 hardware workflow has completed
release testing, including the understood physical pedal behavior, which is not a release blocker.

## Implemented scope

- Windows desktop application using egui/eframe
- MIDI input discovery, connection, parsing, timestamps, Monitor, and disconnect detection
- Per-device Drum Mapping, aliases, explicit conflict handling, CC calibration, and persistence
- Guided Setup Wizard with final live mapping test
- Track Library and Standard MIDI File import
- Tempo-aware Practice, Timeline, Sheet Music, metronome, loops, count-in, and optional backing audio
- Correct/Early/Late, Wrong Drum, Missed, and Extra scoring with velocity and timing records
- Completed-session statistics, Performance Analysis, and multi-session Progress
- Audio-output selection, background device enumeration, fallback, volume, mute, and test sound
- Persisted practice, timing, audio, and appearance Preferences
- Confirmed Reset All Data limited to application-owned records

## Automated verification

The final release-preparation gate verifies:

- `cargo fmt --check`
- `cargo test --workspace` — **180 tests passed, 0 failed, 0 ignored**
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo doc --workspace --no-deps`
- `cargo build --release`
- `git diff --check`

Automated tests cover hardware-independent MIDI parsing/import, mapping aliases and conflicts, Setup
Wizard state, transport and metronome behavior, Practice scoring, hi-hat CC state, Timeline geometry,
notation, persistence round trips, session statistics, Progress, Preferences, audio refresh-worker
coordination, and Reset ownership.

## Real hardware QA

The intended NUX DM-1 kit has been tested for the final release and the application is considered
functionally working with that hardware. The physical pedal behavior has been observed and understood;
MI Drums continues to represent discrete pedal Note input separately from continuous pedal CC state.

Hardware QA and automated verification are complementary. Driver latency, MIDI event sequences, and
audio endpoint behavior can vary on other machines and devices.

## Known limitations

- Continuous hi-hat pedal CC is calibrated and tracked but does not create a scored hit or infer
  Practice articulation.
- Practice maps General MIDI notes 42, 44, and 46 to general `HiHat`; standalone notation retains
  closed, pedal, and open placement detail.
- One live MIDI input connection is supported at a time.
- Backing-track selection is not persisted between application runs.
- Track Library records reference source MIDI paths; moving a source file makes the entry unavailable.
- Preferences and Track Library loaders fall back to defaults/empty state when storage is malformed.
- Windows is the supported v1.0 release platform; the portable executable is unsigned.
- There is no installer or automatic updater in v1.0.

## Post-v1.0 candidates

- Atomic/recoverable persistence writes and clearer malformed-file recovery
- Caching of Track Library/session-history summaries and Timeline profiling
- Code signing and installer packaging
- Broader MIDI-device and operating-system validation
- Optional persisted backing-track associations
- Articulation-aware use of continuous hi-hat state after the lesson model is defined
