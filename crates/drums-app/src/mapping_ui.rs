//! Manual drum mapping view (Phase 2).
//! Allows the user to inspect and edit the drum configuration saved for the
//! currently connected MIDI device.

use drums_core::mapping::{DrumComponent, DrumKitConfiguration, MappingStore, SUPPORTED_COMPONENTS};
use eframe::egui;
use egui::RichText;

use crate::{DrumApp, View};

impl DrumApp {
    /// Ensures a draft mapping exists, initialized from the active config.
    fn ensure_mapping_draft(&mut self) {
        if self.mapping_draft.is_none() {
            if let Some(ref cfg) = self.active_config {
                self.mapping_draft = Some(cfg.clone());
            }
        }
    }
}