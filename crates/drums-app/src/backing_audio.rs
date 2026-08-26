//! Optional backing-track playback synchronized to the musical transport.
//!
//! `BackingAudio` owns decoded media and its Rodio sink, but it does not own
//! practice time. `DrumApp` seeks and controls it from `MusicalTransport`, the
//! authoritative playback clock.

use rodio::{Decoder, OutputStreamHandle, Sink, Source};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    time::Duration,
};

/// Decoded backing media and its current Rodio playback sink.
///
/// Track volume/mute are independent of the application-wide output gain.
/// Loading a file affects only the current practice context and is not
/// persisted in Track Library.
pub struct BackingAudio {
    handle: Option<OutputStreamHandle>,
    sink: Option<Sink>,
    path: Option<PathBuf>,
    duration: Duration,
    volume: f32,
    muted: bool,
    output_gain: f32,
}

impl BackingAudio {
    pub fn new(handle: Option<OutputStreamHandle>, output_gain: f32) -> Self {
        Self {
            handle,
            sink: None,
            path: None,
            duration: Duration::ZERO,
            volume: 0.8,
            muted: false,
            output_gain: output_gain.clamp(0.0, 1.0),
        }
    }

    /// Rebuilds the active sink on a newly selected shared output stream.
    pub fn set_output(
        &mut self,
        handle: Option<OutputStreamHandle>,
        output_gain: f32,
    ) -> Result<(), String> {
        let path = self.path.clone();
        let position = self.position();
        let was_playing = self.is_playing();
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.handle = handle;
        self.output_gain = output_gain.clamp(0.0, 1.0);
        if let Some(path) = path {
            self.rebuild_sink(&path, position, was_playing)?;
        }
        Ok(())
    }

    pub fn set_output_gain(&mut self, output_gain: f32) {
        self.output_gain = output_gain.clamp(0.0, 1.0);
        self.apply_volume();
    }

    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        if !path.exists() {
            return Err("Audio file unavailable".into());
        }
        self.unload();
        self.path = Some(path.to_path_buf());
        if let Err(error) = self.rebuild_sink(path, 0.0, false) {
            self.path = None;
            return Err(error);
        }
        Ok(())
    }

    fn rebuild_sink(
        &mut self,
        path: &Path,
        position: f64,
        should_play: bool,
    ) -> Result<(), String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or("Audio output device unavailable")?;
        let file = File::open(path).map_err(|e| e.to_string())?;
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| format!("Could not decode audio file: {e}"))?;
        self.duration = decoder.total_duration().unwrap_or_default();
        let sink = Sink::try_new(handle).map_err(|e| e.to_string())?;
        sink.append(decoder);
        if position > 0.0 {
            sink.try_seek(Duration::from_secs_f64(
                position.clamp(0.0, self.duration.as_secs_f64()),
            ))
            .map_err(|error| format!("Could not restore backing-track position: {error}"))?;
        }
        if !should_play {
            sink.pause();
        }
        self.sink = Some(sink);
        self.apply_volume();
        Ok(())
    }
    pub fn unload(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        self.path = None;
        self.duration = Duration::ZERO;
    }
    pub fn play(&self) {
        if let Some(s) = &self.sink {
            s.play();
        }
    }
    pub fn pause(&self) {
        if let Some(s) = &self.sink {
            s.pause();
        }
    }
    pub fn stop(&self) {
        if let Some(s) = &self.sink {
            s.pause();
            let _ = s.try_seek(Duration::ZERO);
        }
    }
    pub fn seek(&self, seconds: f64) -> Result<(), String> {
        self.sink
            .as_ref()
            .ok_or("No backing track loaded")?
            .try_seek(Duration::from_secs_f64(
                seconds.clamp(0.0, self.duration.as_secs_f64()),
            ))
            .map_err(|e| e.to_string())
    }
    pub fn position(&self) -> f64 {
        self.sink
            .as_ref()
            .map(|s| s.get_pos().as_secs_f64())
            .unwrap_or(0.0)
    }
    pub fn duration(&self) -> f64 {
        self.duration.as_secs_f64()
    }
    pub fn is_loaded(&self) -> bool {
        self.path.is_some()
    }
    pub fn is_playing(&self) -> bool {
        self.sink
            .as_ref()
            .is_some_and(|sink| !sink.is_paused() && !sink.empty())
    }
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn volume(&self) -> f32 {
        self.volume
    }
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.apply_volume();
    }
    pub fn muted(&self) -> bool {
        self.muted
    }
    pub fn set_muted(&mut self, v: bool) {
        self.muted = v;
        self.apply_volume();
    }
    fn apply_volume(&self) {
        if let Some(s) = &self.sink {
            s.set_volume(if self.muted {
                0.0
            } else {
                self.volume * self.output_gain
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_audio_is_reported_without_crashing() {
        let mut audio = BackingAudio::new(None, 1.0);
        let missing =
            std::env::temp_dir().join(format!("missing-audio-{}.mp3", std::process::id()));
        assert!(audio.load(missing).unwrap_err().contains("unavailable"));
    }

    #[test]
    fn shared_output_gain_does_not_change_track_volume_or_mute() {
        let mut audio = BackingAudio::new(None, 0.5);
        audio.set_volume(0.7);
        audio.set_muted(true);
        audio.set_output_gain(0.25);
        assert_eq!(audio.volume(), 0.7);
        assert!(audio.muted());
        assert!(audio.set_output(None, 0.8).is_ok());
    }
}
