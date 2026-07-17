//! The New Project dialog — sets the project's resolution and frame rate before
//! anything is created (Krita's "New Document" flow, minus its UI layout).
//!
//! Presets here are a minimal starter set of standard resolutions; the full
//! Krita-style saved/custom preset library is a later feature.

use eframe::egui;

use crate::canvas::{DEFAULT_PAPER_H, DEFAULT_PAPER_W};

/// (label, width, height). Width 0 = "Custom" (leave the fields as the user set).
const PRESETS: &[(&str, u32, u32)] = &[
    ("Custom", 0, 0),
    ("HD 720p", 1280, 720),
    ("Full HD 1080p", 1920, 1080),
    ("4K UHD", 3840, 2160),
    ("Square 1080", 1080, 1080),
    ("Vertical 1080×1920", 1080, 1920),
];

pub enum FormAction {
    Create,
    Open,
    Cancel,
}

pub struct NewProjectForm {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub dpi: u32,
    pub preset: usize,
}

impl Default for NewProjectForm {
    fn default() -> Self {
        Self {
            name: "Untitled".to_string(),
            width: DEFAULT_PAPER_W,
            height: DEFAULT_PAPER_H,
            fps: 24,
            dpi: 300,
            preset: 2, // Full HD 1080p
        }
    }
}

/// Render the form body. Returns an action when a button is pressed.
/// `allow_cancel` is true only when a project is already open behind the dialog.
pub fn form_ui(
    ui: &mut egui::Ui,
    form: &mut NewProjectForm,
    allow_cancel: bool,
) -> Option<FormAction> {
    let mut action = None;

    ui.heading("New Project");
    ui.label(
        egui::RichText::new("Set the frame resolution — this is the project's paper size.")
            .weak(),
    );
    ui.add_space(10.0);

    egui::Grid::new("new_project_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut form.name);
            ui.end_row();

            ui.label("Preset");
            egui::ComboBox::from_id_salt("np_preset")
                .selected_text(PRESETS[form.preset].0)
                .show_ui(ui, |ui| {
                    for (i, (label, w, h)) in PRESETS.iter().enumerate() {
                        if ui.selectable_value(&mut form.preset, i, *label).clicked() && *w > 0 {
                            form.width = *w;
                            form.height = *h;
                        }
                    }
                });
            ui.end_row();

            ui.label("Dimensions");
            ui.horizontal(|ui| {
                if ui
                    .add(egui::DragValue::new(&mut form.width).range(1..=16384).suffix(" px"))
                    .changed()
                {
                    form.preset = 0;
                }
                ui.label("×");
                if ui
                    .add(egui::DragValue::new(&mut form.height).range(1..=16384).suffix(" px"))
                    .changed()
                {
                    form.preset = 0;
                }
                if ui.button("⇄").on_hover_text("swap width/height").clicked() {
                    std::mem::swap(&mut form.width, &mut form.height);
                    form.preset = 0;
                }
            });
            ui.end_row();

            ui.label("Frame rate");
            ui.add(egui::DragValue::new(&mut form.fps).range(1..=120).suffix(" fps"));
            ui.end_row();

            ui.label("Resolution");
            ui.add(egui::DragValue::new(&mut form.dpi).range(1..=1200).suffix(" dpi"));
            ui.end_row();
        });

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("Create Project").strong(),
            ))
            .clicked()
        {
            action = Some(FormAction::Create);
        }
        if ui.button("Open Project…").clicked() {
            action = Some(FormAction::Open);
        }
        if allow_cancel && ui.button("Cancel").clicked() {
            action = Some(FormAction::Cancel);
        }
    });

    action
}
