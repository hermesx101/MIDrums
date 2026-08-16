//! Setup wizard view (Phase 2).
//!
//! A focused flow on top of the Phase 1 UI. The state machine itself lives in
//! `drums-core` (`SetupWizard` / `SetupWizardState`); this module only renders
//! it and forwards user decisions. MIDI events are fed to the wizard from
//! `DrumApp::drain_events` — nothing here touches timing.

use drums_core::mapping::{SetupWizard, SetupWizardState, SUPPORTED_COMPONENTS};
use eframe::egui;
use egui::RichText;

use crate::{DrumApp, View};

/// Renders the setup wizard tab into the supplied `Ui`.
///
/// `main.rs` calls this as `wizard_ui::show(self, ui)`; it is a thin
/// presentation layer over the existing `SetupWizard` state machine and
/// `MappingStore` persistence, owning no logic of its own.
pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
        ui.add_space(6.0);

        if !app.connected {
            intro_prompt(ui);
            return;
        }

        let device = app
            .backend
            .connected_port_name()
            .unwrap_or("unknown device")
            .to_string();

        // No wizard running: show the intro screen with a Start button.
        if app.wizard.is_none() {
            intro_screen(ui, &device);
            if ui.button("Start Setup").clicked() {
                let mut wizard = SetupWizard::new();
                wizard.start();
                app.wizard = Some(wizard);
                app.wizard_device = Some(device.clone());
            }
            ui.add_space(4.0);
            if ui.button("Back to Monitor").clicked() {
                app.view = View::Monitor;
            }
            return;
        }

        // Wizard active: collect the clicked action while `wizard` is
        // borrowed, then apply it afterwards so finishing/cancelling can
        // replace or drop the wizard itself without a borrow conflict.
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

                {
            let wizard = app.wizard.as_mut().expect("checked above");

            progress_header(ui, wizard, &device);
            ui.add_space(6.0);
            ui.separator();

            match wizard.state() {
                SetupWizardState::Detecting(component) => {
                    ui.add_space(8.0);
                    ui.heading(RichText::new(component.label()).size(28.0));
                    ui.label(format!("Hit your {} once.", component.label()));
                    ui.weak("Waiting for the next MIDI Note-On (velocity > 0)…");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if wizard.can_go_back() && ui.button("‹ Back").clicked() {
                            action = Action::Back;
                        }
                        if ui.button("Skip").clicked() {
                            action = Action::Skip;
                        }
                        if ui.button("Cancel").clicked() {
                            action = Action::Cancel;
                        }
                    });
                }
                SetupWizardState::Confirming(component) => {
                    ui.add_space(8.0);
                    ui.heading(RichText::new(component.label()).size(28.0));
                    if let Some(hit) = wizard.detected_hit() {
                        egui::Grid::new("wizard_hit_grid")
                            .num_columns(2)
                            .spacing([24.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("MIDI Note");
                                ui.monospace(hit.midi_note.to_string());
                                ui.end_row();
                                ui.label("Velocity");
                                ui.monospace(hit.velocity.to_string());
                                ui.end_row();
                                ui.label("MIDI channel");
                                ui.monospace(hit.channel.to_string());
                                ui.end_row();
                                ui.label("Timestamp");
                                ui.monospace(format!("{} µs", hit.timestamp_micros));
                                ui.end_row();
                            });
                    }

                    if !wizard.conflicting_hits().is_empty() {
                        let extra: Vec<String> = wizard
                            .conflicting_hits()
                            .iter()
                            .map(|h| h.midi_note.to_string())
                            .collect();
                        ui.colored_label(
                            egui::Color32::from_rgb(240, 200, 80),
                            format!(
                                "⚠ Multiple notes arrived together ({}) — only the first is \
                                 staged. Press Retry if this is wrong.",
                                extra.join(", ")
                            ),
                        );
                    }

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("✔ Accept").size(16.0)))
                            .clicked()
                        {
                            action = Action::Accept;
                        }
                        if ui
                            .add(egui::Button::new(RichText::new("↺ Retry").size(16.0)))
                            .clicked()
                        {
                            action = Action::Retry;
                        }
                    });
                    ui.horizontal(|ui| {
                        if wizard.can_go_back() && ui.button("‹ Back").clicked() {
                            action = Action::Back;
                        }
                        if ui.button("Skip").clicked() {
                            action = Action::Skip;
                        }
                        if ui.button("Cancel").clicked() {
                            action = Action::Cancel;
                        }
                    });
                }
                SetupWizardState::Completed => {
                    ui.add_space(8.0);
                    ui.heading(RichText::new("SETUP COMPLETE ✓").size(22.0));
                    ui.label(format!(
                        "{}/{} components configured for “{}”.",
                        wizard.done_count(),
                        SUPPORTED_COMPONENTS.len(),
                        device
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(
                                RichText::new("💾 Finish & Save mapping").size(16.0),
                            ))
                            .clicked()
                        {
                            action = Action::Finish;
                        }
                        if ui.button("Cancel").clicked() {
                            action = Action::Cancel;
                        }
                    });
                }
                SetupWizardState::Intro | SetupWizardState::Canceled => {
                    // Transient states inside an active wizard are impossible in
                    // normal flow; show a hint rather than nothing.
                    ui.weak("Setup is not running. Press “Start Setup” to begin.");
                }
            }
        }

        match action {
                        Action::Accept => {
                if let Some(wizard) = app.wizard.as_mut() {
                    wizard.accept();
                }
            }
            Action::Retry => {
                if let Some(wizard) = app.wizard.as_mut() {
                    wizard.retry();
                }
            }
            Action::Skip => {
                if let Some(wizard) = app.wizard.as_mut() {
                    wizard.skip();
                }
            }
            Action::Back => {
                if let Some(wizard) = app.wizard.as_mut() {
                    wizard.back();
                }
            }
            Action::Cancel => {
                app.wizard = None;
                app.wizard_device = None;
                app.view = View::Monitor;
            }
                        Action::Finish => finish_wizard(app, &device),
            Action::None => {}
        }
    }

/// Shown when no MIDI device is connected.
fn intro_prompt(ui: &mut egui::Ui) {
    ui.heading("SET UP YOUR DRUM KIT");
    ui.separator();
    ui.label("Connect a MIDI input device first.");
    ui.weak("The wizard learns the mapping by listening for real hits on the connected kit.");
}

/// The intro screen shown once a device is connected but no wizard is running.
fn intro_screen(ui: &mut egui::Ui, _device: &str) {
    ui.heading("SET UP YOUR DRUM KIT");
    ui.separator();
    ui.label(
        "We will identify each component of your electronic drum kit.\n\
         Hit each component when asked.",
    );
    ui.add_space(14.0);
}

/// Shared progress header: how many components are done so far.
fn progress_header(ui: &mut egui::Ui, wizard: &SetupWizard, device: &str) {
    ui.colored_label(
        egui::Color32::from_rgb(140, 160, 220),
        format!(
            "Configuring: \u{201c}{}\u{201d}  -  {}/{} components done",
            device,
            wizard.done_count(),
            SUPPORTED_COMPONENTS.len(),
        ),
    );
        ui.add_space(4.0);
    ui.separator();
}

/// Finalises the wizard: produces the configuration, saves it to the
/// `MappingStore`, and returns to the Monitor view.
fn finish_wizard(app: &mut DrumApp, device: &str) {
    let config = app.wizard.as_mut().and_then(|w| w.finish(device));
    if let Some(cfg) = config {
        if let Err(e) = app.store.save(&cfg) {
            app.status = format!("Failed to save mapping: {e}");
        } else {
            app.status = format!(
                "Mapping saved for \u{201c}{}\u{201d}: {}/{} drums configured.",
                cfg.device_name,
                cfg.mapped_count(),
                SUPPORTED_COMPONENTS.len(),
            );
        }
        app.active_config = Some(cfg);
    }
    app.wizard = None;
    app.wizard_device = None;
    app.view = View::Monitor;
}