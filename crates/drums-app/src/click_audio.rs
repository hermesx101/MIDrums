//! Small reusable synthesized click/test sounds routed through shared output.

use std::time::Duration;

use drums_core::ClickKind;
use rodio::{OutputStreamHandle, Sink, Source};

pub struct ClickAudio {
    handle: Option<OutputStreamHandle>,
    output_gain: f32,
    active: Vec<Sink>,
}

impl ClickAudio {
    pub fn new() -> Self {
        Self {
            handle: None,
            output_gain: 1.0,
            active: Vec::new(),
        }
    }

    pub fn set_output(&mut self, handle: Option<OutputStreamHandle>, output_gain: f32) {
        for sink in self.active.drain(..) {
            sink.stop();
        }
        self.handle = handle;
        self.output_gain = output_gain.clamp(0.0, 1.0);
    }

    pub fn set_output_gain(&mut self, output_gain: f32) {
        self.output_gain = output_gain.clamp(0.0, 1.0);
    }

    pub fn play(&mut self, kind: ClickKind) -> Result<(), String> {
        let (frequency, volume) = match kind {
            ClickKind::Accent => (1_200.0, 0.28),
            ClickKind::Beat => (880.0, 0.20),
            ClickKind::Subdivision => (660.0, 0.12),
        };
        self.play_tone(frequency, Duration::from_millis(28), volume)
    }

    pub fn play_test_sound(&mut self) -> Result<(), String> {
        self.play_tone(660.0, Duration::from_millis(450), 0.32)
    }

    fn play_tone(&mut self, frequency: f32, duration: Duration, volume: f32) -> Result<(), String> {
        self.active.retain(|sink| !sink.empty());
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "No audio output device is available.".to_string())?;
        let sink = Sink::try_new(handle)
            .map_err(|error| format!("Could not open the audio output: {error}"))?;
        sink.append(
            rodio::source::SineWave::new(frequency)
                .take_duration(duration)
                .amplify(volume * self.output_gain),
        );
        self.active.push(sink);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_output_is_reported_instead_of_panicking() {
        let mut audio = ClickAudio::new();
        assert!(audio.play_test_sound().unwrap_err().contains("No audio"));
    }
}
