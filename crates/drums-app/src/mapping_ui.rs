//! Transactional manual reassignment for Note and CC MIDI identities.

use drums_core::mapping::{
    CcRange, DrumComponent, DrumKitConfiguration, DrumMapping, MappingEventKind, ObservedMidiInput,
};
use drums_core::MidiEvent;
use eframe::egui;

use crate::{DrumApp, View};

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.heading("DRUM MAPPING");
    ui.weak("Advanced configuration and inspection. Changes apply only after Save Mapping.");
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
        .and_then(ObservedMidiInput::from_event)
        .and_then(|input| draft.resolve(input.identity()));
    if app.mapping_reassign_component.is_some() && app.mapping_reassign_candidate.is_none() {
        if let Some(event) = latest.as_ref().filter(|event| {
            event.timestamp_micros > app.mapping_reassign_after_micros
                && ObservedMidiInput::from_event(event).is_some()
        }) {
            app.mapping_reassign_candidate = Some(event.clone());
        }
    }

    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("MIDI DEVICE");
                ui.label(&draft.device_name);
                ui.colored_label(egui::Color32::from_rgb(90, 210, 135), "Connected");
                ui.separator();
                ui.weak(format!(
                    "{} logical drums / {} MIDI inputs",
                    draft.mapped_component_count(),
                    draft.mapped_count()
                ));
            });
        });
    ui.add_space(6.0);
    last_input_panel(ui, latest.as_ref(), &draft);
    ui.horizontal(|ui| {
        ui.strong("CURRENT / ACTIVE DRUM");
        ui.label(
            app.mapping_last_detected_component
                .map(DrumComponent::label)
                .unwrap_or("Waiting for input..."),
        );
    });
    ui.add_space(10.0);

    if let Some(component) = app.mapping_reassign_component {
        reassignment_panel(app, ui, &mut draft, component);
        ui.add_space(12.0);
    } else if let Some(event) = latest.as_ref() {
        unknown_assignment_panel(app, ui, &draft, event);
    }

    ui.heading("DRUM MAPPINGS");
    mapping_list(app, ui, &mut draft);

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
                if draft.mappings_for(component).next().is_none() {
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
        if ui
            .add_sized([160.0, 38.0], egui::Button::new("Open MIDI Monitor"))
            .clicked()
        {
            app.view = View::Monitor;
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
    let mappings: Vec<_> = draft.mappings_for(component).collect();
    ui.push_id(("mapping_row", component), |ui| {
        egui::Frame::group(ui.style())
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(component.state_label().unwrap_or(component.label()));
                    if mappings.is_empty() {
                        ui.weak("Not configured");
                    } else {
                        for mapping in &mappings {
                            ui.monospace(mapping_text(mapping));
                        }
                    }
                    if ui
                        .button(if mappings.is_empty() {
                            "Assign"
                        } else {
                            "Reassign"
                        })
                        .clicked()
                    {
                        begin_reassignment(app, component, !mappings.is_empty());
                    }
                    if !mappings.is_empty() && ui.button("Add Input").clicked() {
                        begin_reassignment(app, component, false);
                    }
                });
            });
    });
}

fn mapping_component_row(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    draft: &mut DrumKitConfiguration,
    component: DrumComponent,
) {
    ui.push_id(("mapping_component", component), |ui| {
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.set_min_width(135.0);
                    ui.strong(component.instrument_label());
                    let aliases: Vec<_> = draft.mappings_for(component).copied().collect();
                    if aliases.is_empty() {
                        ui.weak("Not configured");
                    } else {
                        for mapping in &aliases {
                            let text = mapping_text(mapping);
                            egui::Frame::default()
                                .fill(ui.visuals().faint_bg_color)
                                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                                .corner_radius(3.0)
                                .inner_margin(egui::Margin::symmetric(7, 3))
                                .show(ui, |ui| {
                                    ui.monospace(text);
                                    if ui.small_button("Remove").clicked() {
                                        draft.remove_input(mapping.identity());
                                    }
                                });
                        }
                    }
                    if ui.small_button("Add Input").clicked() {
                        begin_reassignment(app, component, false);
                    }
                    if !aliases.is_empty() && ui.small_button("Reassign All").clicked() {
                        begin_reassignment(app, component, true);
                    }
                });
            });
    });
}

fn mapping_list(app: &mut DrumApp, ui: &mut egui::Ui, draft: &mut DrumKitConfiguration) {
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
    for component in ORDER {
        mapping_component_row(app, ui, draft, component);
        ui.add_space(4.0);
    }
}

fn begin_reassignment(app: &mut DrumApp, component: DrumComponent, replace_component: bool) {
    app.mapping_reassign_component = Some(component);
    app.mapping_replace_component = replace_component;
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
    app.mapping_replace_component = false;
    app.mapping_reassign_candidate = None;
    app.mapping_reassign_started = None;
}

fn reassignment_panel(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    draft: &mut DrumKitConfiguration,
    component: DrumComponent,
) {
    egui::Frame::group(ui.style()).fill(ui.visuals().faint_bg_color).stroke(egui::Stroke::new(1.0_f32, app.accent_color())).inner_margin(14.0).show(ui, |ui| {
        ui.heading("REASSIGNING");
        ui.strong(component.label());
        ui.label(reassignment_instruction(component));
        let candidate = app
            .mapping_reassign_candidate
            .as_ref()
            .and_then(ObservedMidiInput::from_event);
        let Some(input) = candidate else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
            ui.spinner();
            let waiting = app.mapping_reassign_started.is_some_and(|started| started.elapsed().as_secs() >= 8);
            ui.label(if waiting { "Still waiting for MIDI input..." } else { "Listening for MIDI input..." });
            if ui.button("Cancel").clicked() { clear_reassignment(app); }
            return;
        };
        ui.colored_label(egui::Color32::from_rgb(90, 220, 140), "MIDI INPUT DETECTED");
        ui.monospace(input_text(input));
        if input.event_kind == MappingEventKind::ControlChange {
            let range = observed_cc_range(
                &app.events,
                input,
                app.mapping_reassign_after_micros,
            );
            ui.add(
                egui::ProgressBar::new(input.value as f32 / 127.0)
                    .text(format!("Pedal position: {} / 127", input.value)),
            );
            ui.monospace(format!(
                "Observed controller range: {}–{}",
                range.min, range.max
            ));
            ui.weak("Move the pedal fully open and fully closed before accepting.");
        }

        let expects_cc = component == DrumComponent::HiHatPedalPosition;
        if expects_cc && input.event_kind == MappingEventKind::Note {
            ui.colored_label(egui::Color32::from_rgb(240, 190, 80), "A MIDI Note was detected. This may be a foot-hit event.");
            ui.horizontal(|ui| {
                if ui.button("Assign as Foot Hit").clicked() { app.mapping_reassign_component = Some(DrumComponent::HiHatPedal); }
                if ui.button("Cancel").clicked() { clear_reassignment(app); }
            });
            return;
        }
        if !expects_cc && input.event_kind == MappingEventKind::ControlChange {
            ui.colored_label(egui::Color32::from_rgb(240, 190, 80), "Controller data is not a playable strike. Assign it as Pedal Position instead.");
            ui.horizontal(|ui| {
                if ui.button("Use as Pedal Position").clicked() { app.mapping_reassign_component = Some(DrumComponent::HiHatPedalPosition); }
                if ui.button("Cancel").clicked() { clear_reassignment(app); }
            });
            return;
        }

        let conflict = draft.mappings.iter().find(|mapping| {
            mapping.component != component
                && mapping.matches(input.identity())
        }).copied();
        let cc_range = (input.event_kind == MappingEventKind::ControlChange).then(|| {
            observed_cc_range(
                &app.events,
                input,
                app.mapping_reassign_after_micros,
            )
        });
        let new_mapping = DrumMapping {
            component,
            event_kind: input.event_kind,
            midi_note: input.number,
            channel: input.channel,
            articulation: component.default_articulation(),
            cc_range,
        };
        let calibration_ready = input.event_kind == MappingEventKind::Note
            || cc_range.is_some_and(|range| range.min < range.max);
        if !calibration_ready {
            ui.colored_label(
                egui::Color32::from_rgb(240, 190, 80),
                "Move the controller through its range before accepting.",
            );
        }
        if let Some(existing) = conflict {
            ui.colored_label(egui::Color32::from_rgb(245, 110, 100), "MAPPING CONFLICT");
            ui.label(format!("{} is already assigned to {}.", mapping_text(&existing), existing.component.label()));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(calibration_ready, egui::Button::new("Reassign Anyway"))
                    .clicked()
                {
                    if app.mapping_replace_component {
                        draft.remove(component);
                    }
                    draft.reassign_input(new_mapping);
                    clear_reassignment(app);
                }
                if ui.button("Cancel").clicked() { clear_reassignment(app); }
            });
        } else {
            ui.label(format!("New mapping: {} -> {}", component.label(), mapping_text(&new_mapping)));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(calibration_ready, egui::Button::new("Accept"))
                    .clicked()
                {
                    if app.mapping_replace_component {
                        draft.remove(component);
                    }
                    let _ = draft.add_input(new_mapping);
                    clear_reassignment(app);
                }
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
    let Some(input) = ObservedMidiInput::from_event(event) else {
        return;
    };
    let known = draft.resolve(input.identity()).is_some();
    if known {
        return;
    }
    egui::Frame::group(ui.style())
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.strong("UNKNOWN MIDI INPUT");
            ui.monospace(input_text(input));
            if input.event_kind == MappingEventKind::ControlChange {
                ui.add(
                    egui::ProgressBar::new(input.value as f32 / 127.0)
                        .text(format!("Pedal position: {} / 127", input.value)),
                );
            }
            if input.event_kind == MappingEventKind::ControlChange {
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

fn begin_reassignment_with_event(app: &mut DrumApp, component: DrumComponent, event: MidiEvent) {
    begin_reassignment(app, component, false);
    app.mapping_reassign_candidate = Some(event);
}

fn observed_cc_range(events: &[MidiEvent], input: ObservedMidiInput, after_micros: u64) -> CcRange {
    let mut range = CcRange::new(input.value, input.value);
    for event in events {
        let Some(observed) = ObservedMidiInput::from_event(event) else {
            continue;
        };
        if event.timestamp_micros > after_micros
            && observed.event_kind == MappingEventKind::ControlChange
            && observed.identity() == input.identity()
        {
            range.observe(observed.value);
        }
    }
    range
}

fn mapping_text(mapping: &DrumMapping) -> String {
    let articulation = mapping
        .articulation
        .map(|value| format!("{}: ", value.label()))
        .unwrap_or_default();
    let range = mapping
        .cc_range
        .map(|value| format!(" | Range {}–{}", value.min, value.max))
        .unwrap_or_default();
    format!(
        "{articulation}{} {} | Ch {}{range}",
        mapping.event_kind.label(),
        mapping.midi_note,
        mapping.channel
    )
}

fn input_text(input: ObservedMidiInput) -> String {
    let value_label = if input.event_kind == MappingEventKind::Note {
        "Velocity"
    } else {
        "Value"
    };
    format!(
        "Type: {} | {}: {} | Channel: {} | {value_label}: {}",
        input.event_kind.label(),
        input.event_kind.label(),
        input.number,
        input.channel,
        input.value
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
            let Some(input) = ObservedMidiInput::from_event(event) else {
                ui.monospace(format!("Type: {}", event.message.kind.name()));
                return;
            };
            let (message_type, number_label, value_label) =
                if input.event_kind == MappingEventKind::Note {
                    ("Note On", "Note", "Velocity")
                } else {
                    ("Control Change", "Controller", "Value")
                };
            ui.monospace(format!("Type: {message_type}"));
            ui.monospace(format!("{number_label}: {}", input.number));
            ui.monospace(format!("Channel: {}", input.channel));
            ui.monospace(format!("{value_label}: {}", input.value));
            let interpretation = config
                .resolve(input.identity())
                .map(DrumComponent::label)
                .unwrap_or("UNMAPPED");
            ui.label(format!("Interpretation: {interpretation}"));
        });
}
