//! The New Project dialog — sets the project's resolution and frame rate before
//! anything is created (Krita's "New Document" flow, minus its UI layout).
//!
//! GM21: the paper sizes are `templates::SIZES` (every template's, then two
//! extras); the full Krita-style saved/custom preset library is a later
//! feature.

use anim_core::color::ColorLaw;
use eframe::egui;

use crate::canvas::{DEFAULT_PAPER_H, DEFAULT_PAPER_W};
use crate::doc::AppState;
use crate::plate;
use crate::templates;

/// The template row that starts from nothing.
const BLANK: &str = "blank (one track)";

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
    /// GM21: the paper picked from `templates::SIZES`; None: custom.
    pub preset: Option<usize>,
    /// GM8: the Welcome template this cut starts from (`templates::ALL`):
    /// its columns, cut length and first room, at this form's size and
    /// rate (GM21: a page template's paper and dpi are its page's). None: a
    /// blank cut with one column.
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
            preset: templates::SIZES
                .iter()
                .position(|s| (s.1, s.2) == (DEFAULT_PAPER_W, DEFAULT_PAPER_H)),
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
            // GM21: a page template keeps its page's paper and dpi — the
            // page geometry in the cut meta is drawn for them.
            Some(t) => {
                let mut s = t.build();
                let p = &mut s.engine.project;
                p.name = name;
                p.fps = self.fps.clamp(1, 240);
                if t.page.is_none() {
                    p.width = self.width.max(1);
                    p.height = self.height.max(1);
                    p.dpi = (self.dpi as f32).max(1.0);
                }
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

    /// GM21: start from template `i` (None: blank) — a template sets the
    /// paper, rate and dpi from itself (a page template's are its page's).
    /// Leaving a template for blank goes back to the form's defaults, so a
    /// page's paper and dpi do not stay behind on a cut with no page.
    pub fn pick_template(&mut self, i: Option<usize>) {
        let left = self.template.is_some() && i.is_none();
        self.template = i;
        if let Some(t) = self.template() {
            self.width = t.width;
            self.height = t.height;
            self.fps = t.fps;
            self.dpi = t.dpi().round() as u32;
            // SIZES lists the templates first, in order.
            self.preset = i;
        } else if left {
            let d = Self::default();
            (self.width, self.height, self.fps) = (d.width, d.height, d.fps);
            (self.dpi, self.preset) = (d.dpi, d.preset);
        }
    }

    /// GM21: pick paper `i` of `templates::SIZES` — a page template's paper
    /// comes with its print dpi (D15), so the page is at print size.
    pub fn pick_paper(&mut self, i: usize) {
        let (_, w, h) = templates::SIZES[i];
        (self.width, self.height) = (w, h);
        if let Some(t) = templates::ALL.get(i).filter(|t| t.page.is_some()) {
            self.dpi = t.dpi().round() as u32;
        }
    }

    /// GM21: a page template fixes the paper and dpi.
    pub fn page_template(&self) -> bool {
        self.template().is_some_and(|t| t.page.is_some())
    }
}

/// GM21: paper `i`'s row in the combo — its name and its size (Manga B5 —
/// 2591×3624), the size said once when the name already says it.
fn paper_label(i: usize) -> String {
    let (name, w, h) = templates::SIZES[i];
    if name.contains('×') {
        name.to_string()
    } else {
        format!("{name} — {w}×{h}")
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
    plate::legend(ui, "new shot");
    ui.label(
        egui::RichText::new("The paper you will draw on, and how fast it plays.")
            .font(plate::Role::Small.font())
            .color(plate::INK2),
    );
    ui.add_space(12.0);
    egui::Grid::new("new_project_grid")
        .num_columns(2)
        .spacing([14.0, 10.0])
        .show(ui, |ui| {
            plate::legend(ui, "name");
            plate::text_field(
                ui,
                egui::TextEdit::singleline(&mut form.name)
                    .desired_width(220.0)
                    .hint_text("the shot's name"),
            );
            ui.end_row();
            // GM8: the Welcome's templates — picking one sets the paper and
            // rate; its columns, cut length and room come with it (GM21:
            // and its dpi).
            plate::legend(ui, "start from");
            egui::ComboBox::from_id_salt("np_template")
                .selected_text(form.template().map_or(BLANK, |t| t.name))
                .show_ui(ui, |ui| {
                    // GM21: through pick_template, so a template's paper
                    // does not stay behind on a blank cut.
                    if ui
                        .selectable_label(form.template.is_none(), BLANK)
                        .clicked()
                    {
                        form.pick_template(None);
                    }
                    for (i, t) in templates::ALL.iter().enumerate() {
                        if ui
                            .selectable_value(&mut form.template, Some(i), t.name)
                            .on_hover_text(format!("{} · {}", t.summary(), t.tooltip()))
                            .clicked()
                        {
                            form.pick_template(Some(i));
                        }
                    }
                });
            ui.end_row();
            // GM21: a page template's paper and dpi are its page's.
            let paged = form.page_template();
            plate::legend(ui, "paper");
            ui.add_enabled_ui(!paged, |ui| {
                egui::ComboBox::from_id_salt("np_preset")
                    .selected_text(form.preset.map_or_else(|| "Custom".to_string(), paper_label))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.preset, None, "Custom");
                        for (i, (_, w, h)) in templates::SIZES.iter().enumerate() {
                            // One row per size: a later template with the
                            // same paper would be the same pick again.
                            if templates::SIZES[..i].iter().any(|s| (s.1, s.2) == (*w, *h)) {
                                continue;
                            }
                            if ui
                                .selectable_value(&mut form.preset, Some(i), paper_label(i))
                                .clicked()
                            {
                                form.pick_paper(i);
                            }
                        }
                    });
            });
            ui.end_row();
            plate::legend(ui, "size");
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!paged, |ui| {
                    if plate::field(
                        ui,
                        egui::DragValue::new(&mut form.width)
                            .range(1..=16384)
                            .suffix(" px"),
                    )
                    .changed()
                    {
                        form.preset = None;
                    }
                    ui.label(egui::RichText::new("×").color(plate::INK2));
                    if plate::field(
                        ui,
                        egui::DragValue::new(&mut form.height)
                            .range(1..=16384)
                            .suffix(" px"),
                    )
                    .changed()
                    {
                        form.preset = None;
                    }
                    // AUDIT [32]: this wore Icon::Fit — the fit-view mark.
                    if ui
                        .button("swap")
                        .on_hover_text("swap width and height")
                        .clicked()
                    {
                        std::mem::swap(&mut form.width, &mut form.height);
                        form.preset = None;
                    }
                });
                if paged {
                    ui.label(
                        egui::RichText::new("set by the page template")
                            .font(plate::Role::Small.font())
                            .color(plate::INK2),
                    );
                }
            });
            ui.end_row();
            plate::legend(ui, "frame rate");
            ui.horizontal(|ui| {
                plate::field(
                    ui,
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
                    .font(plate::Role::Small.font())
                    .color(plate::INK2),
                );
            });
            ui.end_row();
            plate::legend(ui, "print");
            ui.horizontal(|ui| {
                plate::field_if(
                    ui,
                    !paged,
                    egui::DragValue::new(&mut form.dpi)
                        .range(1..=1200)
                        .suffix(" dpi"),
                );
                ui.label(
                    // AUDIT [30]: dpi is stored in the project and read by
                    // nothing yet — say that rather than implying it acts.
                    egui::RichText::new(if paged {
                        "set by the page template"
                    } else {
                        "recorded with the project; nothing uses it yet"
                    })
                    .font(plate::Role::Small.font())
                    .color(plate::INK2),
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
                        .font(plate::Role::Panel.font())
                        .color(plate::INK),
                )
                .min_size(egui::vec2(140.0, 30.0)),
            )
            .on_hover_text("create the shot and open the Draw step")
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
        ui.label(egui::RichText::new(err).color(plate::RED));
    }
    action
}
