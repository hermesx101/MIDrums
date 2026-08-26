//! Shared Rodio output-device selection and application-wide output gain.

use std::{
    sync::{
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc,
    },
    thread,
};

use drums_core::ClickKind;
use rodio::{
    cpal::traits::{DeviceTrait, HostTrait},
    OutputStream, OutputStreamHandle,
};

use crate::click_audio::ClickAudio;

pub const DEFAULT_SYSTEM_DEVICE: &str = "Default System Device";

type RefreshResult = Result<AudioDeviceSnapshot, String>;
type DeviceEnumerator = Box<dyn Fn() -> RefreshResult + Send + 'static>;
type RepaintCallback = Arc<dyn Fn() + Send + Sync + 'static>;

struct NamedOutputDevice {
    name: String,
    device: rodio::cpal::Device,
}

/// A completed background query of available and default output devices.
///
/// Device handles travel with their display names so selecting or restoring a
/// route never has to enumerate again on the UI thread.
pub struct AudioDeviceSnapshot {
    devices: Vec<NamedOutputDevice>,
    default_device: Option<NamedOutputDevice>,
}

/// Owns one persistent CPAL enumeration thread.
///
/// The UI only sends a bounded request and polls completed results, so neither
/// operation can wait for CPAL. Keep all calls to `HostTrait::output_devices`
/// behind this worker; synchronous enumeration can freeze egui on slow audio
/// backends.
pub struct AudioDeviceRefreshWorker {
    request_tx: SyncSender<()>,
    result_rx: Receiver<RefreshResult>,
    refresh_in_flight: bool,
}

impl AudioDeviceRefreshWorker {
    pub fn new(repaint: RepaintCallback) -> Self {
        Self::with_enumerator(repaint, Box::new(enumerate_output_device_snapshot))
    }

    fn with_enumerator(repaint: RepaintCallback, enumerate: DeviceEnumerator) -> Self {
        let (request_tx, request_rx) = mpsc::sync_channel(1);
        let (result_tx, result_rx) = mpsc::channel();
        let _ = thread::Builder::new()
            .name("audio-device-refresh".into())
            .spawn(move || {
                while request_rx.recv().is_ok() {
                    let result = enumerate();
                    if result_tx.send(result).is_err() {
                        break;
                    }
                    repaint();
                }
            });
        Self {
            request_tx,
            result_rx,
            refresh_in_flight: false,
        }
    }

    /// Returns immediately. A second request is coalesced while one is active.
    pub fn request_refresh(&mut self) -> bool {
        if self.refresh_in_flight {
            return false;
        }
        match self.request_tx.try_send(()) {
            Ok(()) => {
                self.refresh_in_flight = true;
                true
            }
            Err(TrySendError::Full(_)) => {
                self.refresh_in_flight = true;
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn is_refreshing(&self) -> bool {
        self.refresh_in_flight
    }

    /// Non-blocking result consumption. Only one request can be active, but
    /// draining defensively ensures the newest completed snapshot wins.
    pub fn try_take_latest(&mut self) -> Option<RefreshResult> {
        let mut latest = None;
        loop {
            match self.result_rx.try_recv() {
                Ok(result) => latest = Some(result),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.refresh_in_flight = false;
                    break;
                }
            }
        }
        if latest.is_some() {
            self.refresh_in_flight = false;
        }
        latest
    }
}

/// Application-wide output route and gain shared by clicks and backing audio.
///
/// A missing preferred device uses the system default without forgetting the
/// preference, allowing a later background snapshot to restore it.
pub struct AudioOutput {
    preferred_device: Option<String>,
    available_devices: Vec<String>,
    devices: Vec<NamedOutputDevice>,
    default_device: Option<NamedOutputDevice>,
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
        let preferred_device = normalize_preference(preferred_device);
        let mut output = Self {
            using_fallback: preferred_device.is_some(),
            preferred_device,
            available_devices: Vec::new(),
            devices: Vec::new(),
            default_device: None,
            active_device: None,
            volume: volume.clamp(0.0, 1.0),
            muted,
            notice: None,
            _stream: None,
            handle: None,
            clicks: ClickAudio::new(),
        };
        // Opening the default endpoint does not call `HostTrait::output_devices`
        // and keeps startup usable while the worker resolves the saved route.
        output.open_initial_default();
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
        let route_changed = self.route_needs_refresh();
        if route_changed {
            self.reopen_from_snapshot();
        } else {
            self.update_route_status();
        }
        RouteUpdate {
            route_changed,
            notice: self.notice.clone(),
        }
    }

    /// Applies a snapshot already produced by the worker. This performs no
    /// device enumeration and therefore remains safe to call from egui.
    pub fn apply_device_snapshot(&mut self, snapshot: AudioDeviceSnapshot) -> RouteUpdate {
        self.available_devices = snapshot
            .devices
            .iter()
            .map(|device| device.name.clone())
            .collect();
        self.devices = snapshot.devices;
        self.default_device = snapshot.default_device;
        let route_needs_refresh = self.route_needs_refresh();
        if route_needs_refresh {
            self.reopen_from_snapshot();
        } else {
            self.update_route_status();
        }
        RouteUpdate {
            route_changed: route_needs_refresh,
            notice: self.notice.clone(),
        }
    }

    pub fn apply_refresh_error(&mut self, error: String) -> RouteUpdate {
        let notice = format!("Could not refresh audio output devices: {error}");
        self.notice = Some(notice.clone());
        RouteUpdate {
            route_changed: false,
            notice: Some(notice),
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

    fn open_initial_default(&mut self) {
        match open_default_device() {
            Ok((device, stream, handle)) => {
                self.active_device = Some(device.name.clone());
                self.default_device = Some(device);
                self.handle = Some(handle.clone());
                self._stream = Some(stream);
                self.clicks.set_output(Some(handle), self.effective_gain());
                if self.preferred_device.is_some() {
                    self.notice =
                        Some("Checking selected audio device. Using Default System Device.".into());
                }
            }
            Err(error) => {
                self.notice = Some(format!("Audio output unavailable: {error}"));
            }
        }
    }

    fn route_needs_refresh(&self) -> bool {
        let default_name = self
            .default_device
            .as_ref()
            .map(|device| device.name.as_str());
        match planned_route(self.preferred_device.as_deref(), &self.available_devices) {
            PlannedRoute::Preferred(preferred) => {
                self.using_fallback || self.active_device.as_deref() != Some(preferred)
            }
            PlannedRoute::DefaultFallback(_) => {
                !self.using_fallback || self.active_device.as_deref() != default_name
            }
            PlannedRoute::Default => {
                self.using_fallback
                    || self.active_device.as_deref() != default_name
                    || self.handle.is_none()
            }
        }
    }

    fn update_route_status(&mut self) {
        match planned_route(self.preferred_device.as_deref(), &self.available_devices) {
            PlannedRoute::Preferred(_) | PlannedRoute::Default => {
                self.using_fallback = false;
                self.notice = None;
            }
            PlannedRoute::DefaultFallback(preferred) => {
                self.using_fallback = true;
                self.notice = Some(format!(
                    "Selected audio device unavailable. Using Default System Device. ({preferred} is not connected)"
                ));
            }
        }
    }

    fn reopen_from_snapshot(&mut self) {
        self.clicks.set_output(None, self.effective_gain());
        self.handle = None;
        self._stream = None;
        self.active_device = None;
        self.using_fallback = false;
        self.notice = None;

        let planned = planned_route(self.preferred_device.as_deref(), &self.available_devices);
        let opened = match planned {
            PlannedRoute::Preferred(name) => self
                .devices
                .iter()
                .find(|device| device.name == name)
                .ok_or_else(|| format!("{name} is not connected"))
                .and_then(open_device),
            PlannedRoute::DefaultFallback(preferred) => {
                self.using_fallback = true;
                let opened = self
                    .default_device
                    .as_ref()
                    .ok_or_else(|| {
                        "no default output device was reported by the operating system".to_string()
                    })
                    .and_then(open_device);
                if opened.is_ok() {
                    self.notice = Some(format!(
                        "Selected audio device unavailable. Using Default System Device. ({preferred} is not connected)"
                    ));
                }
                opened
            }
            PlannedRoute::Default => self
                .default_device
                .as_ref()
                .ok_or_else(|| {
                    "no default output device was reported by the operating system".to_string()
                })
                .and_then(open_device),
        };

        match opened {
            Ok((stream, handle, name)) => {
                self.active_device = Some(name);
                self.handle = Some(handle.clone());
                self._stream = Some(stream);
                self.clicks.set_output(Some(handle), self.effective_gain());
            }
            Err(error) => {
                let fallback = self.using_fallback;
                self.notice = Some(if fallback {
                    format!(
                        "Selected audio device unavailable; Default System Device also failed ({error})."
                    )
                } else {
                    format!("Audio output unavailable: {error}")
                });
            }
        }
    }
}

fn enumerate_output_device_snapshot() -> RefreshResult {
    let host = rodio::cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|error| format!("device enumeration failed: {error}"))?;
    let mut devices = devices
        .filter_map(|device| {
            device
                .name()
                .ok()
                .map(|name| NamedOutputDevice { name, device })
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| left.name.cmp(&right.name));
    devices.dedup_by(|left, right| left.name == right.name);
    let default_device = host
        .default_output_device()
        .map(|device| NamedOutputDevice {
            name: device
                .name()
                .unwrap_or_else(|_| DEFAULT_SYSTEM_DEVICE.to_string()),
            device,
        });
    Ok(AudioDeviceSnapshot {
        devices,
        default_device,
    })
}

fn open_default_device() -> Result<(NamedOutputDevice, OutputStream, OutputStreamHandle), String> {
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
    Ok((NamedOutputDevice { name, device }, stream, handle))
}

fn open_device(
    device: &NamedOutputDevice,
) -> Result<(OutputStream, OutputStreamHandle, String), String> {
    let (stream, handle) = OutputStream::try_from_device(&device.device)
        .map_err(|error| format!("could not open {}: {error}", device.name))?;
    Ok((stream, handle, device.name.clone()))
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
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    fn empty_snapshot() -> AudioDeviceSnapshot {
        AudioDeviceSnapshot {
            devices: Vec::new(),
            default_device: None,
        }
    }

    fn wait_for_result(worker: &mut AudioDeviceRefreshWorker) -> RefreshResult {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(result) = worker.try_take_latest() {
                return result;
            }
            assert!(Instant::now() < deadline, "audio refresh worker timed out");
            thread::yield_now();
        }
    }

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
    fn refresh_request_is_non_blocking_and_repeated_requests_are_coalesced() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker_calls = Arc::clone(&calls);
        let mut worker = AudioDeviceRefreshWorker::with_enumerator(
            Arc::new(|| {}),
            Box::new(move || {
                worker_calls.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(empty_snapshot())
            }),
        );

        assert!(worker.request_refresh());
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(worker.is_refreshing());
        assert!(worker.try_take_latest().is_none());
        assert!(!worker.request_refresh());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        release_tx.send(()).unwrap();
        assert!(wait_for_result(&mut worker).is_ok());
        assert!(!worker.is_refreshing());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn completed_snapshot_reaches_ui_and_requests_repaint() {
        let repaints = Arc::new(AtomicUsize::new(0));
        let repaint_count = Arc::clone(&repaints);
        let mut worker = AudioDeviceRefreshWorker::with_enumerator(
            Arc::new(move || {
                repaint_count.fetch_add(1, Ordering::SeqCst);
            }),
            Box::new(|| Ok(empty_snapshot())),
        );
        assert!(worker.request_refresh());
        assert!(wait_for_result(&mut worker).is_ok());
        let deadline = Instant::now() + Duration::from_secs(1);
        while repaints.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "repaint callback timed out");
            thread::yield_now();
        }
        assert_eq!(repaints.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn enumeration_failure_is_delivered_without_discarding_the_worker() {
        let mut worker = AudioDeviceRefreshWorker::with_enumerator(
            Arc::new(|| {}),
            Box::new(|| Err("simulated enumeration failure".into())),
        );
        assert!(worker.request_refresh());
        match wait_for_result(&mut worker) {
            Err(error) => assert_eq!(error, "simulated enumeration failure"),
            Ok(_) => panic!("simulated enumeration failure was not delivered"),
        }
        assert!(!worker.is_refreshing());
        assert!(worker.request_refresh());
        assert!(wait_for_result(&mut worker).is_err());
    }
}
