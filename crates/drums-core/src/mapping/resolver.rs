//! Stateful interpretation of shared hi-hat notes. Pedal CC is never a hit.

use serde::{Deserialize, Serialize};

use super::{
    DrumComponent, DrumKitConfiguration, MappingEventKind, MappingInputIdentity, ObservedMidiInput,
};
use crate::midi::MidiEvent;

/// Optional raw-pedal threshold; defaults suit the measured V51/VH-14D input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiHatPedalMode {
    pub closed_threshold: u8,
    pub high_values_closed: bool,
}

impl Default for HiHatPedalMode {
    fn default() -> Self {
        Self {
            closed_threshold: 90,
            high_values_closed: true,
        }
    }
}

impl HiHatPedalMode {
    fn component(self, value: u8) -> DrumComponent {
        let closed = if self.high_values_closed {
            value >= self.closed_threshold
        } else {
            value <= self.closed_threshold
        };
        if closed {
            DrumComponent::HiHat
        } else {
            DrumComponent::HiHatOpen
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PedalSample {
    identity: MappingInputIdentity,
    value: u8,
    timestamp_micros: u64,
}

/// Feed events in callback order, once each. Reset when device/profile changes.
/// A held pedal remains valid until another value arrives; missing pedal state
/// resolves shared stick notes to unknown rather than guessing their state.
#[derive(Clone, Debug, Default)]
pub struct MidiInputResolver {
    pedals: [Option<PedalSample>; 16],
}

impl MidiInputResolver {
    pub fn reset(&mut self) {
        self.pedals = [None; 16];
    }

    pub fn observe(
        &mut self,
        event: &MidiEvent,
        config: &DrumKitConfiguration,
    ) -> Option<DrumComponent> {
        let input = ObservedMidiInput::from_event(event)?;
        let mapping = config.mapping_for_input(input.identity())?;
        if mapping.event_kind == MappingEventKind::ControlChange
            && mapping.component == DrumComponent::HiHatPedalPosition
        {
            let slot = self
                .pedals
                .get_mut(usize::from(input.channel.checked_sub(1)?))?;
            if slot.is_none_or(|sample| event.timestamp_micros >= sample.timestamp_micros) {
                *slot = Some(PedalSample {
                    identity: input.identity(),
                    value: input.value,
                    timestamp_micros: event.timestamp_micros,
                });
            }
        }
        self.resolve(event, config)
    }

    pub fn resolve(
        &self,
        event: &MidiEvent,
        config: &DrumKitConfiguration,
    ) -> Option<DrumComponent> {
        let input = ObservedMidiInput::from_event(event)?;
        let mapping = config.mapping_for_input(input.identity())?;
        let Some(mode) = config.hi_hat_pedal else {
            return Some(mapping.component);
        };
        if input.event_kind != MappingEventKind::Note
            || !matches!(
                mapping.component,
                DrumComponent::HiHat | DrumComponent::HiHatOpen
            )
        {
            return Some(mapping.component);
        }
        let sample = self
            .pedals
            .get(usize::from(input.channel.checked_sub(1)?))?
            .as_ref()?;
        if sample.timestamp_micros > event.timestamp_micros {
            return None;
        }
        let controller = config.mapping_for_input(sample.identity)?;
        if controller.component != DrumComponent::HiHatPedalPosition {
            return None;
        }
        Some(mode.component(sample.value))
    }
}
