# Changelog

All notable changes to MI Drums are documented in this file.

## [1.0.0] - YYYY-MM-DD

### Added

- Live MIDI device discovery, connection, parsing, monitoring, and disconnect detection.
- Per-device Drum Mapping with explicit conflicts, reassignment, and multiple physical aliases per logical drum.
- Guided Setup Wizard, including hi-hat Note mappings and continuous pedal-CC calibration.
- Track Library and Standard MIDI File import with track metadata and drum-track selection.
- Tempo-aware Practice transport, count-in, seeking, looping, subdivision, and metronome controls.
- Timeline and generated drum Sheet Music views over the same PracticeSession and transport.
- Correct, Early, Late, Wrong Drum, Missed, and Extra feedback with completed-session statistics.
- Selectable audio output, default-device fallback, global gain/mute, test sound, and optional backing audio.
- Progress and detailed Performance Analysis derived from local completed-session history.
- Persisted practice, timing, audio, theme, accent-colour, and UI-scale preferences.
- Confirmed Reset All Data flow limited to application-owned data.

### Known limitations

- Continuous hi-hat pedal CC is calibrated and tracked but does not create a scored hit or infer
  Practice articulation.
- Practice groups General MIDI hi-hat notes 42, 44, and 46 as general `HiHat`; standalone notation
  distinguishes their placement.
- Backing-audio selection is not persisted, and Track Library entries reference their source MIDI paths.
- The Windows executable is unsigned and may trigger Microsoft Defender SmartScreen warnings.
