//! Guided, visual setup flow over the core `SetupWizard` state machine.

use drums_core::mapping::{DrumComponent, SUPPORTED_COMPONENTS, SetupWizard, SetupWizardState};
use eframe::egui;

use crate::{DrumApp, View};

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    if !app.connected {
        ui.heading("SETUP WIZARD");
        ui.label("Connect a MIDI input device to begin.");
        return;
    }
    let device = app
        .backend
        .connected_port_name()
        .unwrap_or("MIDI device")
        .to_string();
    if app.wizard.is_none() {
        ui.heading("SETUP WIZARD");
        ui.label(format!("Connected device: {device}"));
        ui.label("The wizard will ask you to hit each playable drum or hi-hat state.");
        ui.add_space(16.0);
        if ui
            .add_sized([180.0, 40.0], egui::Button::new("Start Setup"))
            .clicked()
        {
            let mut wizard = SetupWizard::new();
            wizard.start_after(app.clock.now_micros());
            app.wizard = Some(wizard);
            app.wizard_device = Some(device);
            app.wizard_last_input = None;
        }
        return;
    }

    enum Action {
        None,
        Accept,
        Retry,
        Skip,
        Back,
        Cancel,
        Finish,
    }
    let mut action = Action::None;
    let latest = app.wizard_last_input.clone();
    let interpreted = app.wizard.as_ref().and_then(|wizard| {
        wizard
            .rejected_hit()
            .map(|(_, component)| component)
            .or_else(|| wizard.detected_hit().and(wizard.current_component()))
    });
    {
        let wizard = app.wizard.as_ref().expect("checked above");
        ui.heading("SETUP WIZARD");
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!(
                "Step {} of {}",
                current_step(wizard),
                SUPPORTED_COMPONENTS.len()
            ));
            ui.separator();
            ui.label(format!("Connected device: {device}"));
        });
        ui.add_space(10.0);
        ui.columns(2, |columns| {
            columns[0].set_min_width(210.0);
            progress_list(&mut columns[0], wizard);
            columns[1].set_min_width(420.0);
            match wizard.state() {
                SetupWizardState::Detecting(component) => {
                    component_prompt(&mut columns[1], component, false);
                    columns[1].label(format!("Hit the {} on your kit.", component.label()));
                    if component == DrumComponent::HiHatPedalPosition {
                        columns[1].weak(
                            "This optional MIDI control is supported by some electronic drum kits. If your kit does not provide pedal-position data, choose Skip.",
                        );
                    } else if matches!(component, DrumComponent::HiHatOpen | DrumComponent::HiHatPedal) {
                        columns[1].weak(
                            "If your kit does not send a distinct MIDI note for this state, choose Skip.",
                        );
                    }
                    columns[1].add_space(8.0);
                    if let Some((hit, detected)) = wizard.rejected_hit() {
                        columns[1].colored_label(
                            egui::Color32::from_rgb(245, 105, 100),
                            format!(
                                "Wrong input - expected {}, detected {}",
                                component.label(),
                                detected.label()
                            ),
                        );
                        columns[1].monospace(format!(
                            "Note {} | Channel {} | Velocity {}",
                            hit.midi_note, hit.channel, hit.velocity
                        ));
                    }
                    columns[1].spinner();
                    columns[1].strong("Waiting for new MIDI input...");
                }
                SetupWizardState::Confirming(component) => {
                    component_prompt(&mut columns[1], component, true);
                    columns[1].colored_label(
                        egui::Color32::from_rgb(90, 220, 140),
                        "New MIDI input captured - confirm this mapping",
                    );
                    if let Some(hit) = wizard.detected_hit() {
                        columns[1].monospace(format!(
                            "{} {} | Channel {} | {} {}",
                            hit.event_kind.label(),
                            hit.midi_note,
                            hit.channel,
                            if hit.event_kind == drums_core::mapping::MappingEventKind::Note { "Velocity" } else { "Value" },
                            hit.velocity
                        ));
                    }
                }
                SetupWizardState::Completed => {
                    columns[1].heading("SETUP COMPLETE");
                    columns[1].label(format!("{} / {} components configured", wizard.done_count(), SUPPORTED_COMPONENTS.len()));
                    columns[1].label(format!("Connected device: {device}"));
                }
                SetupWizardState::Intro | SetupWizardState::Canceled => {}
            }
        });
        wizard_target_circles(ui, wizard.current_component());
        ui.add_space(12.0);
        last_input_panel(ui, latest.as_ref(), interpreted);
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| match wizard.state() {
            SetupWizardState::Detecting(_) => {
                if wizard.can_go_back() && big_button(ui, "Back") {
                    action = Action::Back;
                }
                if big_button(ui, "Skip") {
                    action = Action::Skip;
                }
                if big_button(ui, "Cancel") {
                    action = Action::Cancel;
                }
            }
            SetupWizardState::Confirming(_) => {
                if big_button_enabled(ui, "Accept / Continue", true) {
                    action = Action::Accept;
                }
                if big_button(ui, "Reassign / Retry") {
                    action = Action::Retry;
                }
                if wizard.can_go_back() && big_button(ui, "Back") {
                    action = Action::Back;
                }
                if big_button(ui, "Skip") {
                    action = Action::Skip;
                }
                if big_button(ui, "Cancel") {
                    action = Action::Cancel;
                }
            }
            SetupWizardState::Completed => {
                if big_button(ui, "Done - Save Mapping") {
                    action = Action::Finish;
                }
                if big_button(ui, "Cancel") {
                    action = Action::Cancel;
                }
            }
            _ => {}
        });
    }
    match action {
        Action::Accept => {
            app.wizard.as_mut().unwrap().accept();
            let boundary = app.clock.now_micros();
            app.wizard.as_mut().unwrap().begin_step_after(boundary);
            app.wizard_last_input = None;
        }
        Action::Retry => {
            app.wizard.as_mut().unwrap().retry();
            let boundary = app.clock.now_micros();
            app.wizard.as_mut().unwrap().begin_step_after(boundary);
            app.wizard_last_input = None;
        }
        Action::Skip => {
            app.wizard.as_mut().unwrap().skip();
            let boundary = app.clock.now_micros();
            app.wizard.as_mut().unwrap().begin_step_after(boundary);
            app.wizard_last_input = None;
        }
        Action::Back => {
            app.wizard.as_mut().unwrap().back();
            let boundary = app.clock.now_micros();
            app.wizard.as_mut().unwrap().begin_step_after(boundary);
            app.wizard_last_input = None;
        }
        Action::Cancel => {
            app.wizard = None;
            app.wizard_device = None;
            app.wizard_last_input = None;
            app.view = View::Monitor;
        }
        Action::Finish => finish_wizard(app, &device),
        Action::None => {}
    }
}

fn current_step(wizard: &SetupWizard) -> usize {
    wizard
        .current_component()
        .and_then(|current| {
            SUPPORTED_COMPONENTS
                .iter()
                .position(|value| *value == current)
        })
        .map(|index| index + 1)
        .unwrap_or(SUPPORTED_COMPONENTS.len())
}

fn progress_list(ui: &mut egui::Ui, wizard: &SetupWizard) {
    egui::Frame::group(ui.style())
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.strong("PROGRESS");
            for component in SUPPORTED_COMPONENTS {
                let prefix = if wizard.is_done(component) {
                    "DONE"
                } else if wizard.current_component() == Some(component) {
                    "CURRENT"
                } else {
                    "PENDING"
                };
                let color = if wizard.current_component() == Some(component) {
                    egui::Color32::from_rgb(120, 205, 255)
                } else {
                    ui.visuals().text_color()
                };
                ui.colored_label(color, format!("{prefix}  {}", component.label()));
            }
        });
}

fn component_prompt(ui: &mut egui::Ui, component: DrumComponent, _active: bool) {
    ui.weak("CONFIGURE");
    ui.heading(egui::RichText::new(component.label()).size(30.0));
}

fn last_input_panel(
    ui: &mut egui::Ui,
    event: Option<&drums_core::MidiEvent>,
    interpreted: Option<DrumComponent>,
) {
    egui::Frame::group(ui.style())
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.strong("LAST DETECTED INPUT");
            let Some(event) = event else {
                ui.weak("Waiting for MIDI input...");
                return;
            };
            ui.label(
                egui::RichText::new(interpreted.map(DrumComponent::label).unwrap_or("UNKNOWN"))
                    .strong()
                    .size(21.0),
            );
            let kind = if event.message.kind == drums_core::MessageKind::ControlChange {
                "CC"
            } else {
                "Note"
            };
            let number = event
                .message
                .data1
                .map_or_else(|| "N/A".into(), |value| value.to_string());
            let value = event
                .message
                .data2
                .map_or_else(|| "N/A".into(), |value| value.to_string());
            let channel = event
                .message
                .channel_number()
                .map_or_else(|| "N/A".into(), |value| value.to_string());
            ui.monospace(format!(
                "Type: {kind} | {kind}: {number} | Channel: {channel} | Value: {value}"
            ));
            if event.message.kind == drums_core::MessageKind::ControlChange {
                ui.add(
                    egui::ProgressBar::new(event.message.data2.unwrap_or(0) as f32 / 127.0).text(
                        format!("Pedal position: {} / 127", event.message.data2.unwrap_or(0)),
                    ),
                );
            }
        });
}

fn wizard_target_circles(ui: &mut egui::Ui, target: Option<DrumComponent>) {
    ui.add_space(10.0);
    ui.strong("DRUM KIT");
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
                ui.push_id(("wizard_target", component), |ui| {
                    let (response, painter) =
                        ui.allocate_painter(egui::vec2(96.0, 72.0), egui::Sense::hover());
                    let active = target.is_some_and(|value| {
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
                    let fill = if active {
                        egui::Color32::from_rgb(80, 205, 245)
                    } else {
                        egui::Color32::from_rgb(65, 78, 96)
                    };
                    painter.circle_filled(center, 22.0, fill);
                    painter.circle_stroke(
                        center,
                        22.0,
                        egui::Stroke::new(
                            if active { 4.0_f32 } else { 2.0_f32 },
                            if active {
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
                        if active {
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

fn big_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add_sized([150.0, 40.0], egui::Button::new(label))
        .clicked()
}

fn big_button_enabled(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(label).min_size(egui::vec2(150.0, 40.0)),
    )
    .clicked()
}

fn finish_wizard(app: &mut DrumApp, device: &str) {
    if let Some(config) = app.wizard.as_mut().and_then(|wizard| wizard.finish(device)) {
        match app.store.save(&config) {
            Ok(()) => {
                app.status = format!(
                    "Mapping saved for {device}: {} components configured.",
                    config.mapped_count()
                );
                app.active_config = Some(config);
            }
            Err(error) => app.status = format!("Failed to save mapping: {error}"),
        }
    }
    app.wizard = None;
    app.wizard_device = None;
    app.wizard_last_input = None;
    app.view = View::Monitor;
}
