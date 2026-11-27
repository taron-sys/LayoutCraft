//! The File tab: a page over the whole window with the file commands at the side and, in the middle, a new drawing, a path
//! to open and the drawings opened lately. Choosing something, Back or Esc return to the tab that was shown before.
use eframe::egui;

use super::{read_recent, recent_file, Action, LayoutCraftApp};

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
                if ui.add(egui::Button::new("Blank drawing").min_size(egui::vec2(150.0, 90.0))).clicked() {
                    action = Some(Action::NewDrawing);
                }
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
                for path in recent {
                    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
                    let folder = path.parent().map(|folder| folder.display().to_string()).unwrap_or_default();
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(name).min_size(egui::vec2(260.0, 26.0))).on_hover_text(path.display().to_string()).clicked() {
                            action = Some(Action::OpenRecent(path.clone()));
                        }
                        ui.add(egui::Label::new(egui::RichText::new(folder).weak()).truncate());
                    });
                }
            });
        });
        if back || action.is_some() {
            self.ribbon_tab = self.tab_before_file;
            self.ribbon_peek = false;
        }
        action
    }
}
