# Project Status — MIDI Drum Practice Application

## Current phase
**Phase 1: MIDI Foundation** (complete for this milestone).

Guided by `SPECIFICATION.md`, the project is deliberately following the
"smallest technically meaningful prototype" goal (spec §45): a working
USB-MIDI-kit → MIDI-input → display-note/velocity/timestamp pipeline. No later
phases have been started yet.

## Completed features (Phase 1)
- **Cargo workspace** (Rust 2021, `resolver = "2"`) split into:
  - `crates/drums-core` — hardware-independent MIDI logic (library crate).
  - `crates/drums-app`    — egui/eframe desktop GUI (binary crate).
- **GUI (egui/eframe 0.31)**
  - Native desktop window with a dark, workbench-style theme.
  - Enumerates and lists all available MIDI input devices in a combo box.
  - User selects a device and connects/disconnects.
  - Connection errors surface in the status line instead of crashing.
  - Live "Latest event" panel + a scrolling event log.
  - GUI code is fully separate from MIDI processing (lives in a different crate).
- **MIDI (midir 0.10)**
  - Device enumeration, selection, connection.
  - Disconnection detection via periodic, cheap re-enumeration; the app does
    not crash and lets the user reconnect.
- **MIDI event monitor**
  - For every incoming message the GUI shows: message type, MIDI note number
    (when applicable), velocity (when applicable), channel (when applicable),
    and a high-resolution application timestamp.
  - Streaming parser with running-status support, split-across-chunk handling,
    SysEx, System Common, and System Real-Time messages.
- **Threading / timing**
  - Real-time MIDI callback runs on the OS MIDI thread.
  - `std::sync::mpsc` channel (thread-safe) carries events from the callback
    to the GUI; the GUI only drains it each frame.
  - Timing is **not** derived from the GUI frame loop: each event carries a
    monotonic `Instant`-based timestamp (`µs`) captured at receipt time.
  - No expensive work in the MIDI callback (parse is cheap; no I/O; no
    allocation on the hot path for short messages).
- **Testing**
  - 10 unit tests covering the hardware-independent MIDI/event parsing:
    note-on/off, multi-message chunks, messages split across chunks, running
    status, program-change running status, CC + pitch bend, SysEx, interleaved
    real-time, timestamp propagation.
  - `cargo check --workspace` passes with no warnings.
  - `cargo test --workspace` passes (10/10).

## Current limitations
- No drum mapping, setup wizard, metronome, MIDI import, audio, scoring,
  practice engine, song library, or AI coaching (Phase 2+ / out of scope).
- Disconnection handling is best-effort: midir has no uniform cross-platform
  "device unplugged" notification, so the app polls device presence ~1×/second.
  This is cheap but not instantaneous.
- Multiple simultaneous devices are not yet practical; only one input
  connection at a time.
- The GUI event log is capped (newest 400 events) to bound memory.
- Painter-free minimal UI; visual timeline is intentionally not implemented.
- No persistence of selected device (not required for Phase 1).

## Important architectural decisions
- **Two-crate workspace**: MIDI logic (`drums-core`) has no GUI/audio deps so
  it is fast to test and reusable by any front-end. The GUI (`drums-app`) is a
  thin front-end.
- **Real-time callback separation**: midir's callback only (a) samples the
  high-resolution clock, (b) parses bytes, (c) pushes onto an `mpsc` channel.
  All UI is driven by draining that channel on the GUI thread.
- **Clock abstraction** (`time::Clock` / `SystemClock`): timestamps are taken
  at message receipt via a monotonic `Instant`, decoupled from frame rate.
- **`mpsc` over shared-memory/atom** for events: simple, blocking-free for the
  producer at the scales expected, and the receiver never stalls the producer
  (the callback ignores send failures if the GUI is gone).
- **Streaming byte parser** with running-status support to be robust to the
  arbitrary chunking real MIDI delivers, rather than assuming one message per
  callback.
- **Error type** `midi::error::MidiError` wraps midir/port/connect failures so
  the GUI can present them gracefully.

## Known issues
- The midir callback's native `u64` timestamp is captured but not currently
  surfaced; the authoritative timestamp shown is our own monotonic µs clock
  captured at receipt (this is intentional and satisfies the spec, but the raw
  midir timestamp is available if ever needed).
- Running-status messages that begin mid-stream (device already sending with no
  preceding status in our stream) are handled only once a full 2-byte pair
  arrives; a lone first byte is buffered.

## Next phase
- **Phase 2 → Setup wizard + drum mapping** (spec §4–5): guided identification
  of each physical drum, per-device note/channel/velocity mapping, and local
  persistence/restore of a device's configuration.
- Eventually: transport clock, metronome, MIDI file import/timeline, scoring.

## Build & run
```
cargo run --release -p drums-app     # launch the GUI
cargo test --workspace               # run unit tests
```