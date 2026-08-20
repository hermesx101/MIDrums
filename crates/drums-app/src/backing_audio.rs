use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct BackingAudio {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    sink: Option<Sink>,
    path: Option<PathBuf>,
    duration: Duration,
    volume: f32,
    muted: bool,
}

impl BackingAudio {
    pub fn new() -> Self {
        match OutputStream::try_default() {
            Ok((stream, handle)) => Self {
                _stream: Some(stream),
                handle: Some(handle),
                sink: None,
                path: None,
                duration: Duration::ZERO,
                volume: 0.8,
                muted: false,
            },
            Err(_) => Self {
                _stream: None,
                handle: None,
                sink: None,
                path: None,
                duration: Duration::ZERO,
                volume: 0.8,
                muted: false,
            },
        }
    }
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        if !path.exists() {
            return Err("Audio file unavailable".into());
        }
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
        sink.pause();
        self.sink = Some(sink);
        self.path = Some(path.to_path_buf());
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
        self.sink.is_some()
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
            s.set_volume(if self.muted { 0.0 } else { self.volume });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_audio_is_reported_without_crashing() {
        let mut audio = BackingAudio::new();
        let missing =
            std::env::temp_dir().join(format!("missing-audio-{}.mp3", std::process::id()));
        assert!(audio.load(missing).unwrap_err().contains("unavailable"));
    }
}
