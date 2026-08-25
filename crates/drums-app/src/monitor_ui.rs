//! Live MIDI monitor presentation.

use drums_core::{MessageKind, MidiEvent};
use eframe::egui;

use crate::DrumApp;

pub(crate) fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.heading("MIDI Monitor");
    ui.horizontal_wrapped(|ui| {
        if ui.button("Refresh devices").clicked() {
            app.refresh_devices();
        }
        let selected_text = app
            .ports
            .get(app.selected.unwrap_or(usize::MAX))
            .map(|port| port.name.clone())
            .unwrap_or_else(|| "Select a MIDI input...".to_string());
        egui::ComboBox::from_id_salt("monitor_device_combo")
            .width(320.0)
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                if app.ports.is_empty() {
                    ui.add_enabled(false, egui::Label::new("No MIDI devices found"));
                }
                for port in &app.ports {
                    ui.selectable_value(&mut app.selected, Some(port.index), &port.name);
                }
            });
        if app.connected {
            if ui.button("Disconnect").clicked() {
                app.disconnect_clicked();
            }
        } else if ui
            .add_enabled(
                app.selected.is_some() && !app.ports.is_empty(),
                egui::Button::new("Connect"),
            )
            .clicked()
        {
            app.connect_clicked();
        }
    });
    if !app.status.is_empty() {
        ui.weak(&app.status);
    }
    ui.separator();
    ui.heading("Latest event");
    ui.separator();

    if let Some(latest) = app.events.first() {
        egui::Grid::new("latest_grid")
            .num_columns(2)
            .spacing([24.0, 4.0])
            .show(ui, |ui| {
                ui.label("Message type");
                ui.monospace(latest.message.kind.name());
                ui.end_row();
                ui.label("MIDI note number");
                ui.monospace(fmt_opt(latest.message.note()));
                ui.end_row();
                ui.label("Velocity");
                ui.monospace(fmt_opt(latest.message.velocity()));
                ui.end_row();
                ui.label("CC controller");
                ui.monospace(if latest.message.kind == MessageKind::ControlChange {
                    fmt_opt(latest.message.data1)
                } else {
                    "N/A".into()
                });
                ui.end_row();
                ui.label("CC value");
                ui.monospace(if latest.message.kind == MessageKind::ControlChange {
                    fmt_opt(latest.message.data2)
                } else {
                    "N/A".into()
                });
                ui.end_row();
                ui.label("MIDI channel");
                ui.monospace(fmt_opt(latest.message.channel_number()));
                ui.end_row();
                ui.label("Timestamp");
                ui.monospace(timestamp_text(latest.timestamp_micros));
                ui.end_row();
            });
    } else {
        ui.weak("No MIDI events yet. Connect a device and hit a pad.");
    }

    ui.add_space(12.0);
    ui.heading(format!("Event log ({})", app.events.len()));
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for event in &app.events {
                ui.monospace(fmt_event(event));
            }
        });
}

fn fmt_opt(value: Option<u8>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "N/A".to_string(),
    }
}

fn timestamp_text(microseconds: u64) -> String {
    format!(
        "{} us (~{:.3} s)",
        microseconds,
        microseconds as f64 / 1_000_000.0
    )
}

fn fmt_event(event: &MidiEvent) -> String {
    format!(
        "{:>12} us  {:<17} ch={:<4} data1={:<5} data2={:<4}",
        event.timestamp_micros,
        event.message.kind.name(),
        fmt_opt(event.message.channel_number()),
        fmt_opt(event.message.data1),
        fmt_opt(event.message.data2),
    )
}
