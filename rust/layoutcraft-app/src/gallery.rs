//! The Design tab's theme and variant pictures. Each picture is a small drawing of ours (`Engine::theme_sample`) drawn by our
//! own renderer; resting the pointer or the keyboard focus on one shows the page that way, and a click applies it.
use eframe::egui;
use layoutcraft_engine::Engine;
use layoutcraft_render::{render_rgba, scene_from_document};

use super::{current_theme, theme_op, Action, LayoutCraftApp};

/// The size of a picture in the ribbon, in logical pixels (the sample drawing is 384 x 264 scene pixels).
const TILE: egui::Vec2 = egui::vec2(64.0, 44.0);
const SAMPLE: (f64, f64) = (384.0, 264.0);
/// How long the pointer or the focus stays on a picture before the page is drawn that way, in seconds: passing over the row
/// does not draw every theme on the way.
const REST: f64 = 0.15;

/// The shown page as a theme choice would leave it.
pub(crate) struct Preview {
    choice: (i32, String),
    page: usize,
    pub(crate) texture: egui::TextureHandle,
}

/// How many theme pictures stand side by side in a window `width` wide; the rest are reached with the arrow buttons.
pub(crate) fn themes_in_view(width: f32) -> usize {
    (((width - 900.0) / (TILE.x + 12.0)).floor().max(0.0) as usize).clamp(3, 12)
}

/// The first theme of the row after stepping `by` rows of `shown` from `first`, among `count` themes: never past the end, so
/// that the last row is full.
pub(crate) fn stepped(first: usize, by: isize, shown: usize, count: usize) -> usize {
    let last = count.saturating_sub(shown);
    (first as isize + by * shown as isize).clamp(0, last as isize) as usize
}

fn sample_picture(ctx: &egui::Context, theme: &str, variant: u32) -> Option<egui::TextureHandle> {
    let document = Engine::theme_sample(theme, variant).ok()?;
    let mut scene = scene_from_document(&document, 0).ok()?;
    (scene.min_x, scene.min_y, scene.width, scene.height) = (0.0, 0.0, SAMPLE.0, SAMPLE.1);
    // twice the logical size, so that it stays sharp on a scaled screen
    let (width, height, rgba) = render_rgba(&scene, 2.0 * f64::from(TILE.x) / SAMPLE.0).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
    Some(ctx.load_texture(format!("theme-{theme}-{variant}"), image, egui::TextureOptions::LINEAR))
}

impl LayoutCraftApp {
    /// The picture of `theme` in `variant`: drawn when first asked for, then kept. Only one is drawn per frame (`budget`), so
    /// that opening the tab does not stall the window.
    fn theme_picture(&mut self, ctx: &egui::Context, theme: &str, variant: u32, budget: &mut usize) -> Option<egui::TextureHandle> {
        let key = (theme.to_string(), variant);
        if !self.theme_pictures.contains_key(&key) {
            if *budget == 0 {
                ctx.request_repaint();
                return None;
            }
            *budget -= 1;
            let picture = sample_picture(ctx, theme, variant);
            self.theme_pictures.insert(key.clone(), picture);
            ctx.request_repaint();
        }
        self.theme_pictures.get(&key).cloned().flatten()
    }

    /// One picture as a button: `name` is its accessible name and tooltip; the chosen one has a thick frame.
    fn theme_tile(&mut self, ui: &mut egui::Ui, picture: Option<egui::TextureHandle>, name: &str, chosen: bool) -> egui::Response {
        let enabled = ui.is_enabled();
        let (rect, response) = ui.allocate_exact_size(TILE + egui::vec2(6.0, 6.0), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, chosen, name));
        let visuals = ui.visuals();
        let frame = if chosen {
            egui::Stroke::new(3.0, visuals.selection.stroke.color)
        } else if response.hovered() || response.has_focus() {
            egui::Stroke::new(1.5, visuals.text_color())
        } else {
            egui::Stroke::new(1.0, visuals.weak_text_color())
        };
        let inner = rect.shrink(3.0);
        match picture {
            Some(texture) => {
                let all = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                let tint = if enabled { egui::Color32::WHITE } else { egui::Color32::from_white_alpha(90) };
                ui.painter().image(texture.id(), inner, all, tint);
            }
            // not drawn yet: the name stands in
            None => {
                ui.painter().text(inner.center(), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(9.0), ui.visuals().text_color());
            }
        }
        ui.painter().rect_stroke(rect.shrink(1.5), 2.0, frame, egui::StrokeKind::Middle);
        response.on_hover_text(name)
    }

    /// Note the picture under the pointer, or else the one with the keyboard focus, for `settle_theme_preview`. The chosen
    /// one needs no preview: the page shows it.
    fn look_at(&mut self, response: &egui::Response, chosen: bool, op: i32, value: &str) {
        if !chosen && response.enabled() && (response.hovered() || (response.has_focus() && self.theme_hover.is_none())) {
            self.theme_hover = Some((op, value.to_string()));
        }
    }

    /// After the ribbon: draw the page as the picture looked at would leave it once it has been looked at for a moment, and
    /// drop that drawing when it no longer is.
    pub(crate) fn settle_theme_preview(&mut self, ctx: &egui::Context) {
        let Some(choice) = self.theme_hover.clone() else {
            self.theme_resting = None;
            self.theme_preview = None;
            return;
        };
        if self.theme_preview.as_ref().is_some_and(|preview| preview.choice == choice && preview.page == self.page_index) {
            return;
        }
        let now = ctx.input(|input| input.time);
        let since = match &self.theme_resting {
            Some((resting, since)) if *resting == choice => *since,
            _ => {
                self.theme_resting = Some((choice.clone(), now));
                now
            }
        };
        if since.is_infinite() {
            // drawing it failed: not tried again until something else is looked at
            return;
        }
        if now - since < REST {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(REST - (now - since)));
            return;
        }
        match self.preview_picture(ctx, &choice) {
            Ok(texture) => {
                self.theme_preview = Some(Preview { choice, page: self.page_index, texture });
                ctx.request_repaint();
            }
            Err(error) => {
                self.theme_preview = None;
                self.theme_resting = Some((choice, f64::INFINITY));
                self.status = format!("Could not preview: {error}");
            }
        }
    }

    /// The shown page after theme operation `choice`, drawn at the page picture's scale so that it can stand in its place.
    fn preview_picture(&self, ctx: &egui::Context, choice: &(i32, String)) -> Result<egui::TextureHandle, String> {
        let document = self.engine.preview_theme(choice.0, self.page_index, &choice.1)?;
        let scene = self.page_scene(&document)?;
        let (width, height, rgba) = render_rgba(&scene, self.raster_scale)?;
        let image = egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
        Ok(ctx.load_texture("layoutcraft-theme-preview", image, egui::TextureOptions::LINEAR))
    }

    /// The row of theme pictures, with arrow buttons when not all fit. The list with names and the "all pages" box follows it
    /// (`ribbon_themes`).
    pub(crate) fn theme_strip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, action: &mut Option<Action>) {
        let Some(info) = self.theme_info() else { return };
        let (current, _) = current_theme(&info, self.page_index);
        let names: Vec<String> = info["themes"].as_array().into_iter().flatten().filter_map(|theme| theme["name"].as_str().map(str::to_string)).collect();
        let shown = themes_in_view(ctx.content_rect().width()).min(names.len());
        self.theme_first = stepped(self.theme_first, 0, shown, names.len());
        let op = theme_op(false, self.theme_all_pages);
        let mut budget = 1;
        ui.add_enabled_ui(self.texture.is_some(), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if ui.add_enabled(self.theme_first > 0, egui::Button::new("\u{2039}").min_size(egui::vec2(16.0, TILE.y))).on_hover_text("Earlier themes").clicked() {
                    self.theme_first = stepped(self.theme_first, -1, shown, names.len());
                }
                for name in names.iter().skip(self.theme_first).take(shown) {
                    let picture = self.theme_picture(ctx, name, 0, &mut budget);
                    let chosen = current.as_deref() == Some(name.as_str());
                    let response = self.theme_tile(ui, picture, &format!("Theme {name}"), chosen);
                    self.look_at(&response, chosen, op, name);
                    if response.clicked() {
                        *action = Some(Action::Theme(op, name.clone()));
                    }
                }
                if ui.add_enabled(self.theme_first + shown < names.len(), egui::Button::new("\u{203a}").min_size(egui::vec2(16.0, TILE.y))).on_hover_text("More themes").clicked() {
                    self.theme_first = stepped(self.theme_first, 1, shown, names.len());
                }
            });
        });
    }

    /// The four colour variants of the shown theme as pictures.
    pub(crate) fn variant_strip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, action: &mut Option<Action>) {
        let Some(info) = self.theme_info() else { return };
        let (current, variant) = current_theme(&info, self.page_index);
        let Some(theme) = current else {
            ui.label(egui::RichText::new("Choose a theme to see its variants.").weak());
            return;
        };
        let op = theme_op(true, self.theme_all_pages);
        let mut budget = 1;
        ui.add_enabled_ui(self.texture.is_some(), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for choice in 0..4u32 {
                    let picture = self.theme_picture(ctx, &theme, choice, &mut budget);
                    let chosen = u64::from(choice) == variant;
                    let response = self.theme_tile(ui, picture, &format!("Variant {} of {theme}", choice + 1), chosen);
                    self.look_at(&response, chosen, op, &choice.to_string());
                    if response.clicked() {
                        *action = Some(Action::Theme(op, choice.to_string()));
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{stepped, themes_in_view};

    #[test]
    fn a_wider_window_shows_more_themes_and_the_arrows_stop_at_both_ends() {
        assert!(themes_in_view(1920.0) > themes_in_view(1300.0));
        assert!(themes_in_view(200.0) >= 1);
        let (shown, count) = (5, 14);
        let mut first = 0;
        let mut seen = vec![false; count];
        for _ in 0..4 {
            seen.iter_mut().skip(first).take(shown).for_each(|seen| *seen = true);
            first = stepped(first, 1, shown, count);
        }
        assert!(seen.iter().all(|seen| *seen), "{seen:?}");
        assert_eq!(first + shown, count, "the last row is full");
        assert_eq!(stepped(0, -1, shown, count), 0);
        assert_eq!(stepped(3, 1, 20, count), 0, "everything fits");
    }
}
