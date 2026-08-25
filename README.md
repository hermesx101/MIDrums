# MI Drums

MI Drums is a native desktop practice application for electronic drum kits connected through MIDI. It imports MIDI lessons, turns their drum events into a timed practice target, resolves live kit input through a per-device drum mapping, and records timing and accuracy results locally.

The application is written in Rust with egui/eframe. It does not require a server or cloud account.

## Core capabilities

- MIDI input-device discovery, connection, disconnect detection, and live MIDI Monitor
- Guided Setup Wizard and manual Drum Mapping
- Per-device mappings, including multiple physical inputs for one logical drum
- Separate closed hi-hat, open hi-hat, discrete pedal, and continuous pedal-position mappings
- Track Library with MIDI lesson import, search, sorting, rename, and history summaries
- Practice transport with BPM, subdivision, count-in, lookahead, looping, and seeking
- Timeline and generated drum Sheet Music views
- Transport-synchronized metronome and optional backing audio
- Live outcomes for Correct, Early, Late, Wrong Drum, Missed, and Extra hits
- Session statistics, per-session Performance Analysis, and multi-session Progress views
- Persisted timing and appearance Preferences
- Confirmed Reset All Data flow that removes application-owned data without deleting source MIDI or audio files

## Current status

The implemented application and automated test suite are functional. The current focus is real-world hardware validation with the NUX DM-1, especially pedal behavior and end-to-end timing/audio synchronization. MI Drums should not yet be described as production-ready.

See [PROJECT_STATUS.md](PROJECT_STATUS.md) for current validation results and release blockers.

## Requirements

- A current stable Rust toolchain with Cargo
- A supported desktop graphical environment
- A system MIDI input backend and a MIDI-capable drum kit for live setup/practice
- An audio output device for metronome clicks or backing tracks

The current repository has been built and tested on Windows. The dependencies are cross-platform Rust crates, but other operating systems have not been fully validated and may require platform MIDI, audio, and window-system development packages.

## Building

Build the complete workspace:

```sh
cargo build --workspace
```

Build the optimized desktop application:

```sh
cargo build --release -p drums-app
```

## Running

Run the development build:

```sh
cargo run -p drums-app
```

The optimized executable is produced under `target/release/` after a release build.

## Testing

Run all unit and documentation tests:

```sh
cargo test --workspace --no-fail-fast
```

The repository also uses Rustfmt and strict Clippy checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Automated tests exercise hardware-independent behavior. They do not replace testing with a physical drum kit, MIDI driver, and audio device.

## Project structure

```text
crates/drums-core/   MIDI, mapping, transport, practice, scoring, notation,
                    session history, performance, and progress logic
crates/drums-app/    egui/eframe application state, views, persistence adapters,
                    metronome audio, and backing audio
Files/               Sample MIDI/audio files for manual use
```

## Documentation

- [SPECIFICATION.md](SPECIFICATION.md) defines current product behavior and requirements.
- [PROJECT_STATUS.md](PROJECT_STATUS.md) records what is complete, what was validated, and what remains.
- [ARCHITECTURE.md](ARCHITECTURE.md) explains the code structure and data flow.
- [DEVELOPMENT.md](DEVELOPMENT.md) contains the developer build, quality, and debugging workflow.

Cargo metadata declares the project as `MIT OR Apache-2.0`. License text files have not yet been added and are listed as a pre-release task.
