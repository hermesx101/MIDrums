# MI Drums

MI Drums is a native desktop practice application for electronic drum kits connected through
MIDI. It turns drum parts from Standard MIDI Files into playable exercises, resolves the notes and
controllers emitted by a physical kit through a saved per-device mapping, and gives immediate
timing and accuracy feedback.

MI Drums keeps its data locally. It does not require a server or cloud account. The desktop UI is
built with Rust and egui/eframe; MIDI input, scoring, transport, notation, and analysis logic are
kept in a separate hardware-independent core crate.

## Features

- MIDI input discovery, connection, disconnect detection, and a live MIDI Monitor
- Guided Setup Wizard plus manual Drum Mapping
- Multiple physical MIDI inputs mapped to one logical drum
- Closed/open hi-hat and discrete pedal mappings, with continuous pedal-CC calibration
- Track Library import, search, sorting, rename, removal, and recent-result metadata
- Standard MIDI File parsing and likely drum-track selection
- Practice transport with tempo control, count-in, lookahead, seeking, and bar-based loops
- Timeline with drum lanes, grid, playhead, expected notes, actual hits, and result markers
- Generated drum Sheet Music with zoom and Follow Playback
- Transport-synchronized metronome
- Selectable application audio output, Default System Device fallback, volume, mute, and test sound
- Optional MP3, WAV, OGG, or FLAC backing audio for the current practice context
- Correct, Early, Late, Wrong Drum, Missed, and Extra feedback
- Completed-session statistics, detailed Performance Analysis, and multi-session Progress views
- Persisted timing, practice-view, audio, theme, accent-colour, and UI-scale preferences
- Confirmed Reset All Data flow limited to application-owned data

## Requirements

For normal use:

- Windows is the currently tested desktop environment. Other platforms are not declared supported
  by this repository.
- A MIDI-capable electronic drum kit and a working system MIDI driver are required for live input.
- An audio output is required for metronome clicks, test audio, or backing tracks. MIDI input itself
  does not produce drum sounds through MI Drums.
- MIDI lesson files are required for Practice. Backing audio is optional.

For development, install a current stable Rust toolchain with Cargo. No minimum Rust version is
declared. Native prerequisites are those required by eframe, midir, rodio/CPAL, and rfd on the host
platform.

## Download

Download `MI-Drums-v1.0.0-windows-x86_64.zip` from this repository's GitHub **Releases** page. The
canonical repository URL is not recorded in the current source tree, so this README does not embed a
potentially incorrect download link. Verify the published SHA-256 checksum before running the file.

## Installation

MI Drums v1.0.0 uses a portable ZIP rather than an installer:

1. Download the Windows x86_64 ZIP and its `.sha256` checksum file from GitHub Releases.
2. Verify the checksum using PowerShell:

   ```powershell
   Get-FileHash .\MI-Drums-v1.0.0-windows-x86_64.zip -Algorithm SHA256
   ```

3. Extract the ZIP to a user-writable folder.
4. Open the extracted `MI Drums` folder and run `MI-Drums.exe`.

The v1.0 executable is unsigned. Windows may show a Microsoft Defender SmartScreen warning; confirm
that the download came from the project's GitHub Release and that its checksum matches before
choosing to run it. MI Drums stores its settings and practice data under the current user's Windows
application-data directory; no administrator installation is required.

## First launch

```text
Launch MI Drums
  -> Setup Wizard (when no usable saved mapping exists)
  -> select and connect a MIDI input
  -> map or skip each requested logical drum
  -> test and save the device mapping
  -> Track Library
  -> import a MIDI file
  -> choose its drum track and open Practice
```

If exactly one MIDI port is discovered, it is preselected; MI Drums does not automatically open the
connection. A kit with a usable saved mapping restores that mapping when its exact device name is
connected. If mappings already exist when the application starts, the initial page is Track
Library rather than Setup.

## Basic usage

1. Start MI Drums with `cargo run -p drums-app` or the release executable.
2. Select the kit in the MIDI-device control and connect it.
3. Complete Setup Wizard. Hit the requested drum, confirm the detected input, optionally add another
   input for the same logical drum, and continue. Drum Mapping can later edit or explicitly reassign
   inputs.
4. Open Track Library and choose **Import MIDI**. MI Drums stores a reference to the selected file;
   it does not copy or own the original.
5. Open a library entry in Practice. Choose the appropriate drum track if needed.
6. Use Timeline or Sheet Music. Set tempo, subdivision, count-in, metronome, lookahead, or loop
   controls as needed, then start the transport and play the expected part.
7. At completion, review the session summary. Open Performance Analysis for a detailed saved
   session or Progress for history across sessions.

## Audio

Preferences -> Audio Output selects either **Default System Device** or a named output. The saved
named device remains preferred if it disappears; MI Drums falls back to the system default and can
restore the preferred route after it returns. Device discovery is performed on a background worker,
so refreshing or periodically checking devices does not block the UI thread.

Application volume and mute affect both metronome/test sounds and backing audio. **Test Audio**
plays a short synthesized sound through the active route. Backing-track volume and mute are separate
per-session controls, multiplied by the application-wide gain.

MIDI input and audio output are independent: MIDI identifies what the player hit, while Rodio/CPAL
plays MI Drums audio. MI Drums does not synthesize the connected drum kit's sounds.

## Practice results

- **Correct:** the mapped logical drum matches an available expected drum inside the matching window.
- **Early / Late:** a correct hit outside the central on-time band but inside the outer matching window.
- **Wrong Drum:** a timed input reaches an expected target but resolves to a different logical drum.
- **Missed:** a known expected note passes its late matching boundary without a matching hit.
- **Extra:** a playable mapped hit has no available expected target inside the matching window.

The default central on-time band is +/-50 ms and the outer matching window is +/-100 ms. Completed
sessions preserve result records, signed timing offsets, velocity when available, overall counts,
accuracy, and per-drum statistics. Accuracy is based on correct outcomes divided by finalized known
targets; Extra hits do not add targets to that denominator.

## Known limitations

- Continuous hi-hat pedal CC can be calibrated and its latest normalized position is tracked, but it
  does not itself create a scored hit and is not used to infer open/closed articulation in Practice.
  A discrete pedal Note mapping remains separately playable and scoreable.
- Practice expected-note conversion currently groups General MIDI hi-hat notes 42, 44, and 46 as
  general `HiHat`; standalone notation retains closed/pedal/open placement detail.
- Backing-audio selection is held only for the current practice context and is not saved in Track
  Library.
- Track Library stores source paths rather than copies. Moving or deleting a source MIDI file makes
  opening that entry fail until the record is removed or the file is restored.
- Hardware MIDI/audio timing, drivers, and hot-plug behavior depend on the host system and require
  validation with the intended kit and output device.
- There is no installer, signing workflow, CI configuration, or automated release packaging in the
  repository.

## Build and documentation

```sh
cargo build --workspace
cargo run -p drums-app
```

The optimized executable is produced at `target/release/drums-app.exe` on Windows after
`cargo build --release`.

- [ARCHITECTURE.md](ARCHITECTURE.md) explains modules, invariants, and data flow.
- [DEVELOPMENT.md](DEVELOPMENT.md) contains setup, commands, and debugging guidance.
- [RELEASE.md](RELEASE.md) is the reproducible v1.0 release procedure.
- [SPECIFICATION.md](SPECIFICATION.md) records product-level behavior.

## License

MI Drums source code is dual-licensed under your choice of the
[MIT License](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE).

The project-created beginner MIDI exercises under `Files/` are distributed under the same choice of
licenses. Third-party Cargo dependencies remain governed by their own license terms.
