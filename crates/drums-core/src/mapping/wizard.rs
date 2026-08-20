//! Setup wizard state machine.
//!
//! This is *pure logic*: it knows nothing about the GUI frame loop, the MIDI
//! callback or the event channel. The GUI feeds timestamped `MidiEvent`s into
//! [`SetupWizard::handle_event`] and calls the navigation methods (`accept`,
//! `retry`, `skip`, `back`, `cancel`, `start`, `finish`). Because every input
//! is an explicit call, the whole state machine is unit-testable without any
//! hardware or rendering.
//!
//! ## Hit detection rules
//!
//! - Only **Note On with velocity > 0** is treated as a drum hit.
//! - Note On with velocity 0 follows the MIDI convention and is treated as a
//!   Note Off — it never becomes a hit.
//! - Note Off messages are ignored for hit detection.
//! - Timestamps come straight from the MIDI event (captured inside the
//!   real-time callback by `drums-core`); GUI timing is never introduced here.
//! - Only the **first** hit is captured while waiting for a component. Any
//!   additional Note On events arriving "close together" (within
//!   [`CONFLICT_WINDOW_MICROS`]) after it are recorded as *conflicting* hits so
//!   the user can see that multiple notes arrived — but a drum never receives
//!   several notes from one prompt. Nothing is accepted automatically.
//! - After a hit is accepted or retried, further hits within
//!   [`HIT_DEBOUNCE_MICROS`] are ignored, so a double-trigger or a flam from
//!   the previous pad cannot slide into the *next* component's prompt.

use super::{
    CcRange, DrumComponent, DrumKitConfiguration, DrumMapping, MappingArticulation,
    MappingEventKind, MappingInputIdentity, SUPPORTED_COMPONENTS,
};
use crate::midi::{MessageKind, MidiEvent};

/// After a hit is accepted/retried, Note On events arriving within this window
/// (relative to the original hit timestamp) are suppressed so they are not
/// mistaken for the next component's hit.
pub const HIT_DEBOUNCE_MICROS: u64 = 50 * 1000; // 50 ms

/// Window after the first captured hit during which additional Note On events
/// are collected as "conflicting" hits (shown to the user for confirmation).
pub const CONFLICT_WINDOW_MICROS: u64 = 250 * 1000; // 250 ms

/// A drum hit detected from a MIDI event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectedHit {
    pub event_kind: MappingEventKind,
    /// MIDI Note number or CC controller number, depending on `event_kind`.
    pub midi_note: u8,
    /// MIDI channel as displayed, 1..=16.
    pub channel: u8,
    /// Note velocity or CC controller value.
    pub velocity: u8,
    /// Original MIDI event timestamp in microseconds (no GUI timing).
    pub timestamp_micros: u64,
}

impl DetectedHit {
    /// Converts a MIDI event into a drum hit according to the rules in the
    /// module docs, or `None` when the event is not a positive-velocity Note On.
    pub fn from_event(ev: &MidiEvent) -> Option<Self> {
        let msg = &ev.message;
        let (event_kind, midi_note, velocity) = match msg.kind {
            MessageKind::NoteOn if msg.data2.unwrap_or(0) > 0 => {
                (MappingEventKind::Note, msg.data1?, msg.data2.unwrap_or(0))
            }
            MessageKind::ControlChange => (
                MappingEventKind::ControlChange,
                msg.data1?,
                msg.data2.unwrap_or(0),
            ),
            _ => return None,
        };
        if event_kind == MappingEventKind::Note && velocity == 0 {
            // Note On with zero velocity == Note Off (MIDI convention).
            return None;
        }
        let channel = msg.channel_number().unwrap_or(1);
        Some(Self {
            event_kind,
            midi_note,
            channel,
            velocity,
            timestamp_micros: ev.timestamp_micros,
        })
    }
}
/// The current state of the setup wizard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupWizardState {
    /// Shown before the user presses "Start setup".
    Intro,
    /// Waiting for the user to hit the given component.
    Detecting(DrumComponent),
    /// A hit has been captured; the user must accept or retry it.
    Confirming(DrumComponent),
    /// Every component was handled (accepted or skipped).
    Completed,
    /// The user cancelled; all collected results were discarded.
    Canceled,
}

/// Interactive setup-wizard state machine for mapping a drum kit.
#[derive(Clone, Debug)]
pub struct SetupWizard {
    state: SetupWizardState,
    /// Index of the current component in [`SUPPORTED_COMPONENTS`] while the
    /// wizard is running.
    index: usize,
    /// The hit awaiting confirmation, if any.
    detected: Option<DetectedHit>,
    /// Additional hits captured near the detected one (possible mistake).
    conflicting: Vec<DetectedHit>,
    /// A hit matching another component learned in this run or restored from
    /// the saved baseline configuration.
    rejected: Option<(DetectedHit, DrumComponent)>,
    /// Accepted physical inputs, indexed by logical component. Each component
    /// can own several aliases/zones.
    accepted: [Vec<DrumMapping>; SUPPORTED_COMPONENTS.len()],
    /// Whether each component has been handled (accepted *or* skipped).
    handled: [bool; SUPPORTED_COMPONENTS.len()],
    /// Ignore hits with timestamp <= this value (micros).
    suppress_until_micros: u64,
    /// Guard against calling [`Self::finish`] twice.
    finished: bool,
    /// Observed range while calibrating the current continuous controller.
    cc_range: Option<CcRange>,
    /// Most recent input used by the final test/review stage.
    test_hit: Option<DetectedHit>,
    test_component: Option<DrumComponent>,
    /// Saved configuration used only for conflict protection and preserving
    /// skipped components during a later re-run of setup.
    base_config: Option<DrumKitConfiguration>,
}

impl Default for SetupWizard {
    fn default() -> Self {
        Self::new()
    }
}

impl SetupWizard {
    /// Creates a fresh wizard in the `Intro` state with nothing captured.
    pub fn new() -> Self {
        Self {
            state: SetupWizardState::Intro,
            index: 0,
            detected: None,
            conflicting: Vec::new(),
            rejected: None,
            accepted: std::array::from_fn(|_| Vec::new()),
            handled: [false; SUPPORTED_COMPONENTS.len()],
            suppress_until_micros: 0,
            finished: false,
            cc_range: None,
            test_hit: None,
            test_component: None,
            base_config: None,
        }
    }

    /// Creates a wizard that safely edits an existing kit configuration.
    pub fn with_existing(config: DrumKitConfiguration) -> Self {
        let mut wizard = Self::new();
        wizard.base_config = Some(config);
        wizard
    }

    /// Starts (or restarts) the wizard from the first component.
    pub fn start(&mut self) {
        self.start_after(0);
    }

    /// Starts fresh and rejects every event at or before `boundary_micros`.
    pub fn start_after(&mut self, boundary_micros: u64) {
        self.state = SetupWizardState::Detecting(SUPPORTED_COMPONENTS[0]);
        self.index = 0;
        self.detected = None;
        self.conflicting.clear();
        self.rejected = None;
        self.accepted = std::array::from_fn(|_| Vec::new());
        self.handled = [false; SUPPORTED_COMPONENTS.len()];
        self.suppress_until_micros = boundary_micros;
        self.finished = false;
        self.cc_range = None;
        self.test_hit = None;
        self.test_component = None;
    }

    /// Cancels the run and discards everything collected so far.
    pub fn cancel(&mut self) {
        self.state = SetupWizardState::Canceled;
        self.detected = None;
        self.conflicting.clear();
        self.rejected = None;
        self.accepted = std::array::from_fn(|_| Vec::new());
        self.handled = [false; SUPPORTED_COMPONENTS.len()];
        self.suppress_until_micros = 0;
        self.cc_range = None;
        self.test_hit = None;
        self.test_component = None;
    }

    pub fn state(&self) -> SetupWizardState {
        self.state
    }

    /// The component the wizard is currently handling, if active.
    pub fn current_component(&self) -> Option<DrumComponent> {
        match self.state {
            SetupWizardState::Detecting(c) | SetupWizardState::Confirming(c) => Some(c),
            _ => None,
        }
    }

    /// The hit currently awaiting confirmation (if any).
    pub fn detected_hit(&self) -> Option<DetectedHit> {
        self.detected
    }

    /// Additional hits that arrived close to the detected one.
    pub fn conflicting_hits(&self) -> &[DetectedHit] {
        &self.conflicting
    }

    pub fn cc_range(&self) -> Option<CcRange> {
        self.cc_range
    }

    pub fn test_hit(&self) -> Option<DetectedHit> {
        self.test_hit
    }

    pub fn test_component(&self) -> Option<DrumComponent> {
        self.test_component
    }

    pub fn accepted_mappings(&self) -> impl Iterator<Item = &DrumMapping> {
        self.accepted.iter().flatten()
    }

    pub fn has_usable_mapping(&self) -> bool {
        self.accepted_mappings()
            .any(|mapping| mapping.event_kind == MappingEventKind::Note)
            || self
                .base_config
                .as_ref()
                .is_some_and(DrumKitConfiguration::is_usable)
    }

    /// Continuous CC calibration is only confirmable after the controller has
    /// produced at least two distinct values. A single unrelated CC message is
    /// not enough evidence to create a pedal-position mapping.
    pub fn can_accept(&self) -> bool {
        let Some(hit) = self.detected else {
            return false;
        };
        hit.event_kind == MappingEventKind::Note
            || self.cc_range.is_some_and(|range| range.min < range.max)
    }

    /// Latest hit rejected because it belongs to another component in this
    /// calibration or the saved baseline configuration.
    pub fn rejected_hit(&self) -> Option<(DetectedHit, DrumComponent)> {
        self.rejected
    }

    /// Establishes a fresh event boundary for the current step.
    pub fn begin_step_after(&mut self, boundary_micros: u64) {
        self.suppress_until_micros = boundary_micros;
        self.detected = None;
        self.conflicting.clear();
        self.rejected = None;
        self.cc_range = None;
    }

    /// Whether an event is a relevant, post-boundary input for this step.
    pub fn is_current_step_event(&self, ev: &MidiEvent) -> bool {
        let Some(hit) = DetectedHit::from_event(ev) else {
            return false;
        };
        let Some(component) = self.current_component() else {
            return false;
        };
        let expected_kind = if component == DrumComponent::HiHatPedalPosition {
            MappingEventKind::ControlChange
        } else {
            MappingEventKind::Note
        };
        hit.event_kind == expected_kind && hit.timestamp_micros > self.suppress_until_micros
    }

    /// Whether the given component has an accepted mapping so far.
    pub fn is_done(&self, component: DrumComponent) -> bool {
        SUPPORTED_COMPONENTS
            .iter()
            .position(|&c| c == component)
            .map(|i| !self.accepted[i].is_empty())
            .unwrap_or(false)
    }

    /// Whether a component has already been handled (accepted or skipped).
    pub fn is_handled(&self, component: DrumComponent) -> bool {
        SUPPORTED_COMPONENTS
            .iter()
            .position(|&c| c == component)
            .map(|i| self.handled[i])
            .unwrap_or(false)
    }

    /// Number of components accepted so far.
    pub fn done_count(&self) -> usize {
        self.accepted.iter().filter(|m| !m.is_empty()).count()
    }

    /// Whether the user can go back to a previous component.
    pub fn can_go_back(&self) -> bool {
        self.index > 0
            && matches!(
                self.state,
                SetupWizardState::Detecting(_)
                    | SetupWizardState::Confirming(_)
                    | SetupWizardState::Completed
            )
    }

    /// Feeds a MIDI event into the wizard. Returns `true` if a new hit was
    /// captured (the GUI may want to repaint immediately).
    pub fn handle_event(&mut self, ev: &MidiEvent) -> bool {
        let Some(hit) = DetectedHit::from_event(ev) else {
            return false;
        };
        if self.state == SetupWizardState::Completed {
            self.test_hit = Some(hit);
            self.test_component = self.accepted.iter().flatten().find_map(|mapping| {
                mapping
                    .matches(MappingInputIdentity {
                        event_kind: hit.event_kind,
                        number: hit.midi_note,
                        channel: hit.channel,
                    })
                    .then_some(mapping.component)
            });
            if self.test_component.is_none() {
                self.test_component = self.base_config.as_ref().and_then(|config| {
                    config.mappings.iter().find_map(|mapping| {
                        let component_was_replaced = SUPPORTED_COMPONENTS
                            .iter()
                            .position(|component| *component == mapping.component)
                            .is_some_and(|index| !self.accepted[index].is_empty());
                        (!component_was_replaced
                            && mapping.matches(MappingInputIdentity {
                                event_kind: hit.event_kind,
                                number: hit.midi_note,
                                channel: hit.channel,
                            }))
                        .then_some(mapping.component)
                    })
                });
            }
            return true;
        }
        let Some(component) = self.current_component() else {
            return false;
        };
        let expected_kind = if component == DrumComponent::HiHatPedalPosition {
            MappingEventKind::ControlChange
        } else {
            MappingEventKind::Note
        };
        if hit.event_kind != expected_kind {
            return false;
        }
        if hit.timestamp_micros <= self.suppress_until_micros {
            return false;
        }
        match self.state {
            SetupWizardState::Detecting(_) => {
                if let Some(existing) = self.accepted.iter().flatten().find(|mapping| {
                    mapping.event_kind == hit.event_kind
                        && mapping.midi_note == hit.midi_note
                        && mapping.channel == hit.channel
                }) {
                    self.rejected = Some((hit, existing.component));
                    return false;
                }
                if let Some(existing) = self.base_config.as_ref().and_then(|config| {
                    config.mappings.iter().find(|mapping| {
                        mapping.event_kind == hit.event_kind
                            && mapping.midi_note == hit.midi_note
                            && mapping.channel == hit.channel
                    })
                }) {
                    let owner_was_replaced = self.is_done(existing.component);
                    if existing.component != component && !owner_was_replaced {
                        self.rejected = Some((hit, existing.component));
                        return false;
                    }
                }
                self.detected = Some(hit);
                self.cc_range = (hit.event_kind == MappingEventKind::ControlChange)
                    .then(|| CcRange::new(hit.velocity, hit.velocity));
                self.conflicting.clear();
                self.rejected = None;
                self.state = SetupWizardState::Confirming(self.current_component().unwrap());
                true
            }
            SetupWizardState::Confirming(_) => {
                if let Some(base) = self.detected {
                    if base.event_kind == MappingEventKind::ControlChange
                        && hit.event_kind == base.event_kind
                        && hit.midi_note == base.midi_note
                        && hit.channel == base.channel
                    {
                        if let Some(range) = self.cc_range.as_mut() {
                            range.observe(hit.velocity);
                        }
                        self.detected = Some(hit);
                        return true;
                    }
                    let close = hit.timestamp_micros.saturating_sub(base.timestamp_micros)
                        <= CONFLICT_WINDOW_MICROS;
                    if close
                        && hit.midi_note != base.midi_note
                        && !self
                            .conflicting
                            .iter()
                            .any(|c| c.midi_note == hit.midi_note && c.channel == hit.channel)
                    {
                        self.conflicting.push(hit);
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Accepts the detected note and advances to the next component.
    pub fn accept(&mut self) {
        if self.capture_detected() {
            self.handled[self.index] = true;
            self.advance();
        }
    }

    /// Accepts the current input as an alias and listens for another input for
    /// the same logical drum instead of advancing.
    pub fn accept_and_add_another(&mut self) {
        if !self.capture_detected() {
            return;
        }
        self.detected = None;
        self.conflicting.clear();
        self.rejected = None;
        self.cc_range = None;
        self.state = SetupWizardState::Detecting(SUPPORTED_COMPONENTS[self.index]);
    }

    fn capture_detected(&mut self) -> bool {
        if !matches!(self.state, SetupWizardState::Confirming(_)) || !self.can_accept() {
            return false;
        }
        let Some(hit) = self.detected else {
            return false;
        };
        let component = self.current_component().unwrap();
        self.accepted[self.index].push(DrumMapping {
            component,
            event_kind: hit.event_kind,
            midi_note: hit.midi_note,
            channel: hit.channel,
            articulation: articulation_for(component),
            cc_range: self.cc_range,
        });
        self.suppress_until_micros = hit.timestamp_micros + HIT_DEBOUNCE_MICROS;
        true
    }

    /// Rejects the detected note and waits for a fresh hit on the same drum.
    pub fn retry(&mut self) {
        if !matches!(self.state, SetupWizardState::Confirming(_)) {
            return;
        }
        if let Some(hit) = self.detected.take() {
            self.suppress_until_micros = hit.timestamp_micros + HIT_DEBOUNCE_MICROS;
        }
        self.conflicting.clear();
        self.rejected = None;
        self.cc_range = None;
        self.state = SetupWizardState::Detecting(self.current_component().unwrap());
    }

    /// Skips the current component without mapping it.
    pub fn skip(&mut self) {
        if !matches!(
            self.state,
            SetupWizardState::Detecting(_) | SetupWizardState::Confirming(_)
        ) {
            return;
        }
        self.detected = None;
        self.conflicting.clear();
        self.rejected = None;
        self.handled[self.index] = true;
        self.advance();
    }

    /// Moves back to the previous component so it can be done again.
    /// Returns `true` if the wizard actually moved.
    pub fn back(&mut self) -> bool {
        if !self.can_go_back() {
            return false;
        }
        self.index -= 1;
        self.accepted[self.index].clear();
        self.handled[self.index] = false;
        self.detected = None;
        self.conflicting.clear();
        self.rejected = None;
        self.cc_range = None;
        self.suppress_until_micros = 0;
        self.state = SetupWizardState::Detecting(SUPPORTED_COMPONENTS[self.index]);
        true
    }

    /// Finalizes the wizard into a configured kit. Only valid once all
    /// components have been handled (`Completed`); skipped components are
    /// simply absent from the configuration. The device is not stored by the
    /// wizard itself — the caller supplies the MIDI device name it ran against.
    pub fn finish(&mut self, device_name: &str) -> Option<DrumKitConfiguration> {
        if self.state != SetupWizardState::Completed || self.finished {
            return None;
        }
        let config = self.configuration(device_name)?;
        self.finished = true;
        Some(config)
    }

    /// Builds the review-stage configuration without consuming/finalizing the
    /// wizard. This lets callers persist successfully before leaving setup.
    pub fn configuration(&self, device_name: &str) -> Option<DrumKitConfiguration> {
        if self.state != SetupWizardState::Completed || self.finished {
            return None;
        }
        let mut config = self
            .base_config
            .clone()
            .filter(|config| config.device_name == device_name)
            .unwrap_or_else(|| DrumKitConfiguration::new(device_name));
        for (index, component) in SUPPORTED_COMPONENTS.iter().copied().enumerate() {
            if self.accepted[index].is_empty() {
                continue;
            }
            config.remove(component);
            for mapping in &self.accepted[index] {
                let _ = config.add_input(*mapping);
            }
        }
        Some(config)
    }

    fn advance(&mut self) {
        self.detected = None;
        self.conflicting.clear();
        self.rejected = None;
        self.cc_range = None;
        self.index += 1;
        if self.index >= SUPPORTED_COMPONENTS.len() {
            self.state = SetupWizardState::Completed;
        } else {
            self.state = SetupWizardState::Detecting(SUPPORTED_COMPONENTS[self.index]);
        }
    }
}

fn articulation_for(component: DrumComponent) -> Option<MappingArticulation> {
    match component {
        DrumComponent::HiHat => Some(MappingArticulation::Closed),
        DrumComponent::HiHatOpen => Some(MappingArticulation::Open),
        DrumComponent::HiHatPedal => Some(MappingArticulation::Pedal),
        DrumComponent::HiHatPedalPosition => Some(MappingArticulation::Continuous),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{MessageKind, MidiMessage};

    fn note_on(ts: u64, channel: u8, note: u8, velocity: u8) -> MidiEvent {
        MidiEvent {
            message: MidiMessage {
                kind: MessageKind::NoteOn,
                channel: Some(channel), // 0-based like the parser produces
                data1: Some(note),
                data2: Some(velocity),
                system_exclusive: None,
            },
            timestamp_micros: ts,
        }
    }

    fn note_off(ts: u64, channel: u8, note: u8, velocity: u8) -> MidiEvent {
        MidiEvent {
            message: MidiMessage {
                kind: MessageKind::NoteOff,
                channel: Some(channel),
                data1: Some(note),
                data2: Some(velocity),
                system_exclusive: None,
            },
            timestamp_micros: ts,
        }
    }

    fn control_change(ts: u64, channel: u8, controller: u8, value: u8) -> MidiEvent {
        MidiEvent {
            message: MidiMessage {
                kind: MessageKind::ControlChange,
                channel: Some(channel),
                data1: Some(controller),
                data2: Some(value),
                system_exclusive: None,
            },
            timestamp_micros: ts,
        }
    }

    #[test]
    fn accepts_detected_midi_note() {
        let mut w = SetupWizard::new();
        w.start();
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Crash));

        // Any velocity is recorded; note 49 on channel 10 (0-based 9).
        assert!(w.handle_event(&note_on(1_000_000, 9, 49, 105)));
        assert_eq!(
            w.state(),
            SetupWizardState::Confirming(DrumComponent::Crash)
        );
        let hit = w.detected_hit().unwrap();
        assert_eq!(hit.midi_note, 49);
        assert_eq!(hit.channel, 10); // converted to 1-based display channel
        assert_eq!(hit.velocity, 105);
        assert_eq!(hit.timestamp_micros, 1_000_000); // original timestamp kept

        w.accept();
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Ride));
        assert!(w.is_done(DrumComponent::Crash));
        assert!(!w.is_done(DrumComponent::Ride));
        assert_eq!(w.done_count(), 1);
    }

    #[test]
    fn note_on_with_zero_velocity_is_treated_as_note_off() {
        let mut w = SetupWizard::new();
        w.start();
        // Note On with velocity 0 must NOT be interpreted as a hit.
        assert!(!w.handle_event(&note_on(5, 9, 49, 0)));
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Crash));
        assert!(w.detected_hit().is_none());
    }

    #[test]
    fn note_off_is_not_a_hit() {
        let mut w = SetupWizard::new();
        w.start();
        assert!(!w.handle_event(&note_off(7, 9, 49, 64)));
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Crash));
        assert!(w.detected_hit().is_none());
    }

    #[test]
    fn retry_discards_detected_note_and_waits_again() {
        let mut w = SetupWizard::new();
        w.start();
        w.handle_event(&note_on(1_000_000, 9, 49, 105));
        assert!(w.detected_hit().is_some());

        w.retry();
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Crash));
        assert!(w.detected_hit().is_none());

        // The same (flam-like) note within the debounce window is suppressed.
        assert!(!w.handle_event(&note_on(1_000_030, 9, 49, 104)));
        assert!(w.detected_hit().is_none());

        // A later, fresh hit is captured again.
        assert!(w.handle_event(&note_on(3_000_000, 9, 62, 90)));
        let hit = w.detected_hit().unwrap();
        assert_eq!(hit.midi_note, 62);
    }

    #[test]
    fn cancel_discards_everything() {
        let mut w = SetupWizard::new();
        w.start();
        w.handle_event(&note_on(100, 9, 49, 100));
        w.accept();
        assert_eq!(w.done_count(), 1);

        w.cancel();
        assert_eq!(w.state(), SetupWizardState::Canceled);
        assert_eq!(w.done_count(), 0);
        assert!(w.detected_hit().is_none());

        // No further inputs can happen after cancellation.
        assert!(!w.handle_event(&note_on(200, 9, 49, 100)));
        w.accept(); // no-op, must not panic or change anything
        assert_eq!(w.state(), SetupWizardState::Canceled);
    }

    #[test]
    fn completing_setup_builds_configuration() {
        // Notes chosen per component; deliberately NOT the typical GM layout to
        // prove the wizard learns whatever the device sends.
        let numbers = [49u8, 51, 42, 46, 44, 4, 38, 36, 48, 47, 45];
        let mut w = SetupWizard::new();
        w.start();

        let mut ts = 100u64;
        for (i, component) in SUPPORTED_COMPONENTS.iter().enumerate() {
            assert_eq!(w.current_component(), Some(*component));
            let event = if *component == DrumComponent::HiHatPedalPosition {
                let first = control_change(ts, 9, numbers[i], 20);
                assert!(w.handle_event(&first));
                let second = control_change(ts + 1, 9, numbers[i], 110);
                assert!(w.handle_event(&second));
                second
            } else {
                note_on(ts, 9, numbers[i], 100 + i as u8)
            };
            if *component != DrumComponent::HiHatPedalPosition {
                assert!(w.handle_event(&event));
            }
            w.accept();
            assert!(w.is_done(*component));
            ts += 100_000;
        }

        assert_eq!(w.state(), SetupWizardState::Completed);
        assert_eq!(w.done_count(), SUPPORTED_COMPONENTS.len());

        let config = w.finish("NUX DM-1").expect("finish should succeed");
        assert_eq!(config.device_name, "NUX DM-1");
        assert_eq!(config.mapped_count(), SUPPORTED_COMPONENTS.len());
        for (i, component) in SUPPORTED_COMPONENTS.iter().enumerate() {
            assert_eq!(config.note_for(*component), Some(numbers[i]));
            assert_eq!(config.channel_for(*component), Some(10));
        }
        assert_eq!(
            config
                .get(DrumComponent::HiHatPedalPosition)
                .unwrap()
                .event_kind,
            MappingEventKind::ControlChange
        );
    }
    #[test]
    fn skipping_a_component_advances_without_mapping() {
        let mut w = SetupWizard::new();
        w.start();
        // Crash connects, then the user skips Ride.
        w.handle_event(&note_on(100, 9, 49, 100));
        w.accept();
        w.skip();

        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::HiHat));
        assert!(w.is_done(DrumComponent::Crash));
        assert!(!w.is_done(DrumComponent::Ride));
        assert_eq!(w.done_count(), 1);

        // Skip the rest to reach the end.
        for _ in 0..SUPPORTED_COMPONENTS.len() - 2 {
            w.skip();
        }
        assert_eq!(w.state(), SetupWizardState::Completed);

        let config = w.finish("Kit").unwrap();
        assert_eq!(config.mapped_count(), 1);
        assert_eq!(config.note_for(DrumComponent::Crash), Some(49));
        assert!(config.get(DrumComponent::Ride).is_none());
    }

    #[test]
    fn close_notes_do_not_assign_multiple_notes_to_one_drum() {
        let mut w = SetupWizard::new();
        w.start();

        // First note captured.
        assert!(w.handle_event(&note_on(1_000_000, 9, 49, 100)));
        assert_eq!(w.detected_hit().unwrap().midi_note, 49);

        // A second, different note arriving 1 ms later is a "conflict", not a
        // second mapping: the wizard stays on the same single candidate.
        assert!(!w.handle_event(&note_on(1_001_000, 9, 51, 100)));
        assert_eq!(w.detected_hit().unwrap().midi_note, 49);
        assert_eq!(w.conflicting_hits().len(), 1);
        assert_eq!(w.conflicting_hits()[0].midi_note, 51);

        // Repeats of the same conflict are not recorded again.
        assert!(!w.handle_event(&note_on(1_002_000, 9, 51, 100)));
        assert_eq!(w.conflicting_hits().len(), 1);

        // Accepting maps only the original note.
        w.accept();
        assert!(w.is_done(DrumComponent::Crash));
        assert!(!w.is_done(DrumComponent::Ride));
        assert!(w.conflicting_hits().is_empty());
    }

    #[test]
    fn debounce_keeps_flam_out_of_next_prompt() {
        let mut w = SetupWizard::new();
        w.start();

        // Crash hit at t=1_000_000, accepted.
        w.handle_event(&note_on(1_000_000, 9, 49, 100));
        w.accept();
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Ride));

        // A flam on the crash pad 20 ms later must not be captured as Ride.
        assert!(!w.handle_event(&note_on(1_020_000, 9, 49, 99)));
        assert!(w.detected_hit().is_none());

        // A genuine Ride hit later is captured normally.
        assert!(w.handle_event(&note_on(2_000_000, 9, 51, 90)));
        assert_eq!(w.detected_hit().unwrap().midi_note, 51);
    }

    #[test]
    fn back_returns_to_previous_component() {
        let mut w = SetupWizard::new();
        w.start();
        w.handle_event(&note_on(3_000_000, 9, 49, 100));
        w.accept();
        w.handle_event(&note_on(4_000_000, 9, 51, 100));
        w.accept();
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::HiHat));

        assert!(w.can_go_back());
        assert!(w.back());
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Ride));
        // The previous mapping for Ride was invalidated for re-detection.
        assert!(!w.is_done(DrumComponent::Ride));
        assert!(w.is_done(DrumComponent::Crash));
    }

    #[test]
    fn cannot_finish_before_all_components_handled() {
        let mut w = SetupWizard::new();
        w.start();
        w.handle_event(&note_on(100, 9, 49, 100));
        w.accept();
        assert!(w.finish("Kit").is_none());
    }

    #[test]
    fn dismissive_non_note_events_are_ignored() {
        let mut w = SetupWizard::new();
        w.start();
        // Control change / program change must not trigger a hit.
        let cc = MidiEvent {
            message: MidiMessage {
                kind: MessageKind::ControlChange,
                channel: Some(9),
                data1: Some(1),
                data2: Some(127),
                system_exclusive: None,
            },
            timestamp_micros: 1,
        };
        assert!(!w.handle_event(&cc));
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Crash));
    }

    #[test]
    fn detected_hit_from_parser_output() {
        // End-to-end: bytes through the streaming parser yield an event that
        // the wizard recognises, using the parser's 1-based channel_number().
        let mut p = crate::midi::MidiParser::new();
        let events = p.push_bytes(123_456, &[0x99, 49, 105]); // ch 10 (0-based 9), note 49
        assert_eq!(events.len(), 1);
        let hit = DetectedHit::from_event(&events[0]).expect("hit expected");
        assert_eq!(hit.midi_note, 49);
        assert_eq!(hit.channel, 10);
        assert_eq!(hit.velocity, 105);
        assert_eq!(hit.timestamp_micros, 123_456);
    }

    #[test]
    fn wizard_order_is_explicit_and_stable() {
        assert_eq!(
            SUPPORTED_COMPONENTS,
            [
                DrumComponent::Crash,
                DrumComponent::Ride,
                DrumComponent::HiHat,
                DrumComponent::HiHatOpen,
                DrumComponent::HiHatPedal,
                DrumComponent::HiHatPedalPosition,
                DrumComponent::Snare,
                DrumComponent::Kick,
                DrumComponent::HiTom,
                DrumComponent::MidTom,
                DrumComponent::LowTom,
            ]
        );
    }

    #[test]
    fn fresh_step_boundary_rejects_stale_events() {
        let mut w = SetupWizard::new();
        w.start_after(1_000);
        let stale = note_on(999, 9, 41, 90);
        let boundary = note_on(1_000, 9, 41, 90);
        let fresh = note_on(1_001, 9, 55, 90);
        assert!(!w.is_current_step_event(&stale));
        assert!(!w.is_current_step_event(&boundary));
        assert!(w.is_current_step_event(&fresh));
        assert!(!w.handle_event(&stale));
        assert!(!w.handle_event(&boundary));
        assert!(w.detected_hit().is_none());
        assert!(w.handle_event(&fresh));
    }

    #[test]
    fn hit_learned_in_this_run_is_rejected_for_a_later_target() {
        let mut w = SetupWizard::new();
        w.start();
        w.handle_event(&note_on(100, 9, 55, 100));
        w.accept();
        assert_eq!(w.current_component(), Some(DrumComponent::Ride));

        assert!(!w.handle_event(&note_on(200_000, 9, 55, 80)));
        assert_eq!(w.state(), SetupWizardState::Detecting(DrumComponent::Ride));
        let (hit, component) = w.rejected_hit().expect("wrong hit should be retained");
        assert_eq!(hit.midi_note, 55);
        assert_eq!(component, DrumComponent::Crash);

        assert!(w.handle_event(&note_on(300_000, 9, 51, 90)));
        assert_eq!(w.state(), SetupWizardState::Confirming(DrumComponent::Ride));
    }

    #[test]
    fn wizard_keeps_multiple_aliases_for_one_logical_component() {
        let mut w = SetupWizard::new();
        w.start();
        assert!(w.handle_event(&note_on(100_000, 9, 49, 100)));
        w.accept_and_add_another();
        assert_eq!(w.current_component(), Some(DrumComponent::Crash));
        assert!(w.handle_event(&note_on(200_000, 9, 55, 90)));
        w.accept();
        assert_eq!(w.current_component(), Some(DrumComponent::Ride));
        let aliases: Vec<_> = w
            .accepted_mappings()
            .filter(|mapping| mapping.component == DrumComponent::Crash)
            .map(|mapping| mapping.midi_note)
            .collect();
        assert_eq!(aliases, vec![49, 55]);
    }

    #[test]
    fn continuous_controller_discovers_number_and_observed_range() {
        let mut w = SetupWizard::new();
        w.start();
        while w.current_component() != Some(DrumComponent::HiHatPedalPosition) {
            w.skip();
        }
        assert_eq!(
            w.current_component(),
            Some(DrumComponent::HiHatPedalPosition)
        );
        assert!(w.handle_event(&control_change(1_000_000, 3, 17, 96)));
        assert!(w.handle_event(&control_change(1_010_000, 3, 17, 12)));
        assert!(w.handle_event(&control_change(1_020_000, 3, 17, 121)));
        assert_eq!(w.cc_range(), Some(CcRange::new(12, 121)));
        w.accept();
        let mapping = w.accepted_mappings().next().unwrap();
        assert_eq!(mapping.midi_note, 17);
        assert_eq!(mapping.channel, 4);
        assert_eq!(mapping.cc_range, Some(CcRange::new(12, 121)));
    }

    #[test]
    fn completed_wizard_is_a_live_mapping_test_stage() {
        let mut w = SetupWizard::new();
        w.start();
        w.handle_event(&note_on(100, 9, 49, 100));
        w.accept();
        for _ in 1..SUPPORTED_COMPONENTS.len() {
            w.skip();
        }
        assert_eq!(w.state(), SetupWizardState::Completed);
        assert!(w.handle_event(&note_on(2_000_000, 9, 49, 88)));
        assert_eq!(w.test_component(), Some(DrumComponent::Crash));
        assert!(w.handle_event(&note_on(3_000_000, 9, 77, 88)));
        assert_eq!(w.test_component(), None);
    }

    #[test]
    fn rerun_protects_other_saved_inputs_and_preserves_skipped_components() {
        let mut existing = DrumKitConfiguration::new("kit");
        existing
            .add_input(DrumMapping {
                component: DrumComponent::Crash,
                event_kind: MappingEventKind::Note,
                midi_note: 49,
                channel: 10,
                articulation: None,
                cc_range: None,
            })
            .unwrap();
        existing
            .add_input(DrumMapping {
                component: DrumComponent::Ride,
                event_kind: MappingEventKind::Note,
                midi_note: 51,
                channel: 10,
                articulation: None,
                cc_range: None,
            })
            .unwrap();

        let mut w = SetupWizard::with_existing(existing);
        w.start();
        assert!(!w.handle_event(&note_on(100, 9, 51, 90)));
        assert_eq!(w.rejected_hit().unwrap().1, DrumComponent::Ride);
        assert!(w.handle_event(&note_on(200, 9, 55, 90)));
        w.accept();
        w.skip(); // Preserve the existing Ride mapping.
        while w.state() != SetupWizardState::Completed {
            w.skip();
        }
        let config = w.configuration("kit").unwrap();
        assert_eq!(config.note_for(DrumComponent::Crash), Some(55));
        assert_eq!(config.note_for(DrumComponent::Ride), Some(51));
        assert!(config.validate().is_ok());
    }
}
