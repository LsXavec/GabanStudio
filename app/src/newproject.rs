//! The New Project dialog — sets the project's resolution and frame rate before
//! anything is created (Krita's "New Document" flow, minus its UI layout).
//!
//! Presets here are a minimal starter set of standard resolutions; the full
//! Krita-style saved/custom preset library is a later feature.

use anim_core::color::ColorLaw;
use eframe::egui;

use crate::canvas::{DEFAULT_PAPER_H, DEFAULT_PAPER_W};
use crate::doc::AppState;
use crate::plate;
use crate::templates;

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
    /// AUDIT [16]: why the last Open failed. Without this, picking an
    /// unreadable project on the startup screen silently redisplayed the
    /// same form and the artist had no idea what happened.
    pub error: Option<String>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub dpi: u32,
    pub preset: usize,
    /// GM8: the Welcome template this cut starts from (`templates::ALL`):
    /// its columns, cut length and first room, at this form's size and
    /// rate. None: a blank cut with one column.
    pub template: Option<usize>,
}

impl Default for NewProjectForm {
    fn default() -> Self {
        Self {
            error: None,
            // AUDIT [31]: empty, so the "name this cut" hint is reachable.
            name: String::new(),
            width: DEFAULT_PAPER_W,
            height: DEFAULT_PAPER_H,
            fps: 24,
            dpi: 300,
            preset: 2, // Full HD 1080p
            template: None,
        }
    }
}

impl NewProjectForm {
    /// The project this form creates. GM7 (D8): a NEW project stores exact
    /// sRGB chips; `AppState::new_project` and the engine stay Legacy (old
    /// files, and the tests that pin v0.3 pixels).
    pub fn new_state(&self) -> AppState {
        // AUDIT [31] follow-on: the name field now starts EMPTY so its
        // hint is reachable, so an untyped name must still produce a
        // named cut rather than a blank one.
        let name = match self.name.trim() {
            "" => "Untitled".to_string(),
            n => n.to_string(),
        };
        let mut state = match self.template() {
            // GM8: the template's sheet, cut length and graph; the paper
            // and rate are this form's (the same clamps as new_project).
            Some(t) => {
                let mut s = t.build();
                let p = &mut s.engine.project;
                p.name = name;
                p.width = self.width.max(1);
                p.height = self.height.max(1);
                p.fps = self.fps.clamp(1, 240);
                p.dpi = (self.dpi as f32).max(1.0);
                s
            }
            None => AppState::new_project(name, self.width, self.height, self.fps, self.dpi as f32),
        };
        state.engine.project.color_law = ColorLaw::Srgb;
        state
    }

    /// GM8: the template chosen, if any.
    pub fn template(&self) -> Option<&'static templates::Template> {
        self.template.and_then(|i| templates::ALL.get(i))
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

    // THE SLATE, at rest: the first screen an artist meets should look
    // like the instrument they are about to use — engraved legends,
    // Struck values, one clear verb. (Repaint sweep 2026-08-18.)
    plate::legend(ui, "new cut");
    ui.label(
        egui::RichText::new(
            "The paper you will draw on, and how fast it plays. \
             Both are fixed once the cut is created.",
        )
        .size(11.5)
        .color(plate::LEGEND),
    );
    ui.add_space(12.0);
    egui::Grid::new("new_project_grid")
        .num_columns(2)
        .spacing([14.0, 10.0])
        .show(ui, |ui| {
            plate::legend(ui, "name");
            ui.add(
                egui::TextEdit::singleline(&mut form.name)
                    .desired_width(220.0)
                    .hint_text("the cut's name"),
            );
            ui.end_row();
            // GM8: the Welcome's templates — picking one sets the paper and
            // rate; its columns, cut length and room come with it.
            plate::legend(ui, "start from");
            egui::ComboBox::from_id_salt("np_template")
                .selected_text(form.template().map_or("blank (one column)", |t| t.name))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut form.template, None, "blank (one column)");
                    for (i, t) in templates::ALL.iter().enumerate() {
                        if ui
                            .selectable_value(&mut form.template, Some(i), t.name)
                            .on_hover_text(format!("{} · {}", t.summary(), t.tooltip()))
                            .clicked()
                        {
                            form.width = t.width;
                            form.height = t.height;
                            form.fps = t.fps;
                            form.preset = PRESETS
                                .iter()
                                .position(|p| (p.1, p.2) == (t.width, t.height))
                                .unwrap_or(0);
                        }
                    }
                });
            ui.end_row();
            plate::legend(ui, "paper");
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
            plate::legend(ui, "size");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut form.width)
                            .range(1..=16384)
                            .suffix(" px"),
                    )
                    .changed()
                {
                    form.preset = 0;
                }
                ui.label(egui::RichText::new("×").color(plate::LEGEND));
                if ui
                    .add(
                        egui::DragValue::new(&mut form.height)
                            .range(1..=16384)
                            .suffix(" px"),
                    )
                    .changed()
                {
                    form.preset = 0;
                }
                // AUDIT [32]: this wore Icon::Fit — the fit-view mark.
                if ui
                    .button("swap")
                    .on_hover_text("swap width and height")
                    .clicked()
                {
                    std::mem::swap(&mut form.width, &mut form.height);
                    form.preset = 0;
                }
            });
            ui.end_row();
            plate::legend(ui, "frame rate");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut form.fps)
                        .range(1..=120)
                        .suffix(" fps"),
                );
                // The number an animator actually thinks in: what one
                // second costs at this rate, on ones and on twos.
                // AUDIT [43]: integer division read "12 on twos" at 25fps.
                let twos = form.fps as f32 / 2.0;
                let twos_txt = if twos.fract().abs() < 0.001 {
                    format!("{twos:.0}")
                } else {
                    format!("{twos:.1}")
                };
                ui.label(
                    egui::RichText::new(if form.fps == 1 {
                        "one second = 1 frame".to_string()
                    } else if form.fps < 12 {
                        format!("one second = {} frames", form.fps)
                    } else {
                        format!(
                            "one second = {} frames · {twos_txt} drawings on twos",
                            form.fps
                        )
                    })
                    .size(10.5)
                    .color(plate::LEGEND),
                );
            });
            ui.end_row();
            plate::legend(ui, "print");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut form.dpi)
                        .range(1..=1200)
                        .suffix(" dpi"),
                );
                ui.label(
                    // AUDIT [30]: dpi is stored in the project and read by
                    // nothing yet — say that rather than implying it acts.
                    egui::RichText::new("recorded with the project; nothing uses it yet")
                        .size(10.5)
                        .color(plate::LEGEND),
                );
            });
            ui.end_row();
        });
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Start drawing")
                        .size(13.0)
                        .color(plate::STRUCK),
                )
                .min_size(egui::vec2(140.0, 30.0)),
            )
            .on_hover_text("create the cut and open the drawing room")
            .clicked()
        {
            action = Some(FormAction::Create);
        }
        if ui.button("Open a project…").clicked() {
            action = Some(FormAction::Open);
        }
        if allow_cancel && ui.button("Cancel").clicked() {
            action = Some(FormAction::Cancel);
        }
    });
    // AUDIT [16]: a file that would not open contradicts what was asked
    // for — Aka, and it names the reason.
    if let Some(err) = &form.error {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(err).color(plate::AKA));
    }
    action
}
