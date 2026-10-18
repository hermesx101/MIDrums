# Hi-hats with shared open/closed stick notes

Some kits send the same stick-zone notes for open and closed hi-hat hits and
report pedal position separately. MI Drums can now interpret that position
without changing the module's calibration or note settings.

Enable **Use pedal position for open / closed hi-hat** before the first Setup
Wizard hit, or in Drum Mapping's Hi-Hat Details. Learn the bow and edge under
Hi-Hat Closed using **Use + Add Another**. The wizard then skips the separate
Open step. Learn the foot-only hit and pedal-position controller separately.
Save the mapping and move the pedal once after connecting or saving.

In Drum Mapping, adjust **Closed pedal threshold** and **Higher pedal values
mean closed** to suit the kit. This threshold changes only MI Drums' interpretation;
it does not recalibrate or send settings to the module. Intermediate positions
on the open side of the threshold count as Open; there is no separate half-open
scoring target in this implementation.

The MIDI Monitor's **Last played drum** retains the hit classification when
subsequent pedal updates arrive. Without a pedal reading, shared stick notes
remain unknown. Held positions do not expire. State is cleared on reconnect,
disconnect, profile save and application reset. Foot-only hits are not
reclassified, and pedal-position messages are never scored as hits.

## Measured Roland V51 / VH-14D example

The tested setup uses USB GENERIC mode, channel 10, shared bow/edge notes 46/26,
foot note 44 and pedal CC4. Closed stick hits arrived with pedal values 116--120;
open stick hits arrived with value 0. Threshold 90 separates these measured
positions. Test other positions before changing this threshold. Other kits or
module settings may use different notes, channels, controllers or direction.

```toml
device_name = "V51 Generic Mode"

[hi_hat_pedal]
closed_threshold = 90
high_values_closed = true

[[mappings]]
component = "Hi-Hat"
event_kind = "Note"
midi_note = 46
channel = 10
articulation = "Bow"

[[mappings]]
component = "Hi-Hat"
event_kind = "Note"
midi_note = 26
channel = 10
articulation = "Edge"

[[mappings]]
component = "Hi-Hat Pedal"
event_kind = "Note"
midi_note = 44
channel = 10
articulation = "Pedal"

[[mappings]]
component = "Hi-Hat Pedal Position"
event_kind = "ControlChange"
midi_note = 4
channel = 10
articulation = "Continuous"
cc_range = { min = 0, max = 127 }
```

When this mode is enabled, imported General MIDI exercises keep notes 42, 46
and 44 as Closed, Open and Foot respectively. Both stick targets are available
from a shared stick mapping. Lesson notes are interpreted as musical roles,
not as the hardware's custom note numbers. Legacy profiles without
`hi_hat_pedal` retain their previous mapping and lesson behavior.

## Implementation and verification

`mapping::MidiInputResolver` processes callback events in order and holds the
last mapped pedal sample per channel. It ignores unrelated controllers and
never uses a future sample to classify an earlier hit. The resolved component
is passed to `ActualHit` before scoring, preserving the original callback
timestamp and input-latency compensation.

`cargo test -p drums-core --test hi_hat_pedal` covers threshold boundaries,
inverted direction, controller/channel isolation, reconnect reset, old profile
compatibility, wizard flow, lesson roles and scoring. The sanitized fixtures
replay measured captures with 15 closed stick hits, 16 open stick hits and
10 foot hits (including one in the closed-state capture).

Two crash cymbals currently remain aliases of one logical Crash. Separate
Crash 1 / Crash 2 exercise targets are a different feature request.
