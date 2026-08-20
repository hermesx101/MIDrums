//! Small reusable synthesized metronome click output.

use drums_core::ClickKind;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use std::time::Duration;

pub struct ClickAudio {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    active: Vec<Sink>,
}

impl ClickAudio {
    pub fn new() -> Self {
        match OutputStream::try_default() {
            Ok((stream, handle)) => Self {
                _stream: Some(stream),
                handle: Some(handle),
                active: Vec::new(),
            },
            Err(_) => Self {
                _stream: None,
                handle: None,
                active: Vec::new(),
            },
        }
    }
    pub fn available(&self) -> bool {
        self.handle.is_some()
    }
    pub fn play(&mut self, kind: ClickKind) {
        self.active.retain(|sink| !sink.empty());
        let Some(handle) = &self.handle else {
            return;
        };
        let (frequency, volume) = match kind {
            ClickKind::Accent => (1_200.0, 0.28),
            ClickKind::Beat => (880.0, 0.20),
            ClickKind::Subdivision => (660.0, 0.12),
        };
        if let Ok(sink) = Sink::try_new(handle) {
            sink.append(
                rodio::source::SineWave::new(frequency)
                    .take_duration(Duration::from_millis(28))
                    .amplify(volume),
            );
            self.active.push(sink);
        }
    }
}
