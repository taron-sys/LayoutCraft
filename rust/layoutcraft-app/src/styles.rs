//! The Styles window: give the selection one of the drawing's styles, and make or delete styles of the drawing's own. The
//! styles and the changes are the engine's; this file asks and draws.
use super::{hex, typed_colour, LayoutCraftApp};
use eframe::egui;

/// What of a style is applied, with the words the engine takes.
const SCOPES: [(&str, &str); 4] = [("all", "Line, fill and text"), ("line", "Line only"), ("fill", "Fill only"), ("text", "Text only")];

/// The window's choice: the style's ID and the part of it to apply, and the style being described.
pub(super) struct StylePick {
    style: Option<String>,
    scope: &'static str,
    pub(super) new: NewStyle,
}

/// A style as typed: what is left empty is not set, and comes from the style it rests on.
#[derive(Default)]
pub(super) struct NewStyle {
    pub(super) name: String,
    pub(super) line: String,
    pub(super) weight: String,
    pub(super) fill: String,
    pub(super) text: String,
    pub(super) size: String,
    pub(super) bold: bool,
    pub(super) italic: bool,
}

impl NewStyle {
    /// The engine's description of the style, or what to correct.
    fn spec(&self) -> Result<serde_json::Value, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Give the style a name.".to_string());
        }
        let mut spec = serde_json::json!({ "name": name });
        for (key, typed, what) in [("lineColor", &self.line, "line colour"), ("fillColor", &self.fill, "fill colour"), ("textColor", &self.text, "text colour")] {
            if !typed.trim().is_empty() {
                let colour = typed_colour(typed).ok_or_else(|| format!("Type the {what} as #rrggbb."))?;
                spec[key] = hex(colour).into();
            }
        }
        for (key, typed, what) in [("lineWeight", &self.weight, "line weight"), ("fontSize", &self.size, "font size")] {
            if !typed.trim().is_empty() {
                let points = typed.trim().parse::<f64>().ok().filter(|points| points.is_finite()).ok_or_else(|| format!("Type the {what} as a number of points."))?;
                spec[key] = points.into();
            }
        }
        if self.bold || self.italic {
            spec["bold"] = self.bold.into();
            spec["italic"] = self.italic.into();
        }
        Ok(spec)
    }
}

impl Default for StylePick {
    fn default() -> Self {
        Self { style: None, scope: SCOPES[0].0, new: NewStyle::default() }
    }
}

impl LayoutCraftApp {
    /// The drawing's styles as (ID, name, made for this drawing: only those can be deleted).
    fn style_list(&self) -> Vec<(String, String, bool)> {
        let list = self.engine.styles_list_json().ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|value| value.as_array().cloned()).unwrap_or_default();
        list.iter().filter_map(|style| Some((style["id"].as_str()?.to_string(), style["name"].as_str()?.to_string(), style["custom"].as_bool() == Some(true)))).collect()
    }

    pub(super) fn draw_styles_window(&mut self, ctx: &egui::Context) {
        if self.style_pick.is_none() {
            return;
        }
        let styles = self.style_list();
        let selected = self.all_selected();
        let Some(pick) = self.style_pick.as_mut() else { return };
        // a style that is gone (undo, another drawing) is no longer chosen
        if pick.style.as_ref().is_some_and(|chosen| styles.iter().all(|style| &style.0 != chosen)) {
            pick.style = None;
        }
        let (mut open, mut apply, mut delete, mut save) = (true, false, false, false);
        egui::Window::new("Styles").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            if styles.is_empty() {
                ui.label("This drawing has no styles.");
            }
            let label = ui.label("Style");
            let shown = styles.iter().find(|style| Some(&style.0) == pick.style.as_ref()).map_or("Choose a style", |style| style.1.as_str());
            egui::ComboBox::from_id_salt("style-pick")
                .selected_text(shown)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for (id, name, _) in &styles {
                        ui.selectable_value(&mut pick.style, Some(id.clone()), name);
                    }
                })
                .response
                .labelled_by(label.id);
            for (scope, name) in SCOPES {
                ui.radio_value(&mut pick.scope, scope, name);
            }
            apply = ui.add_enabled(pick.style.is_some() && !selected.is_empty(), egui::Button::new("Apply style to selection")).clicked();
            if selected.is_empty() {
                ui.label("Select shapes to give them a style.");
            }
            ui.separator();
            let own = styles.iter().any(|style| style.2 && Some(&style.0) == pick.style.as_ref());
            delete = ui
                .add_enabled(own, egui::Button::new("Delete style"))
                .on_hover_text("Shapes that use it fall back to no style; Undo brings it back")
                .on_disabled_hover_text("Choose a style made for this drawing; the built-in ones stay")
                .clicked();
            ui.separator();
            egui::CollapsingHeader::new("New style").show(ui, |ui| {
                let new = &mut pick.new;
                egui::Grid::new("new-style").num_columns(2).show(ui, |ui| {
                    for (name, value, hint) in [
                        ("Name", &mut new.name, ""),
                        ("Line colour", &mut new.line, "#rrggbb"),
                        ("Line weight (pt)", &mut new.weight, ""),
                        ("Fill colour", &mut new.fill, "#rrggbb"),
                        ("Text colour", &mut new.text, "#rrggbb"),
                        ("Font size (pt)", &mut new.size, ""),
                    ] {
                        let label = ui.label(name);
                        ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(140.0)).labelled_by(label.id);
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut new.bold, "Bold");
                    ui.checkbox(&mut new.italic, "Italic");
                });
                save = ui.button("Save style").on_hover_text("What is left empty comes from the Normal style. A style of this name made for the drawing is replaced.").clicked();
            });
        });
        let request = pick.style.clone().filter(|_| apply).map(|style| format!("{style}|{}|{}", pick.scope, selected.join(",")));
        let doomed = pick.style.clone().filter(|_| delete);
        let described = save.then(|| (pick.new.name.trim().to_string(), pick.new.spec()));
        if !open || (!self.focus_at_frame_start && ctx.input(|input| input.key_pressed(egui::Key::Escape))) {
            self.style_pick = None;
        }
        match described {
            Some((name, Ok(spec))) => {
                let result = self.engine.page_op(37, self.page_index, 0, &spec.to_string()).map(|_| ());
                let saved = result.is_ok();
                let primary = self.selected.clone();
                self.finish_edit(ctx, result, "Saved the style", primary);
                // the new style is the chosen one, ready to apply
                let made = self.style_list().into_iter().find(|style| style.1 == name).map(|style| style.0);
                if let Some(pick) = self.style_pick.as_mut().filter(|_| saved) {
                    pick.style = made;
                }
            }
            Some((_, Err(why))) => self.status = why,
            None => {}
        }
        if let Some(style) = doomed {
            let result = self.engine.page_op(38, self.page_index, 0, &style).map(|_| ());
            let primary = self.selected.clone();
            self.finish_edit(ctx, result, "Deleted the style", primary);
        }
        if let Some(request) = request {
            let result = self.engine.page_op(36, self.page_index, 0, &request).map(|_| ());
            let primary = self.selected.clone();
            self.finish_edit(ctx, result, "Applied the style", primary);
        }
    }
}
