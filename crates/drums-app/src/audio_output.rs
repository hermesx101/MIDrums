//! Shared Rodio output-device selection and application-wide output gain.

use std::collections::BTreeSet;

use drums_core::ClickKind;
use rodio::{
    cpal::traits::{DeviceTrait, HostTrait},
    OutputStream, OutputStreamHandle,
};

use crate::click_audio::ClickAudio;

pub const DEFAULT_SYSTEM_DEVICE: &str = "Default System Device";

pub struct AudioOutput {
    preferred_device: Option<String>,
    available_devices: Vec<String>,
    active_device: Option<String>,
    using_fallback: bool,
    volume: f32,
    muted: bool,
    notice: Option<String>,
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    clicks: ClickAudio,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteUpdate {
    pub route_changed: bool,
    pub notice: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlannedRoute<'a> {
    Default,
    Preferred(&'a str),
    DefaultFallback(&'a str),
}

impl AudioOutput {
    pub fn new(preferred_device: Option<String>, volume: f32, muted: bool) -> Self {
        let mut output = Self {
            preferred_device: normalize_preference(preferred_device),
            available_devices: Vec::new(),
            active_device: None,
            using_fallback: false,
            volume: volume.clamp(0.0, 1.0),
            muted,
            notice: None,
            _stream: None,
            handle: None,
            clicks: ClickAudio::new(),
        };
        output.available_devices = enumerate_output_devices().unwrap_or_default();
        output.reopen();
        output
    }

    pub fn available_devices(&self) -> &[String] {
        &self.available_devices
    }

    pub fn preferred_device(&self) -> Option<&str> {
        self.preferred_device.as_deref()
    }

    pub fn active_device(&self) -> Option<&str> {
        self.active_device.as_deref()
    }

    pub fn using_fallback(&self) -> bool {
        self.using_fallback
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn effective_gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume
        }
    }

    pub fn handle(&self) -> Option<OutputStreamHandle> {
        self.handle.clone()
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.clicks.set_output_gain(self.effective_gain());
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.clicks.set_output_gain(self.effective_gain());
    }

    pub fn select_device(&mut self, preferred_device: Option<String>) -> RouteUpdate {
        self.preferred_device = normalize_preference(preferred_device);
        self.available_devices = enumerate_output_devices().unwrap_or_default();
        self.reopen();
        RouteUpdate {
            route_changed: true,
            notice: self.notice.clone(),
        }
    }

    /// Refreshes hot-plug state and restores a preferred device when it returns.
    pub fn refresh_devices(&mut self) -> RouteUpdate {
        let devices = match enumerate_output_devices() {
            Ok(devices) => devices,
            Err(error) => {
                let notice = format!("Could not refresh audio output devices: {error}");
                self.notice = Some(notice.clone());
                return RouteUpdate {
                    route_changed: false,
                    notice: Some(notice),
                };
            }
        };
        self.available_devices = devices;

        let default_name = default_output_device_name();
        let route_needs_refresh =
            match planned_route(self.preferred_device.as_deref(), &self.available_devices) {
                PlannedRoute::Preferred(preferred) => {
                    self.using_fallback || self.active_device.as_deref() != Some(preferred)
                }
                PlannedRoute::DefaultFallback(_) => {
                    !self.using_fallback || self.active_device != default_name
                }
                PlannedRoute::Default => {
                    self.active_device != default_name || self.handle.is_none()
                }
            };
        if route_needs_refresh {
            self.reopen();
        }
        RouteUpdate {
            route_changed: route_needs_refresh,
            notice: self.notice.clone(),
        }
    }

    pub fn play_click(&mut self, kind: ClickKind) -> Result<(), String> {
        self.clicks.play(kind)
    }

    pub fn play_test_sound(&mut self) -> Result<(), String> {
        if self.muted || self.volume <= f32::EPSILON {
            return Err("Audio output is muted or its volume is zero.".into());
        }
        self.clicks.play_test_sound()
    }

    fn reopen(&mut self) {
        self.clicks.set_output(None, self.effective_gain());
        self.handle = None;
        self._stream = None;
        self.active_device = None;
        self.using_fallback = false;
        self.notice = None;

        let requested = self.preferred_device.clone();
        let opened = match requested.as_deref() {
            Some(name) => match open_named_device(name) {
                Ok(opened) => Ok(opened),
                Err(selected_error) => {
                    self.using_fallback = true;
                    match open_default_device() {
                        Ok(opened) => {
                            self.notice = Some(format!(
                                "Selected audio device unavailable. Using Default System Device. ({selected_error})"
                            ));
                            Ok(opened)
                        }
                        Err(default_error) => Err(format!(
                            "Selected audio device unavailable ({selected_error}); Default System Device also failed ({default_error})."
                        )),
                    }
                }
            },
            None => open_default_device(),
        };

        match opened {
            Ok((stream, handle, name)) => {
                self.active_device = Some(name);
                self.handle = Some(handle.clone());
                self._stream = Some(stream);
                self.clicks.set_output(Some(handle), self.effective_gain());
            }
            Err(error) => {
                self.notice = Some(format!("Audio output unavailable: {error}"));
            }
        }
    }
}

pub fn enumerate_output_devices() -> Result<Vec<String>, String> {
    let host = rodio::cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|error| format!("device enumeration failed: {error}"))?;
    let mut names = BTreeSet::new();
    for device in devices {
        if let Ok(name) = device.name() {
            names.insert(name);
        }
    }
    Ok(names.into_iter().collect())
}

fn default_output_device_name() -> Option<String> {
    rodio::cpal::default_host()
        .default_output_device()
        .and_then(|device| device.name().ok())
}

fn open_default_device() -> Result<(OutputStream, OutputStreamHandle, String), String> {
    let device = rodio::cpal::default_host()
        .default_output_device()
        .ok_or_else(|| {
            "no default output device was reported by the operating system".to_string()
        })?;
    let name = device
        .name()
        .unwrap_or_else(|_| DEFAULT_SYSTEM_DEVICE.to_string());
    let (stream, handle) = OutputStream::try_from_device(&device)
        .map_err(|error| format!("could not open {name}: {error}"))?;
    Ok((stream, handle, name))
}

fn open_named_device(name: &str) -> Result<(OutputStream, OutputStreamHandle, String), String> {
    let host = rodio::cpal::default_host();
    let mut devices = host
        .output_devices()
        .map_err(|error| format!("device enumeration failed: {error}"))?;
    let device = devices
        .find(|device| device.name().is_ok_and(|device_name| device_name == name))
        .ok_or_else(|| format!("{name} is not connected"))?;
    let (stream, handle) = OutputStream::try_from_device(&device)
        .map_err(|error| format!("could not open {name}: {error}"))?;
    Ok((stream, handle, name.to_string()))
}

fn normalize_preference(preferred_device: Option<String>) -> Option<String> {
    preferred_device.and_then(|device| {
        let device = device.trim();
        (!device.is_empty() && device != DEFAULT_SYSTEM_DEVICE).then(|| device.to_string())
    })
}

fn planned_route<'a>(
    preferred_device: Option<&'a str>,
    available_devices: &[String],
) -> PlannedRoute<'a> {
    match preferred_device {
        Some(preferred) if available_devices.iter().any(|device| device == preferred) => {
            PlannedRoute::Preferred(preferred)
        }
        Some(preferred) => PlannedRoute::DefaultFallback(preferred),
        None => PlannedRoute::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_device_preference_is_normalized() {
        assert_eq!(normalize_preference(None), None);
        assert_eq!(normalize_preference(Some(String::new())), None);
        assert_eq!(
            normalize_preference(Some(DEFAULT_SYSTEM_DEVICE.into())),
            None
        );
        assert_eq!(
            normalize_preference(Some("USB Headphones".into())),
            Some("USB Headphones".into())
        );
    }

    #[test]
    fn global_gain_clamps_and_mute_is_independent() {
        let mut output = AudioOutput::new(None, 1.5, false);
        assert_eq!(output.volume(), 1.0);
        assert_eq!(output.effective_gain(), 1.0);
        output.set_volume(-1.0);
        assert_eq!(output.volume(), 0.0);
        output.set_volume(0.65);
        output.set_muted(true);
        assert_eq!(output.volume(), 0.65);
        assert_eq!(output.effective_gain(), 0.0);
    }

    #[test]
    fn routing_plan_uses_specific_devices_and_falls_back_without_losing_preference() {
        let devices = vec!["Speakers".to_string(), "USB Headphones".to_string()];
        assert_eq!(planned_route(None, &devices), PlannedRoute::Default);
        assert_eq!(
            planned_route(Some("USB Headphones"), &devices),
            PlannedRoute::Preferred("USB Headphones")
        );
        assert_eq!(
            planned_route(Some("Bluetooth Headset"), &devices),
            PlannedRoute::DefaultFallback("Bluetooth Headset")
        );
    }

    #[test]
    fn output_enumeration_is_safe_even_without_hardware() {
        let _ = enumerate_output_devices();
    }
}
