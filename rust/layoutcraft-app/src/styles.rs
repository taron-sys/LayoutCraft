//! The Styles window: give the selection one of the drawing's styles. The styles and the change are the engine's; this file
//! asks and draws.
use super::LayoutCraftApp;
use eframe::egui;

/// What of a style is applied, with the words the engine takes.
const SCOPES: [(&str, &str); 4] = [("all", "Line, fill and text"), ("line", "Line only"), ("fill", "Fill only"), ("text", "Text only")];

/// The window's choice: the style's ID and the part of it to apply.
pub(super) struct StylePick {
    style: Option<String>,
    scope: &'static str,
}

impl Default for StylePick {
    fn default() -> Self {
        Self { style: None, scope: SCOPES[0].0 }
    }
}

impl LayoutCraftApp {
    /// The drawing's styles as (ID, name).
    fn style_list(&self) -> Vec<(String, String)> {
        let list = self.engine.styles_list_json().ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|value| value.as_array().cloned()).unwrap_or_default();
        list.iter().filter_map(|style| Some((style["id"].as_str()?.to_string(), style["name"].as_str()?.to_string()))).collect()
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
        let (mut open, mut apply) = (true, false);
        egui::Window::new("Styles").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            if styles.is_empty() {
                ui.label("This drawing has no styles.");
                return;
            }
            let label = ui.label("Style");
            let shown = styles.iter().find(|style| Some(&style.0) == pick.style.as_ref()).map_or("Choose a style", |style| style.1.as_str());
            egui::ComboBox::from_id_salt("style-pick")
                .selected_text(shown)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for (id, name) in &styles {
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
        });
        let request = pick.style.clone().filter(|_| apply).map(|style| format!("{style}|{}|{}", pick.scope, selected.join(",")));
        if !open || (!self.focus_at_frame_start && ctx.input(|input| input.key_pressed(egui::Key::Escape))) {
            self.style_pick = None;
        }
        if let Some(request) = request {
            let result = self.engine.page_op(36, self.page_index, 0, &request).map(|_| ());
            let primary = self.selected.clone();
            self.finish_edit(ctx, result, "Applied the style", primary);
        }
    }
}
