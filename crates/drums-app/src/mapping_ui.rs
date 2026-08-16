//! Transactional manual reassignment for Note and CC MIDI identities.

use drums_core::mapping::{
    DrumComponent, DrumKitConfiguration, DrumMapping, MappingEventKind,
};
use drums_core::{MessageKind, MidiEvent};
use eframe::egui;

use crate::{DrumApp, View};

#[derive(Clone, Copy)]
struct InputIdentity {
    kind: MappingEventKind,
    number: u8,
    channel: u8,
    value: u8,
}

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.heading("DRUM MAPPING");
    ui.label("Mappings are changed only after you confirm the detected MIDI input.");
    if app.mapping_draft.is_none() {
        app.mapping_draft = app.active_config.clone().or_else(|| {
            app.backend
                .connected_port_name()
                .map(DrumKitConfiguration::new)
        });
    }
    let Some(mut draft) = app.mapping_draft.take() else {
        ui.weak("Connect a MIDI device before editing its mapping.");
        if ui.button("Open Setup Wizard").clicked() {
            app.view = View::Setup;
        }
        return;
    };
    let latest = app.mapping_last_input.clone();
    app.mapping_last_detected_component = latest
        .as_ref()
        .and_then(input_identity)
        .and_then(|identity| mapped_component(&draft, identity));
    if app.mapping_reassign_component.is_some() && app.mapping_reassign_candidate.is_none() {
        if let Some(event) = latest.as_ref().filter(|event| {
            event.timestamp_micros > app.mapping_reassign_after_micros
                && input_identity(event).is_some()
        }) {
            app.mapping_reassign_candidate = Some(event.clone());
        }
    }

    ui.label(egui::RichText::new(&draft.device_name).strong().size(20.0));
    last_input_panel(ui, latest.as_ref(), &draft);
    ui.add_space(8.0);
    ui.heading("CURRENT / ACTIVE DRUM");
    kit_overview(ui, app.mapping_last_detected_component);
    ui.add_space(10.0);

    if let Some(component) = app.mapping_reassign_component {
        reassignment_panel(app, ui, &mut draft, component);
        ui.add_space(12.0);
    } else if let Some(event) = latest.as_ref() {
        unknown_assignment_panel(app, ui, &draft, event);
    }

    ui.heading("DRUM MAPPINGS");
    mapping_cards(app, ui, &draft);

    ui.add_space(10.0);
    ui.heading("HI-HAT DETAILS");
    egui::Frame::group(ui.style())
        .inner_margin(12.0)
        .show(ui, |ui| {
            for component in [
                DrumComponent::HiHatOpen,
                DrumComponent::HiHatPedal,
                DrumComponent::HiHatPedalPosition,
            ] {
                if draft.get(component).is_none() {
                    continue;
                }
                mapping_row(app, ui, &draft, component);
                ui.separator();
            }
            let configured = [
                DrumComponent::HiHat,
                DrumComponent::HiHatOpen,
                DrumComponent::HiHatPedal,
                DrumComponent::HiHatPedalPosition,
            ]
            .into_iter()
            .filter(|component| draft.get(*component).is_some())
            .count();
            ui.weak(format!(
                "{configured} Hi-Hat articulations/controllers configured"
            ));
        });

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if ui
            .add_sized([150.0, 38.0], egui::Button::new("Save Mapping"))
            .clicked()
        {
            match app.store.save(&draft) {
                Ok(()) => {
                    app.active_config = Some(draft.clone());
                    app.status = "Drum mapping saved.".into();
                    app.rebuild_timeline();
                }
                Err(error) => app.status = format!("Could not save mapping: {error}"),
            }
        }
        if ui
            .add_sized([160.0, 38.0], egui::Button::new("Discard Changes"))
            .clicked()
        {
            draft = app
                .active_config
                .clone()
                .unwrap_or_else(|| DrumKitConfiguration::new(&draft.device_name));
            clear_reassignment(app);
        }
        if ui
            .add_sized([170.0, 38.0], egui::Button::new("Open Setup Wizard"))
            .clicked()
        {
            app.view = View::Setup;
        }
    });
    if !app.status.is_empty() {
        ui.weak(&app.status);
    }
    app.mapping_draft = Some(draft);
}

fn mapping_row(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    draft: &DrumKitConfiguration,
    component: DrumComponent,
) {
    let mapping = draft.get(component);
    ui.push_id(("mapping_row", component), |ui| {
        egui::Frame::group(ui.style())
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(component.state_label().unwrap_or(component.label()));
                    if let Some(mapping) = mapping {
                        ui.monospace(mapping_text(mapping));
                    } else {
                        ui.weak("Not configured");
                    }
                    if ui
                        .button(if mapping.is_some() {
                            "Reassign"
                        } else {
                            "Assign"
                        })
                        .clicked()
                    {
                        begin_reassignment(app, component);
                    }
                });
            });
    });
}

fn mapping_card(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    draft: &DrumKitConfiguration,
    component: DrumComponent,
) {
    let mapping = draft.get(component);
    ui.push_id(("mapping_card", component), |ui| {
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                // Width is controlled; height is deliberately left natural.
                ui.set_width(190.0);
                ui.strong(component.instrument_label());
                ui.add_space(6.0);
                if let Some(mapping) = mapping {
                    ui.monospace(format!("{} {}", mapping.event_kind.label(), mapping.midi_note));
                    ui.monospace(format!("Channel {}", mapping.channel));
                } else {
                    ui.weak("Not configured");
                    ui.add_space(18.0);
                }
                ui.add_space(8.0);
                if ui
                    .button(if mapping.is_some() { "Reassign" } else { "Assign" })
                    .clicked()
                {
                    begin_reassignment(app, component);
                }
            });
    });
}

fn mapping_cards(app: &mut DrumApp, ui: &mut egui::Ui, draft: &DrumKitConfiguration) {
    const ORDER: [DrumComponent; 8] = [
        DrumComponent::Crash,
        DrumComponent::Ride,
        DrumComponent::HiHat,
        DrumComponent::Snare,
        DrumComponent::Kick,
        DrumComponent::HiTom,
        DrumComponent::MidTom,
        DrumComponent::LowTom,
    ];
    let card_outer_width = 216.0;
    let columns = (ui.available_width() / card_outer_width).floor().max(1.0) as usize;
    for row in ORDER.chunks(columns) {
        ui.horizontal_top(|ui| {
            for component in row {
                mapping_card(app, ui, draft, *component);
            }
        });
        ui.add_space(8.0);
    }
}

fn begin_reassignment(app: &mut DrumApp, component: DrumComponent) {
    app.mapping_reassign_component = Some(component);
    app.mapping_reassign_after_micros = app
        .events
        .first()
        .map(|event| event.timestamp_micros)
        .unwrap_or(0);
    app.mapping_reassign_candidate = None;
    app.mapping_reassign_started = Some(std::time::Instant::now());
}

fn clear_reassignment(app: &mut DrumApp) {
    app.mapping_reassign_component = None;
    app.mapping_reassign_candidate = None;
    app.mapping_reassign_started = None;
}

fn reassignment_panel(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    draft: &mut DrumKitConfiguration,
    component: DrumComponent,
) {
    egui::Frame::group(ui.style()).fill(egui::Color32::from_rgb(28, 48, 64)).inner_margin(14.0).show(ui, |ui| {
        ui.heading("REASSIGNING");
        ui.strong(component.label());
        ui.label(reassignment_instruction(component));
        let candidate = app.mapping_reassign_candidate.as_ref().and_then(input_identity);
        let Some(identity) = candidate else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
            ui.spinner();
            let waiting = app.mapping_reassign_started.is_some_and(|started| started.elapsed().as_secs() >= 8);
            ui.label(if waiting { "Still waiting for MIDI input..." } else { "Listening for MIDI input..." });
            if ui.button("Cancel").clicked() { clear_reassignment(app); }
            return;
        };
        ui.colored_label(egui::Color32::from_rgb(90, 220, 140), "MIDI INPUT DETECTED");
        ui.monospace(identity_text(identity));
        if identity.kind == MappingEventKind::ControlChange {
            ui.add(
                egui::ProgressBar::new(identity.value as f32 / 127.0)
                    .text(format!("Pedal position: {} / 127", identity.value)),
            );
        }

        let expects_cc = component == DrumComponent::HiHatPedalPosition;
        if expects_cc && identity.kind == MappingEventKind::Note {
            ui.colored_label(egui::Color32::from_rgb(240, 190, 80), "A MIDI Note was detected. This may be a foot-hit event.");
            ui.horizontal(|ui| {
                if ui.button("Assign as Foot Hit").clicked() { app.mapping_reassign_component = Some(DrumComponent::HiHatPedal); }
                if ui.button("Cancel").clicked() { clear_reassignment(app); }
            });
            return;
        }
        if !expects_cc && identity.kind == MappingEventKind::ControlChange {
            ui.colored_label(egui::Color32::from_rgb(240, 190, 80), "Controller data is not a playable strike. Assign it as Pedal Position instead.");
            ui.horizontal(|ui| {
                if ui.button("Use as Pedal Position").clicked() { app.mapping_reassign_component = Some(DrumComponent::HiHatPedalPosition); }
                if ui.button("Cancel").clicked() { clear_reassignment(app); }
            });
            return;
        }

        let conflict = draft.mappings.iter().find(|mapping| {
            mapping.component != component
                && mapping.event_kind == identity.kind
                && mapping.midi_note == identity.number
                && mapping.channel == identity.channel
        }).copied();
        let new_mapping = DrumMapping { component, event_kind: identity.kind, midi_note: identity.number, channel: identity.channel };
        if let Some(existing) = conflict {
            ui.colored_label(egui::Color32::from_rgb(245, 110, 100), "MAPPING CONFLICT");
            ui.label(format!("{} is already assigned to {}.", mapping_text(&existing), existing.component.label()));
            ui.horizontal(|ui| {
                if ui.button("Reassign Anyway").clicked() {
                    draft.remove(existing.component);
                    draft.set(new_mapping);
                    clear_reassignment(app);
                }
                if ui.button("Cancel").clicked() { clear_reassignment(app); }
            });
        } else {
            ui.label(format!("New mapping: {} -> {}", component.label(), mapping_text(&new_mapping)));
            ui.horizontal(|ui| {
                if ui.button("Accept").clicked() { draft.set(new_mapping); clear_reassignment(app); }
                if ui.button("Cancel").clicked() { clear_reassignment(app); }
            });
        }
    });
}

fn unknown_assignment_panel(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    draft: &DrumKitConfiguration,
    event: &MidiEvent,
) {
    let Some(identity) = input_identity(event) else {
        return;
    };
    let known = draft.mappings.iter().any(|mapping| {
        mapping.event_kind == identity.kind
            && mapping.midi_note == identity.number
            && mapping.channel == identity.channel
    });
    if known {
        return;
    }
    egui::Frame::group(ui.style())
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.strong("UNKNOWN MIDI INPUT");
            ui.monospace(identity_text(identity));
            if identity.kind == MappingEventKind::ControlChange {
                ui.add(
                    egui::ProgressBar::new(identity.value as f32 / 127.0)
                        .text(format!("Pedal position: {} / 127", identity.value)),
                );
            }
            if identity.kind == MappingEventKind::ControlChange {
                ui.label("Compatible assignment: Hi-Hat Pedal Position");
                if ui.button("Assign").clicked() {
                    begin_reassignment_with_event(
                        app,
                        DrumComponent::HiHatPedalPosition,
                        event.clone(),
                    );
                }
            } else {
                egui::ComboBox::from_id_salt("unknown_assignment")
                    .selected_text(app.mapping_assign_component.label())
                    .show_ui(ui, |ui| {
                        for component in DrumComponent::ALL {
                            ui.selectable_value(
                                &mut app.mapping_assign_component,
                                component,
                                component.label(),
                            );
                        }
                    });
                if ui.button("Assign").clicked() {
                    begin_reassignment_with_event(app, app.mapping_assign_component, event.clone());
                }
            }
        });
}

fn kit_overview(ui: &mut egui::Ui, active: Option<DrumComponent>) {
    const ORDER: [DrumComponent; 8] = [
        DrumComponent::Crash,
        DrumComponent::Ride,
        DrumComponent::HiHat,
        DrumComponent::Snare,
        DrumComponent::Kick,
        DrumComponent::HiTom,
        DrumComponent::MidTom,
        DrumComponent::LowTom,
    ];
    let cell_width = 104.0;
    let columns = (ui.available_width() / cell_width).floor().max(1.0) as usize;
    for row in ORDER.chunks(columns) {
        ui.horizontal_top(|ui| {
            for component in row {
                ui.push_id(("active_drum", component), |ui| {
                    let (response, painter) =
                        ui.allocate_painter(egui::vec2(96.0, 72.0), egui::Sense::hover());
                    let is_active = active.is_some_and(|value| {
                        value == *component
                            || (*component == DrumComponent::HiHat
                                && matches!(
                                    value,
                                    DrumComponent::HiHatOpen
                                        | DrumComponent::HiHatPedal
                                        | DrumComponent::HiHatPedalPosition
                                ))
                    });
                    let center = response.rect.center_top() + egui::vec2(0.0, 27.0);
                    let fill = if is_active {
                        egui::Color32::from_rgb(80, 205, 245)
                    } else {
                        egui::Color32::from_rgb(65, 78, 96)
                    };
                    painter.circle_filled(center, 22.0, fill);
                    painter.circle_stroke(
                        center,
                        22.0,
                        egui::Stroke::new(
                            if is_active { 4.0_f32 } else { 2.0_f32 },
                            if is_active {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::from_gray(125)
                            },
                        ),
                    );
                    painter.text(
                        response.rect.center_bottom() - egui::vec2(0.0, 3.0),
                        egui::Align2::CENTER_BOTTOM,
                        component.instrument_label(),
                        egui::FontId::proportional(12.0),
                        if is_active {
                            egui::Color32::from_rgb(120, 220, 255)
                        } else {
                            ui.visuals().text_color()
                        },
                    );
                });
            }
        });
    }
}

fn begin_reassignment_with_event(app: &mut DrumApp, component: DrumComponent, event: MidiEvent) {
    begin_reassignment(app, component);
    app.mapping_reassign_candidate = Some(event);
}

fn input_identity(event: &MidiEvent) -> Option<InputIdentity> {
    let channel = event.message.channel_number()?;
    match event.message.kind {
        MessageKind::NoteOn if event.message.velocity().unwrap_or(0) > 0 => Some(InputIdentity {
            kind: MappingEventKind::Note,
            number: event.message.note()?,
            channel,
            value: event.message.velocity().unwrap_or(0),
        }),
        MessageKind::ControlChange => Some(InputIdentity {
            kind: MappingEventKind::ControlChange,
            number: event.message.data1?,
            channel,
            value: event.message.data2.unwrap_or(0),
        }),
        _ => None,
    }
}

fn mapped_component(
    config: &DrumKitConfiguration,
    identity: InputIdentity,
) -> Option<DrumComponent> {
    config.mappings.iter().find_map(|mapping| {
        (mapping.event_kind == identity.kind
            && mapping.midi_note == identity.number
            && mapping.channel == identity.channel)
            .then_some(mapping.component)
    })
}

fn mapping_text(mapping: &DrumMapping) -> String {
    format!(
        "{} {} | Channel {}",
        mapping.event_kind.label(),
        mapping.midi_note,
        mapping.channel
    )
}

fn identity_text(identity: InputIdentity) -> String {
    let value_label = if identity.kind == MappingEventKind::Note {
        "Velocity"
    } else {
        "Value"
    };
    format!(
        "Type: {} | {}: {} | Channel: {} | {value_label}: {}",
        identity.kind.label(),
        identity.kind.label(),
        identity.number,
        identity.channel,
        identity.value
    )
}

fn reassignment_instruction(component: DrumComponent) -> &'static str {
    match component {
        DrumComponent::HiHat => "Close the Hi-Hat and strike it.",
        DrumComponent::HiHatOpen => "Open the Hi-Hat and strike it.",
        DrumComponent::HiHatPedal => "Press the Hi-Hat pedal for a foot hit.",
        DrumComponent::HiHatPedalPosition => "Move the Hi-Hat pedal to send controller data.",
        _ => "Hit the drum you want to assign.",
    }
}

fn last_input_panel(ui: &mut egui::Ui, event: Option<&MidiEvent>, config: &DrumKitConfiguration) {
    egui::Frame::group(ui.style())
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.strong("LAST MIDI INPUT");
            let Some(event) = event else {
                ui.weak("Waiting for MIDI input...");
                return;
            };
            let Some(identity) = input_identity(event) else {
                ui.monospace(format!("Type: {}", event.message.kind.name()));
                return;
            };
            let (message_type, number_label, value_label) =
                if identity.kind == MappingEventKind::Note {
                    ("Note On", "Note", "Velocity")
                } else {
                    ("Control Change", "Controller", "Value")
                };
            ui.monospace(format!("Type: {message_type}"));
            ui.monospace(format!("{number_label}: {}", identity.number));
            ui.monospace(format!("Channel: {}", identity.channel));
            ui.monospace(format!("{value_label}: {}", identity.value));
            let interpretation = config
                .mappings
                .iter()
                .find(|mapping| {
                    mapping.event_kind == identity.kind
                        && mapping.midi_note == identity.number
                        && mapping.channel == identity.channel
                })
                .map(|mapping| mapping.component.label())
                .unwrap_or("UNMAPPED");
            ui.label(format!("Interpretation: {interpretation}"));
        });
}
