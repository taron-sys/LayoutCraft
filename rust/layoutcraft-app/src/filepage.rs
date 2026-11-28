//! The File tab: a page over the whole window with the file commands at the side and, in the middle, a new drawing, a path
//! to open and the drawings opened lately. Choosing something, Back or Esc return to the tab that was shown before.
use eframe::egui;

use super::{iso_from_unix, name_control, read_recent, recent_file, Action, LayoutCraftApp};

/// The empty drawings offered: name, paper, page width and height in inches, and whether it measures in millimetres.
/// The first is what "New" makes.
const BLANK_PAGES: [(&str, &str, f64, f64, bool); 4] = [
    ("Blank drawing", "Letter, landscape", 11.0, 8.5, false),
    ("Letter portrait", "8.5 \u{d7} 11 in", 8.5, 11.0, false),
    ("A4 landscape", "297 \u{d7} 210 mm", 297.0 / 25.4, 210.0 / 25.4, true),
    ("A4 portrait", "210 \u{d7} 297 mm", 210.0 / 25.4, 297.0 / 25.4, true),
];

/// A button showing an empty page of the given shape (width over height) with the name and paper under it.
fn page_tile(ui: &mut egui::Ui, name: &str, paper: &str, shape: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(128.0, 140.0), egui::Sense::click());
    name_control(ui, &response, &format!("{name}, {paper}"));
    let visuals = ui.visuals();
    let (strong, text, accent) = (visuals.strong_text_color(), visuals.text_color(), visuals.selection.stroke.color);
    let space = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 100.0)).shrink(8.0);
    let size = if shape >= space.width() / space.height() { egui::vec2(space.width(), space.width() / shape) } else { egui::vec2(space.height() * shape, space.height()) };
    let page = egui::Rect::from_center_size(space.center(), size);
    let painter = ui.painter();
    painter.rect_filled(page, 0.0, egui::Color32::WHITE);
    painter.rect_stroke(page, 0.0, egui::Stroke::new(1.0, egui::Color32::GRAY), egui::StrokeKind::Outside);
    painter.text(egui::pos2(rect.center().x, space.bottom() + 14.0), egui::Align2::CENTER_TOP, name, egui::FontId::proportional(13.0), strong);
    painter.text(egui::pos2(rect.center().x, space.bottom() + 31.0), egui::Align2::CENTER_TOP, paper, egui::FontId::proportional(11.0), text);
    if response.hovered() || response.has_focus() {
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
    }
    response
}

/// One recently opened drawing: its name over its folder, and when the file was last changed.
fn recent_row(ui: &mut egui::Ui, path: &std::path::Path, changed: &str) -> egui::Response {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    let folder = path.parent().map(|folder| folder.display().to_string()).unwrap_or_default();
    let width = ui.available_width().min(900.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::click());
    name_control(ui, &response, &format!("{name}, in {folder}, changed {changed}"));
    let visuals = ui.visuals();
    let (strong, text, accent, hover) = (visuals.strong_text_color(), visuals.text_color(), visuals.selection.stroke.color, visuals.widgets.hovered.weak_bg_fill);
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, hover);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
    }
    let room = (width - 150.0).max(60.0);
    for (line, size, colour, top) in [(name, 14.0, strong, 3.0), (folder, 11.0, text, 22.0)] {
        let mut job = egui::text::LayoutJob::simple_singleline(line, egui::FontId::proportional(size), colour);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(rect.min + egui::vec2(6.0, top), galley, colour);
    }
    ui.painter().text(egui::pos2(rect.right() - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, changed, egui::FontId::proportional(12.0), text);
    response
}

/// How long before `now` the time `changed` was, in words; the date itself when it was a month ago or more.
fn age_text(changed: std::time::SystemTime, now: std::time::SystemTime) -> String {
    let age = now.duration_since(changed).map_or(0, |age| age.as_secs());
    match age {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{} min ago", age / 60),
        3_600..=86_399 => format!("{} h ago", age / 3_600),
        86_400..=172_799 => "1 day ago".to_string(),
        172_800..=2_591_999 => format!("{} days ago", age / 86_400),
        _ => {
            let seconds = changed.duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64);
            iso_from_unix(seconds).chars().take(10).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::age_text;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_change_is_told_by_its_age_and_an_old_one_by_its_date() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let ago = |seconds: u64| age_text(now - Duration::from_secs(seconds), now);
        let told = [ago(5), ago(600), ago(7_200), ago(90_000), ago(500_000), ago(9_000_000)];
        assert!(told[0] == "just now" && told[1].ends_with("min ago") && told[2].ends_with("h ago"), "{told:?}");
        assert!(told[3].starts_with("1 day") && told[4].ends_with("days ago"), "{told:?}");
        assert!(told[5].len() == 10 && told[5].matches('-').count() == 2, "{told:?}");
        // a file changed "later than now" (another machine's clock) is not an error
        assert_eq!(age_text(now + Duration::from_secs(60), now), "just now");
    }
}

impl LayoutCraftApp {
    /// Whether the File page is what the window shows.
    pub(crate) fn file_page_open(&self) -> bool {
        !self.full_screen && self.ribbon.tabs.get(self.ribbon_tab).is_some_and(|tab| tab.name == "File")
    }

    /// Draw the File page; what was chosen is returned, and then (as for Back) the earlier tab is shown again.
    pub(crate) fn file_page(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let mut action = None;
        let mut back = !self.focus_at_frame_start && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
            action = Some(Action::SaveCopy);
        }
        let (open, saved) = (self.engine.export().is_some(), self.loaded_path.is_some());
        egui::Panel::left("file-commands").resizable(false).default_size(190.0).show(ui, |ui| {
            let wide = egui::vec2(ui.available_width(), 30.0);
            let command = |ui: &mut egui::Ui, enabled: bool, label: &str| ui.add_enabled(enabled, egui::Button::new(label).frame(false).min_size(wide)).clicked();
            ui.add_space(8.0);
            back |= command(ui, true, "\u{2190} Back");
            ui.separator();
            for (enabled, label, chosen) in [
                (true, "New", Action::NewDrawing),
                (true, "Open...", Action::OpenDialog),
                (saved, "Reload", Action::Reload),
                (saved, "Save a copy", Action::SaveCopy),
                (open, "Save as...", Action::SaveAs),
                (open, "Export...", Action::Export),
                (open, "Copy as Picture", Action::CopyPicture),
                (open, "Document Properties", Action::DocProps),
            ] {
                if command(ui, enabled, label) {
                    action = Some(chosen);
                }
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(8.0);
                ui.heading("New");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for (name, paper, width, height, metric) in BLANK_PAGES {
                        if page_tile(ui, name, paper, (width / height) as f32).clicked() {
                            action = Some(Action::NewSized(width, height, metric));
                        }
                    }
                });
                ui.add_space(18.0);
                ui.heading("Open");
                ui.horizontal(|ui| {
                    let label = ui.label("Path");
                    let width = (ui.available_width() - 90.0).clamp(120.0, 520.0);
                    let field = ui.add(egui::TextEdit::singleline(&mut self.path_input).hint_text(r"path\to\drawing.vsdx").desired_width(width)).labelled_by(label.id);
                    let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if ui.button("Load").clicked() || entered {
                        action = Some(Action::Load);
                    }
                });
                ui.add_space(18.0);
                ui.heading("Recent");
                let recent = recent_file().map(|file| read_recent(&file)).unwrap_or_default();
                if recent.is_empty() {
                    ui.label("No drawings opened yet.");
                }
                if self.recent_dates.iter().map(|(path, _)| path).ne(recent.iter()) {
                    let now = std::time::SystemTime::now();
                    self.recent_dates = recent.iter().map(|path| (path.clone(), std::fs::metadata(path).and_then(|file| file.modified()).map(|changed| age_text(changed, now)).unwrap_or_default())).collect();
                }
                for (path, changed) in &self.recent_dates {
                    if recent_row(ui, path, changed).on_hover_text(path.display().to_string()).clicked() {
                        action = Some(Action::OpenRecent(path.clone()));
                    }
                    ui.separator();
                }
            });
        });
        if back || action.is_some() {
            self.ribbon_tab = self.tab_before_file;
            self.ribbon_peek = false;
            // read the dates afresh the next time
            self.recent_dates.clear();
        }
        action
    }
}
