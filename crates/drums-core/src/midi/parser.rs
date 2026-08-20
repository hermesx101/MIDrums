//! Streaming MIDI byte parser.
//!
//! MIDI callbacks (from midir and most back-ends) deliver an arbitrary chunk
//! of bytes that may contain zero, one or several messages, and a single
//! message may be split across consecutive chunks. This parser consumes a byte
//! stream incrementally and emits complete, timestamped `MidiEvent`s as they
//! become available.
//!
//! It supports:
//! - All channel-voice message types (Note On/Off, aftertouch, CC, program
//!   change, channel pressure, pitch bend).
//! - Running status (a channel-voice status byte omitted when it repeats).
//! - System Exclusive, terminated by EOX (0xF7).
//! - System Common and System Real-Time messages.
//! - Real-Time messages interleaved inside a partially received message.
//!
//! The parser performs no I/O, no dynamic allocation on the hot path for short
//! messages, and no timing operations — timestamps are supplied by the caller.
//! It is therefore safe and cheap to invoke from inside a real-time MIDI
//! callback.

use super::event::{MessageKind, MidiEvent, MidiMessage};

/// Streaming parser with internal running-status state.
#[derive(Clone, Debug, Default)]
pub struct MidiParser {
    /// Status byte of the most recent channel-voice message, for running status.
    last_status: Option<u8>,
    /// Status of the message currently being accumulated.
    current_status: Option<u8>,
    /// Remaining data bytes expected by the current message.
    needed: u8,
    /// Accumulated data bytes for the current (short) message.
    data: [u8; 2],
    data_len: usize,
    /// True while accumulating a System Exclusive message.
    in_sysex: bool,
    sysex: Vec<u8>,
}

impl MidiParser {
    /// Creates a parser with a blank, idle state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a chunk of raw MIDI bytes and returns any complete messages that
    /// became available, each stamped with the supplied application timestamp.
    pub fn push_bytes(&mut self, timestamp_micros: u64, chunk: &[u8]) -> Vec<MidiEvent> {
        let mut events = Vec::new();
        for &b in chunk {
            if b >= 0x80 {
                self.handle_status(b, timestamp_micros, &mut events);
            } else {
                self.handle_data(b, timestamp_micros, &mut events);
            }
        }
        events
    }

    fn handle_status(&mut self, b: u8, ts: u64, out: &mut Vec<MidiEvent>) {
        // Real-Time messages (0xF8..=0xFF) are single byte and may appear
        // anywhere, including between the bytes of another message.
        if b >= 0xF8 {
            out.push(MidiEvent {
                message: MidiMessage {
                    kind: MessageKind::Realtime,
                    channel: None,
                    data1: Some(b),
                    data2: None,
                    system_exclusive: None,
                },
                timestamp_micros: ts,
            });
            return;
        }

        if b == 0xF7 {
            // End of Exclusive terminates a SysEx message.
            if self.in_sysex {
                self.sysex.push(b);
                self.finish_sysex(ts, out);
            } else {
                out.push(MidiEvent {
                    message: MidiMessage {
                        kind: MessageKind::SystemCommon,
                        channel: None,
                        data1: Some(b),
                        data2: None,
                        system_exclusive: None,
                    },
                    timestamp_micros: ts,
                });
            }
            return;
        }

        // A new status byte supersedes any partial message.
        self.data_len = 0;
        self.needed = 0;

        if b == 0xF0 {
            // Start of System Exclusive.
            self.sysex.clear();
            self.sysex.push(b);
            self.in_sysex = true;
            self.current_status = None;
            return;
        }

        if b < 0xF0 {
            // Channel-voice status; also becomes the running status.
            self.current_status = Some(b);
            self.last_status = Some(b);
        } else {
            // System Common (0xF1..=0xF6) does not update running status.
            self.current_status = Some(b);
        }

        self.needed = expected_data_len(b);
        if self.needed == 0 {
            // e.g. Tune Request (0xF6) has no data bytes.
            self.finish_message(ts, out);
        }
    }
    fn handle_data(&mut self, b: u8, ts: u64, out: &mut Vec<MidiEvent>) {
        if self.in_sysex {
            self.sysex.push(b);
            return;
        }

        if self.needed > 0 {
            // Data byte belonging to the current message.
            self.data[self.data_len] = b;
            self.data_len += 1;
            self.needed -= 1;
            if self.needed == 0 {
                self.finish_message(ts, out);
            }
        } else if let Some(st) = self.last_status {
            // Running status: a data byte with no preceding status reuses the
            // last channel-voice status byte.
            self.current_status = Some(st);
            self.consume_data_byte(b, ts, out);
        }
        // Otherwise the byte is a stray data byte with no context; ignore it.
    }

    /// Records one data byte for a running-status message in progress.
    fn consume_data_byte(&mut self, b: u8, ts: u64, out: &mut Vec<MidiEvent>) {
        if let Some(st) = self.current_status {
            self.needed = expected_data_len(st);
            self.data[0] = b;
            self.data_len = 1;
            self.needed -= 1;
            if self.needed == 0 {
                self.finish_message(ts, out);
            }
        }
    }

    fn finish_message(&mut self, ts: u64, out: &mut Vec<MidiEvent>) {
        if let Some(st) = self.current_status {
            let kind = classify_status(st);
            out.push(MidiEvent {
                message: MidiMessage {
                    kind,
                    channel: if st < 0xF0 { Some(st & 0x0F) } else { None },
                    data1: (self.data_len >= 1).then(|| self.data[0]),
                    data2: (self.data_len >= 2).then(|| self.data[1]),
                    system_exclusive: None,
                },
                timestamp_micros: ts,
            });
        }
        self.current_status = None;
        self.data_len = 0;
        self.needed = 0;
    }

    fn finish_sysex(&mut self, ts: u64, out: &mut Vec<MidiEvent>) {
        let sysex = std::mem::take(&mut self.sysex);
        self.in_sysex = false;
        out.push(MidiEvent {
            message: MidiMessage {
                kind: MessageKind::SystemExclusive,
                channel: None,
                data1: None,
                data2: None,
                system_exclusive: Some(sysex),
            },
            timestamp_micros: ts,
        });
    }
}

/// Number of data bytes a status byte expects, or 0 for messages with none.
fn expected_data_len(status: u8) -> u8 {
    if status < 0xF0 {
        // Channel-voice messages: 1 data byte for Program Change and Channel
        // Pressure, 2 for everything else.
        match status & 0xF0 {
            0xC0 | 0xD0 => 1,
            _ => 2,
        }
    } else {
        // System Common messages: F1 (MTC) and F3 (Song Select) take 1,
        // F2 (Song Position) takes 2, and the rest (e.g. Tune Request) take 0.
        match status {
            0xF1 | 0xF3 => 1,
            0xF2 => 2,
            _ => 0,
        }
    }
}

/// Maps a status byte (non-real-time) to a [`MessageKind`].
fn classify_status(status: u8) -> MessageKind {
    if status < 0xF0 {
        match status & 0xF0 {
            0x80 => MessageKind::NoteOff,
            0x90 => MessageKind::NoteOn,
            0xA0 => MessageKind::PolyphonicAftertouch,
            0xB0 => MessageKind::ControlChange,
            0xC0 => MessageKind::ProgramChange,
            0xD0 => MessageKind::ChannelPressure,
            0xE0 => MessageKind::PitchBend,
            _ => MessageKind::Unknown,
        }
    } else {
        MessageKind::SystemCommon
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(events: &[MidiEvent]) -> Vec<MessageKind> {
        events.iter().map(|e| e.message.kind).collect()
    }

    #[test]
    fn parses_note_on_and_off() {
        let mut p = MidiParser::new();
        // Note On, channel 0, note 60, velocity 100.
        let events = p.push_bytes(1000, &[0x90, 60, 100]);
        assert_eq!(events.len(), 1);
        let ev = &events[0];
        assert_eq!(ev.message.kind, MessageKind::NoteOn);
        assert_eq!(ev.message.channel, Some(0));
        assert_eq!(ev.message.channel_number(), Some(1));
        assert_eq!(ev.message.note(), Some(60));
        assert_eq!(ev.message.velocity(), Some(100));
        assert_eq!(ev.timestamp_micros, 1000);

        // Note Off, channel 3, note 38, velocity 64.
        let events = p.push_bytes(2000, &[0x83, 38, 64]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message.kind, MessageKind::NoteOff);
        assert_eq!(events[0].message.channel_number(), Some(4));
        assert_eq!(events[0].message.note(), Some(38));
        assert_eq!(events[0].timestamp_micros, 2000);
    }

    #[test]
    fn handles_multiple_messages_in_one_chunk() {
        let mut p = MidiParser::new();
        let events = p.push_bytes(10, &[0x90, 36, 120, 0x90, 38, 90, 0x90, 42, 70]);
        assert_eq!(
            kinds(&events),
            vec![
                MessageKind::NoteOn,
                MessageKind::NoteOn,
                MessageKind::NoteOn,
            ]
        );
        let notes: Vec<u8> = events.iter().map(|e| e.message.note().unwrap()).collect();
        assert_eq!(notes, vec![36, 38, 42]);
    }

    #[test]
    fn supports_message_split_across_chunks() {
        let mut p = MidiParser::new();
        assert!(p.push_bytes(10, &[0x90]).is_empty()); // status only
        assert!(p.push_bytes(20, &[60]).is_empty()); // one data byte
        let events = p.push_bytes(30, &[105]); // final data byte
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message.kind, MessageKind::NoteOn);
        assert_eq!(events[0].message.note(), Some(60));
        assert_eq!(events[0].message.velocity(), Some(105));
        assert_eq!(events[0].timestamp_micros, 30);
    }

    #[test]
    fn handles_running_status() {
        let mut p = MidiParser::new();
        // Establish running status with an explicit Note On.
        p.push_bytes(10, &[0x90, 36, 100]);
        // Subsequent data pairs reuse 0x90 (Note On) as running status.
        let events = p.push_bytes(11, &[38, 90, 42, 80]);
        assert_eq!(
            kinds(&events),
            vec![MessageKind::NoteOn, MessageKind::NoteOn]
        );
        assert_eq!(events[0].message.note(), Some(38));
        assert_eq!(events[0].message.channel, Some(0));
        assert_eq!(events[1].message.note(), Some(42));
    }
    #[test]
    fn program_change_takes_single_data_byte() {
        let mut p = MidiParser::new();
        let events = p.push_bytes(5, &[0xC0, 23]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message.kind, MessageKind::ProgramChange);
        assert_eq!(events[0].message.data1, Some(23));
        assert_eq!(events[0].message.data2, None);
    }

    #[test]
    fn control_change_and_pitch_bend() {
        let mut p = MidiParser::new();
        let events = p.push_bytes(1, &[0xB1, 7, 100, 0xE2, 0x20, 0x30]);
        assert_eq!(
            kinds(&events),
            vec![MessageKind::ControlChange, MessageKind::PitchBend,]
        );
        assert_eq!(events[0].message.data1, Some(7)); // controller number
        assert_eq!(events[0].message.velocity(), Some(100)); // value
        assert_eq!(events[0].message.channel_number(), Some(2));
        assert_eq!(events[1].message.channel_number(), Some(3));
        assert_eq!(events[1].message.data1, Some(0x20)); // LSB
        assert_eq!(events[1].message.data2, Some(0x30)); // MSB
    }

    #[test]
    fn parses_system_exclusive_spread_over_chunks() {
        let mut p = MidiParser::new();
        assert!(p.push_bytes(1, &[0xF0, 0x7E, 0x7F]).is_empty());
        let events = p.push_bytes(2, &[0x40, 0x01, 0xF7]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message.kind, MessageKind::SystemExclusive);
        let sysex = events[0].message.system_exclusive.as_ref().unwrap();
        assert_eq!(sysex, &[0xF0, 0x7E, 0x7F, 0x40, 0x01, 0xF7]);
    }

    #[test]
    fn realtime_message_interleaved_within_another_message() {
        let mut p = MidiParser::new();
        // A note-on split by a clock tick (0xF8) in the middle.
        let events = p.push_bytes(1, &[0x90, 60, 0xF8, 100]);
        assert_eq!(
            kinds(&events),
            vec![MessageKind::Realtime, MessageKind::NoteOn,]
        );
        assert_eq!(events[0].message.data1, Some(0xF8));
        assert_eq!(events[1].message.note(), Some(60));
        assert_eq!(events[1].message.velocity(), Some(100));
    }

    #[test]
    fn timestamp_flows_through() {
        let mut p = MidiParser::new();
        let events = p.push_bytes(5_000_000, &[0x90, 40, 88]);
        assert_eq!(events[0].timestamp_micros, 5_000_000);
    }

    #[test]
    fn running_status_with_program_change() {
        let mut p = MidiParser::new();
        p.push_bytes(0, &[0xC3, 5]); // Program Change, channel 3
        let events = p.push_bytes(1, &[9]); // running status -> Program Change
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message.kind, MessageKind::ProgramChange);
        assert_eq!(events[0].message.data1, Some(9));
        assert_eq!(events[0].message.channel_number(), Some(4));
    }
}
