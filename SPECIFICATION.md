# MI Drums - Product Specification

This document describes the behavior implemented in the current codebase. It is the product contract, not a historical roadmap.

## 1. Product overview

MI Drums is a local desktop application for practicing electronic drums against imported MIDI lessons. It combines a live MIDI input, a device-specific drum mapping, an imported lesson, a musical transport, immediate hit evaluation, and persisted practice history.

The application does not require an online service. User-selected MIDI and audio files remain in their original locations; the application stores references and application-generated records.

## 2. Core user flow

```text
Application
  -> MIDI device
  -> Setup Wizard
  -> saved drum configuration
  -> Track Library
  -> select or import a lesson
  -> Practice (Timeline or Sheet Music)
  -> session results
  -> Performance Analysis / Progress
```

On startup, the application checks for at least one usable saved mapping. With no usable mapping it opens the existing Setup Wizard flow. Connecting a device without a saved usable configuration also starts setup. A usable configuration must contain at least one playable MIDI Note mapping; a continuous-controller mapping alone is not sufficient.

## 3. MIDI system

### Live input

- The application enumerates MIDI input ports and connects to one input at a time.
- Incoming bytes are parsed on the MIDI callback thread and sent to the GUI through a thread-safe channel.
- Events receive a monotonic application timestamp in microseconds when the callback receives them. Scoring compensates for GUI queue delay using the same clock domain.
- The GUI is requested to repaint when events arrive; MIDI timing does not depend on GUI frame rate.
- Device presence is periodically re-enumerated to detect disconnection and allow reconnection.

The streaming parser supports channel voice messages, running status, messages split across callback chunks, multiple messages in one chunk, System Exclusive, System Common, and interleaved System Real-Time messages.

### Event interpretation

- Note On with velocity greater than zero is a playable hit.
- Note On with velocity zero is treated as Note Off and is not a hit.
- Note Off is not a hit.
- Control Change (CC) can be observed and mapped for continuous hi-hat pedal position, but it is not a playable hit.
- MIDI channels are stored internally as 0-15 in raw messages and displayed/persisted for mappings as 1-16.

### MIDI lesson files

MI Drums imports `.mid` and `.midi` Standard MIDI Files. The parser supports formats 0, 1, and 2, metrical timing and timecode timing, tempo changes, time-signature changes, track names, MIDI channels, and Note On events. It identifies likely drum tracks using channel, track-name, and percussion-note heuristics and automatically selects the strongest likely drum track.

Imported notes are preserved in musical ticks and converted to seconds through the tempo map. Unknown percussion notes remain visible as unknown rather than being guessed.

## 4. Drum Mapping

A `DrumKitConfiguration` belongs to a MIDI device name and contains an ordered list of physical-input mappings.

A physical input identity consists of:

- event kind: MIDI Note or MIDI CC
- note/controller number
- MIDI channel

Velocity or CC value is transient input data and is not part of the identity.

Each mapping resolves one physical identity to a logical `DrumComponent`. Several physical inputs may resolve to the same logical drum, allowing aliases such as multiple ride zones or alternate snare notes. One physical input cannot belong to two logical drums; reassignment must be explicit.

The supported logical components are Crash, Ride, Closed Hi-Hat, Open Hi-Hat, Hi-Hat Pedal, Hi-Hat Pedal Position, Snare, Kick, Hi Tom, Mid Tom, and Low Tom. The pedal-position component is continuous state rather than a playable lane.

The Drum Mapping view can inspect saved mappings, add or remove individual aliases, replace a component's inputs, explicitly resolve conflicts, identify unknown live inputs, and save the edited per-device configuration.

## 5. Setup Wizard

The Setup Wizard is a GUI over the hardware-independent `SetupWizard` state machine. Its states are Intro, Detecting, Confirming, Completed, and Canceled.

The wizard:

1. Walks through the supported components in a fixed order.
2. Waits for the expected event kind: Note for playable components and CC for continuous pedal position.
3. Captures one primary input and displays nearby additional inputs as possible conflicts.
4. Requires explicit acceptance; it does not silently assign a hit.
5. Supports retry, skip, back, cancel, and adding another Note alias to the same component.
6. Rejects an input already owned by a different component unless later explicitly reassigned.
7. Learns the observed minimum and maximum values of the continuous pedal controller.
8. Provides a final live test stage before saving.

Re-running setup uses the existing configuration as a baseline. Skipped components retain their existing mappings unless the component is deliberately replaced. Canceling discards the current wizard run. Finishing validates and persists the resulting configuration through the same mapping store used by manual Drum Mapping.

## 6. Track Library

Importing a valid MIDI file adds a library record containing its source path, display name, first tempo/time signature when available, duration, note count, likely-drum status, import time, and later practice summary fields.

The Track Library supports:

- search by display name or path
- sorting by recently played, recently added, name, BPM, or accuracy
- opening Practice, Sheet Music, or Performance Analysis
- renaming the library display name without renaming the source file
- removing the library record without deleting the source file
- opening the source file's location

The library stores references, not copies. A missing or invalid source file is reported when it is opened.

## 7. Practice system

Opening a library entry creates a transport from the imported tempo and time-signature maps, derives expected drum events from the selected drum track, builds the Timeline and notation model, and creates a `PracticeSession`.

Practice controls include:

- play, pause, stop, beginning, end, and seek
- practice BPM from 20 to 300 BPM and reset to the file tempo
- automatic or manual quarter-, eighth-, and sixteenth-note subdivision
- count-in of Off, 1, 2, 4, or 8 bars
- Timeline lookahead of 1, 2, 4, 6, or 8 seconds
- looping of the current bar, 2, 4, or 8 bars, or a custom bar range
- Timeline or Sheet Music visualization selected under Settings -> View

Changing practice BPM scales playback time while preserving musical tick positions and tempo changes. Seeking or stopping clears the current matching state. Looping rearms expected events in the loop region while retaining prior loop outcomes, so statistics accumulate per encounter.

A completed full-song practice attempt is converted into a persisted session. Stopped or abandoned attempts are not stored as completed sessions.

## 8. Scoring

The lesson produces expected events. A live Note On is timestamped, latency-compensated, resolved through the active kit mapping, and converted into an actual hit at the transport's effective tick.

For each actual hit, the practice engine considers unconsumed, known expected events inside the configured matching window. It prefers the same logical drum, then the closest timing distance, then deterministic musical/stable ordering. One actual hit can consume at most one expected event. Simultaneous expected drums remain independently evaluable.

The default code configuration uses:

- a 100 ms inclusive outer matching window
- a 50 ms inclusive on-time (`Good`) window

These values are implementation defaults, not user Preferences. Actual minus expected is negative for early hits and positive for late hits.

The six user-facing outcome labels are:

- **Correct**: the logical drum matches and timing is within the `Good` window.
- **Early**: the logical drum matches inside the outer window but before the `Good` window.
- **Late**: the logical drum matches inside the outer window but after the `Good` window.
- **Wrong Drum**: an input falls inside a target's window but resolves to a different logical drum. It finalizes that target.
- **Missed**: a known expected event passes the late edge without a matched input.
- **Extra**: a playable actual input has no eligible expected target. Internally this is stored as an `Ignored` result with an actual hit.

`Expected` is a visual reference marker in the Timeline legend, not a scored outcome. Unknown expected MIDI notes are also internally preserved as `Ignored`, but they are excluded from accuracy because no safe logical target is known.

Accuracy is correct finalized targets divided by all finalized known targets (`Correct + Wrong Drum + Missed`). Extra inputs are reported but are outside the accuracy denominator. Timing judgments apply only to correct-drum hits.

## 9. Timeline

The Timeline is a rendered practice view over the transport, normalized expected notes, and practice results. It contains stable drum lanes, beat/bar/subdivision grid lines, expected markers, actual-result overlays, loop boundaries, a playhead, unknown-note handling, and the result legend.

The visible window includes recent history and the selected lookahead interval. Event positions are based on effective transport time; simultaneous events share a horizontal position, while unquantized actual hits retain distinct positions.

## 10. Sheet Music

The notation layer converts recognized drum events into a reusable score model with measures, time signatures, tempo marks, rhythmic values, rests, and simultaneous-note chords. It quantizes for display without modifying the imported source events.

The standalone Sheet Music view supports playback, zoom from 70% to 200%, and optional follow-playback scrolling. Practice can use Sheet Music instead of the Timeline while retaining the persistent transport, practice controls, status, metronome, and scoring. Only the notation content scrolls during practice.

Notation requires ticks-per-quarter timing. Timecode-timed files do not produce a notation score.

## 11. Metronome and backing audio

The metronome is synchronized to transport boundaries and supports quarter, eighth, and sixteenth subdivisions. It produces distinct synthesized clicks for the first-beat accent, other beats, and subdivisions. Automatic subdivision chooses a conventional grid based on imported note coverage; it falls back to eighth notes when confidence is low or timing is unsuitable.

Backing audio is optional and applies only to the current practice context. Supported file-picker extensions are MP3, WAV, OGG, and FLAC. Controls cover load/replace/remove, play, pause, stop, seek, volume, mute, and whether practice starts audio automatically. MIDI input latency and audio synchronization offset are separate settings. Audio seeks and loop wraps are resynchronized to the MIDI transport.

The application does not modify or delete the selected audio file, and the backing-audio path is not currently persisted across restarts.

## 12. Progress

Progress is derived read-only from persisted completed sessions for one track. It includes latest, best, and average accuracy; session count; BPM; consistency when available; accuracy and consistency over time; accuracy/consistency versus BPM; timing tendency; velocity trends; per-drum accuracy; weakest-drum identification after sufficient samples; and threshold-based textual insights.

Progress calculations do not modify session history and avoid inventing values when data is missing or insufficient.

## 13. Performance Analysis

Performance Analysis presents a selected completed session and its track history. It includes accuracy, consistency, signed timing and spread, early/late information, MIDI velocity/dynamics, result counts, individual hit records, per-drum breakdown, weakest supported area, generated insights, and navigation between historical sessions.

Both Performance Analysis and Progress use the same persisted session records produced by the practice engine; they do not maintain separate histories.

## 14. Settings and Preferences

The Settings menu contains the authoritative Timeline/Sheet Music view selector, MIDI input latency, audio sync offset, MIDI Monitor, Setup Wizard, Drum Mapping, and a link to the dedicated Preferences page.

Preferences contains exactly:

- Theme: System, Dark, or Light
- Accent Colour: Blue, Teal, Purple, or Amber
- UI Scale: Small, Normal, or Large
- Reset All Data

Appearance settings are applied globally through egui's theme/style and zoom mechanisms. Normal scale is the default. Accent color is limited to interaction emphasis and does not replace semantic practice-result colors.

## 15. Data persistence

Application-owned data is stored as local TOML files:

- one mapping file per MIDI device in a mappings directory
- Track Library records in `track-library.toml`
- completed practice sessions in `practice-sessions.toml`
- timing, practice-view, audio-start, and appearance settings in `practice-settings.toml`

On Windows these normally live below the user's application-data directory. Mappings, sessions, and settings use the corresponding user configuration directory on HOME-based systems where implemented. Mapping and session paths can be overridden with `DRUMS_CONFIG_DIR` and `DRUMS_HISTORY_PATH`, primarily for testing.

Original MIDI and audio files are never application-owned persistence. The library and audio controls retain paths to those files.

## 16. Reset All Data

Reset All Data requires explicit confirmation. On confirmation it removes application-owned mapping TOML files, the session-history file, the Track Library record file, and the settings file. Progress and Performance Analysis are reset by removing their authoritative session history.

After successful deletion, the application disconnects MIDI, clears relevant in-memory practice, mapping, library, history-derived, notation, and backing-audio state, restores default appearance/settings, and enters the existing Setup Wizard first-launch flow.

Source MIDI and audio files are not deleted. Reset failures are reported. The deletion spans multiple files and is not transactional, so an operating-system failure can result in a reported partial reset.

## 17. Known limitations

- Final physical validation with the NUX DM-1 is pending. A pedal interaction may emit more than one MIDI event; whether that behavior originates in hardware, configuration, or software handling has not been established.
- Continuous hi-hat CC is calibrated, persisted, and tracked as pedal state, but it does not create a hit and does not currently alter scoring articulation.
- General MIDI lesson conversion for Practice currently groups GM hi-hat notes 42, 44, and 46 into the closed/general hi-hat target. Standalone notation distinguishes closed, pedal, and open GM hi-hat notes.
- Only one live MIDI input connection is supported at a time.
- Backing-audio synchronization and MIDI latency compensation require manual calibration and physical validation.
- Windows is the currently verified development platform; cross-platform behavior has not completed validation.
- Some non-critical settings and Track Library load/save paths fall back silently or do not surface every persistence error consistently.
- The repository does not yet contain license text files or automated CI configuration.
