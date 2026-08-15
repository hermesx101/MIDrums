//! Data structures describing parsed MIDI messages and events.

/// High-level classification of a MIDI message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MessageKind {
    /// 0x80 — Note Off.
    NoteOff,
    /// 0x90 — Note On.
    NoteOn,
    /// 0xA0 — Polyphonic Key Pressure (Aftertouch).
    PolyphonicAftertouch,
    /// 0xB0 — Control Change.
    ControlChange,
    /// 0xC0 — Program Change.
    ProgramChange,
    /// 0xD0 — Channel Pressure (Channel Aftertouch).
    ChannelPressure,
    /// 0xE0 — Pitch Bend.
    PitchBend,
    /// 0xF0 .. 0xF7 — System Exclusive.
    SystemExclusive,
    /// 0xF1 .. 0xF6 — System Common (excluding EOX, handled separately).
    SystemCommon,
    /// 0xF8 .. 0xFF — System Real-Time.
    Realtime,
    /// Unrecognised byte.
    Unknown,
}

impl MessageKind {
    /// Short, human readable label used in the GUI.
    pub fn name(&self) -> &'static str {
        match self {
            MessageKind::NoteOff => "Note Off",
            MessageKind::NoteOn => "Note On",
            MessageKind::PolyphonicAftertouch => "Poly Aftertouch",
            MessageKind::ControlChange => "Control Change",
            MessageKind::ProgramChange => "Program Change",
            MessageKind::ChannelPressure => "Channel Pressure",
            MessageKind::PitchBend => "Pitch Bend",
            MessageKind::SystemExclusive => "SysEx",
            MessageKind::SystemCommon => "System Common",
            MessageKind::Realtime => "Real-Time",
            MessageKind::Unknown => "Unknown",
        }
    }
}

/// A parsed MIDI message, independent of its timing.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiMessage {
    pub kind: MessageKind,
    /// MIDI channel 0..15 (present only for channel-voice messages).
    pub channel: Option<u8>,
    /// First data byte (e.g. note number, controller number).
    pub data1: Option<u8>,
    /// Second data byte (e.g. velocity, controller value).
    pub data2: Option<u8>,
    /// Present only for System Exclusive messages; raw 0xF0..0xF7 bytes.
    pub system_exclusive: Option<Vec<u8>>,
}

impl MidiMessage {
    /// MIDI note number for messages where a note is applicable (Note On,
    /// Note Off, Polyphonic Aftertouch), otherwise `None`.
    pub fn note(&self) -> Option<u8> {
        match self.kind {
            MessageKind::NoteOn
            | MessageKind::NoteOff
            | MessageKind::PolyphonicAftertouch => self.data1,
            _ => None,
        }
    }

    /// Velocity / intensity for messages where it is applicable.
    pub fn velocity(&self) -> Option<u8> {
        match self.kind {
            MessageKind::NoteOn | MessageKind::NoteOff => self.data2,
            MessageKind::PolyphonicAftertouch => self.data2,
            MessageKind::ChannelPressure => self.data1,
            MessageKind::ControlChange => self.data2,
            _ => None,
        }
    }

    /// 1-based channel for display purposes (1..16); `None` for system messages.
    pub fn channel_number(&self) -> Option<u8> {
        self.channel.map(|c| c + 1)
    }
}

/// A timestamped MIDI event as produced by the parser and consumed by the GUI.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiEvent {
    pub message: MidiMessage,
    /// High-resolution monotonic application timestamp in microseconds.
    pub timestamp_micros: u64,
}