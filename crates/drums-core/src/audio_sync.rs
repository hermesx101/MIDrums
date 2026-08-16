//! Pure synchronization math shared by the GUI and tests.

pub fn compensate_midi_input(received_seconds: f64, latency_ms: f64) -> f64 {
    (received_seconds - latency_ms / 1_000.0).max(0.0)
}

pub fn audio_position(midi_seconds: f64, offset_ms: f64, duration_seconds: f64) -> f64 {
    (midi_seconds + offset_ms / 1_000.0).clamp(0.0, duration_seconds.max(0.0))
}

pub fn looped_audio_position(midi_seconds: f64, loop_start: f64, loop_end: f64, offset_ms: f64) -> f64 {
    let span = loop_end - loop_start;
    let midi = if span > 0.0 && midi_seconds >= loop_end { loop_start + (midi_seconds - loop_start) % span } else { midi_seconds };
    (midi + offset_ms / 1_000.0).max(0.0)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn latency_compensation_subtracts_without_underflow(){assert_eq!(compensate_midi_input(1.0,25.0),0.975);assert_eq!(compensate_midi_input(0.01,25.0),0.0);}
    #[test] fn audio_offset_is_clamped_and_independent(){assert_eq!(audio_position(1.0,200.0,10.0),1.2);assert_eq!(audio_position(1.0,-1200.0,10.0),0.0);}
    #[test] fn seek_conversion_and_looping_are_stable(){assert_eq!(audio_position(5.0,0.0,20.0),5.0);assert_eq!(looped_audio_position(12.5,4.0,10.0,0.0),6.5);}
}
