# MI Drums - Project Status

## Overall status

MI Drums is functionally developed across its primary setup, mapping, library, practice, scoring, notation, history, analysis, and Preferences workflows. Automated validation is clean. The project is now in real-world hardware and release-readiness validation, not final production release.

The current hardware target is the NUX DM-1. Final end-to-end validation with that kit has not been completed.

## Completed

- **Core application:** native egui/eframe desktop shell, navigation, responsive practice workspace, and local-only operation.
- **MIDI handling:** port enumeration, single-device connection, reconnect/disconnect handling, callback-thread timestamps, event queueing, and repaint notification.
- **MIDI parsing:** streaming parser for channel voice, running status, split/multiple messages, SysEx, System Common, and System Real-Time events.
- **MIDI Monitor:** device controls, latest-event details, and a bounded live event log.
- **Drum Mapping:** per-device Note/CC mappings, explicit conflicts, aliases, manual add/remove/reassign, and TOML persistence.
- **Setup Wizard:** guided detection and confirmation, retry/skip/back/cancel, multiple Note aliases, continuous pedal calibration, final live test, and safe editing of existing mappings.
- **Hi-hat input model:** distinct logical closed, open, discrete pedal, and continuous pedal-position mappings. Continuous CC state is implemented separately from playable hits.
- **Track Library:** `.mid`/`.midi` import, metadata, search, sorting, display rename, record removal, source-location access, and latest/best practice summaries.
- **Practice:** musical transport, tempo scaling, seeking, count-in, subdivision, lookahead, looping, metronome, optional backing audio, and Timeline/Sheet Music selection.
- **Scoring:** deterministic matching for simultaneous and rapid events, timing judgments, Wrong Drum, Missed, Extra, unknown-event handling, session accuracy, and per-drum statistics.
- **Timeline:** drum lanes, beat/bar/subdivision grid, expected/actual markers, playhead, loop region, lookahead, and semantic result legend.
- **Sheet Music:** generated drum notation, time signatures, tempo, rests/chords, zoom, playback following, and a practice-integrated scrolling view.
- **Session history:** completed-session persistence with raw hit records, timing, velocity, authoritative result counts, and per-drum statistics.
- **Performance Analysis:** session scorecard, hit/timing/dynamics analysis, per-drum breakdown, insights, and history navigation.
- **Progress:** multi-session accuracy, consistency, BPM, timing, dynamics, per-drum trends, weakest-drum calculation, and threshold-based insights.
- **Settings and Preferences:** practice view, MIDI latency, audio sync, theme, accent color, UI scale, and persisted defaults.
- **Reset All Data:** confirmed deletion of application-owned mappings, sessions, library records, and settings; in-memory reset to the existing first-launch Setup flow; source media retained.
- **Code cleanup:** shared MIDI-input resolution, first-class mapping aliases, extracted MIDI Monitor UI, and separation of core domain logic from GUI concerns.

## Validation

Validation performed on 2026-08-20 in the current Windows development environment:

- `cargo fmt --all -- --check` - passed
- `cargo clippy --workspace --all-targets -- -D warnings` - passed with no warnings
- `cargo build --workspace` - passed
- `cargo test --workspace --no-fail-fast` - 164 passed, 0 failed
  - `drums-app`: 18 tests
  - `drums-core`: 146 tests

The suite covers hardware-independent parsing, mapping, Setup Wizard state transitions, MIDI file import, transport/metronome behavior, scoring, hi-hat CC state, notation, session persistence, performance/progress calculations, Preferences persistence, reset confirmation, and source-file deletion safety.

No Critical or High software regressions were observed in this automated validation. The checks do not exercise a physical MIDI device, operating-system MIDI driver, real audio output, or manual UI behavior.

## Hardware validation

Final NUX DM-1 validation remains pending. A known observation is that one pedal interaction can produce more than one MIDI output/event. The code can display those inputs, exposes nearby inputs during setup, supports separate discrete Note and continuous CC mappings, and prevents CC pedal state from becoming a scored hit.

The available evidence does not establish whether the multi-event pedal behavior is caused by the kit, its configuration, the MIDI driver, or application interpretation. Capture the exact messages in MIDI Monitor before treating it as a software defect.

## Known limitations

- Physical NUX DM-1 coverage has not completed final validation across every pad, hi-hat state, alias/zone, velocity range, and pedal action.
- Continuous hi-hat CC updates calibrated pedal state but does not currently change lesson articulation or scoring.
- Practice's General MIDI expected-note conversion currently groups GM hi-hat notes 42, 44, and 46 into one general/closed hi-hat target; standalone notation distinguishes them.
- Only one live MIDI input connection is supported at a time.
- MIDI latency compensation and backing-audio synchronization need manual calibration on real hardware.
- Backing-track selection is not persisted across application restarts.
- Windows is validated; other desktop platforms have not completed build and runtime validation.
- Track Library and practice-settings persistence do not surface every load/save failure consistently. Reset reports failures but its multi-file deletion is not transactional.
- Cargo metadata declares `MIT OR Apache-2.0`, but the repository does not contain the corresponding license text files.
- No continuous-integration workflow or packaged installer is present.

## Remaining work

### Required before release

1. Complete an end-to-end NUX DM-1 hardware matrix: connection, each mapped component, aliases, open/closed/pedal behavior, continuous CC range, rapid/simultaneous hits, reconnect, and saved-map restore.
2. Record and classify the pedal's multi-event output in MIDI Monitor; change software only if the trace demonstrates an application defect.
3. Manually validate scoring latency and backing-audio synchronization on the target hardware/audio stack.
4. Run and document build/runtime smoke tests on every operating system claimed as supported.
5. Add the actual MIT and Apache-2.0 license text files, or revise the Cargo license declaration before public distribution.
6. Decide the supported release artifact and perform a clean-machine installation/run test.

### Optional future improvements

- Add CI for formatting, strict Clippy, build, and tests.
- Improve persistence error reporting and make destructive reset more recoverable/transactional.
- Persist backing-track associations if that workflow is desired.
- Support multiple simultaneous MIDI input connections if required by real users.
- Extend continuous hi-hat state into articulation-aware lesson matching after the expected-note model is defined for it.
- Add platform packaging and release automation after supported platforms are established.
